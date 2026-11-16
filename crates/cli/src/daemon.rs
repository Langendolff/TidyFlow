use crate::desktop;
use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{AccessKind, AccessMode, ModifyKind, RenameMode},
};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant, SystemTime},
};
use tidyflow_core::{
    actions::Done,
    config::{self, Config},
    engine::{self, Report},
    journal::Journal,
    stability,
};
use tracing::{debug, error, info, warn};

const RETRY: Duration = Duration::from_secs(60);
const QUIET: Duration = Duration::from_secs(2);
const MAX_WAIT: Duration = Duration::from_secs(60);
const LOOP_GUARD_SECS: u64 = 5;

struct Daemon {
    config_path: PathBuf,
    journal_path: PathBuf,
    cfg: Config,
    journal: Journal,
    watcher: RecommendedWatcher,
    watched: Vec<PathBuf>,
    missing: Vec<PathBuf>,
    next_scan: Instant,
}

pub fn run(config_path: &Path, journal_path: &Path) -> tidyflow_core::Result<()> {
    let config_path = config_path.canonicalize()?;
    let cfg = config::load(&config_path)?;
    let journal = Journal::open(journal_path)?;
    let (tx, rx) = mpsc::channel();
    // Carimba o evento na hora em que o watcher o emite, não quando sai da fila.
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send((SystemTime::now(), res));
    })?;
    // Observa a pasta, não o arquivo: editores salvam via rename e trocam o inode.
    let config_dir = config_path.parent().ok_or("caminho de config inválido")?;
    watcher.watch(config_dir, RecursiveMode::NonRecursive)?;

    let mut d = Daemon {
        config_path,
        journal_path: journal_path.to_path_buf(),
        cfg,
        journal,
        watcher,
        watched: Vec::new(),
        missing: Vec::new(),
        next_scan: Instant::now(),
    };
    d.rewatch();
    d.scan();
    info!("daemon iniciado com {} regra(s)", d.cfg.rules.len());

    let mut next_retry = Instant::now() + RETRY;
    loop {
        let deadline = if d.missing.is_empty() {
            d.next_scan
        } else {
            d.next_scan.min(next_retry)
        };
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((received, Ok(event))) => d.on_event(event, received),
            Ok((_, Err(e))) => warn!("erro do watcher: {e}"),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err("watcher encerrado".into()),
        }
        // Depois de qualquer resultado, para um fluxo constante de eventos não matar os timers.
        let now = Instant::now();
        if !d.missing.is_empty() && now >= next_retry {
            next_retry = now + RETRY;
            let before = d.missing.len();
            d.rewatch();
            if d.missing.len() < before {
                d.scan();
            }
        }
        if now >= d.next_scan {
            d.scan();
        }
    }
}

impl Daemon {
    fn rewatch(&mut self) {
        for dir in self.watched.drain(..) {
            let _ = self.watcher.unwatch(&dir);
        }
        let mut dirs: Vec<PathBuf> = self
            .cfg
            .rules
            .iter()
            .flat_map(|r| r.watch.clone())
            .collect();
        dirs.sort();
        dirs.dedup();
        let was_missing = std::mem::take(&mut self.missing);
        for dir in dirs {
            match self.watcher.watch(&dir, RecursiveMode::NonRecursive) {
                Ok(()) => self.watched.push(dir),
                Err(e) => {
                    warn!("não foi possível monitorar {}: {e}", dir.display());
                    if !was_missing.contains(&dir) {
                        self.alert(&format!("Não foi possível monitorar {}", dir.display()));
                    }
                    self.missing.push(dir);
                }
            }
        }
    }

    fn on_event(&mut self, event: Event, received: SystemTime) {
        if matches!(event.kind, EventKind::Remove(_)) {
            for p in &event.paths {
                if let Some(i) = self.watched.iter().position(|w| w == p) {
                    let dir = self.watched.remove(i);
                    warn!("pasta monitorada removida: {}", dir.display());
                    self.alert(&format!("Pasta monitorada removida: {}", dir.display()));
                    self.missing.push(dir);
                }
            }
            return;
        }
        let path = match event.kind {
            EventKind::Access(AccessKind::Close(AccessMode::Write))
            | EventKind::Modify(ModifyKind::Name(RenameMode::To)) => event.paths.first(),
            _ => None,
        };
        let Some(path) = path else { return };
        if *path == self.config_path {
            self.reload();
        } else {
            self.handle(path, received);
        }
    }

    fn handle(&self, path: &Path, received: SystemTime) {
        let in_watched = path
            .parent()
            .is_some_and(|p| self.watched.iter().any(|w| w == p));
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        // is_file: um FIFO poderia bloquear Facts::read.
        if !in_watched || stability::is_ignored_name(&name) || !path.is_file() {
            return;
        }
        match self
            .journal
            .is_dst_since(path, received - Duration::from_secs(LOOP_GUARD_SECS))
        {
            Ok(true) => return debug!("ignorando evento gerado por nós: {}", path.display()),
            Ok(false) => {}
            Err(e) => warn!("journal: {e}"),
        }
        // ponytail: espera em série; downloads simultâneos somam 2s cada.
        // Mover para uma fila com uma thread por arquivo se isso incomodar.
        if !stability::wait_stable(path, QUIET, MAX_WAIT) {
            return debug!("{} sumiu ou não estabilizou", path.display());
        }
        // Checado depois da espera: o .part pode surgir logo após o marcador.
        if stability::download_in_progress(path) {
            return debug!("{} ainda está baixando", path.display());
        }
        let result = engine::process(&self.cfg, &self.journal, path, false, SystemTime::now());
        self.report(path, result, true);
    }

    fn scan(&mut self) {
        self.next_scan = Instant::now() + self.cfg.age_scan_interval;
        let (mut done, mut failed) = (0, 0);
        for (path, result) in engine::scan(&self.cfg, &self.journal, false, SystemTime::now()) {
            match &result {
                Ok(Report::Done { .. }) => done += 1,
                Ok(Report::Failed { .. }) => failed += 1,
                _ => {}
            }
            self.report(&path, result, false);
        }
        if self.cfg.notify && (done > 0 || failed > 0) {
            let mut body = format!("{done} arquivo(s) organizado(s)");
            if failed > 0 {
                body += &format!(", {failed} falha(s), veja tidyflow history");
            }
            desktop::summary(&body);
        }
    }

    /// `single`: evento do watcher (notificação com Desfazer); senão, varredura (só resumo).
    fn report(&self, path: &Path, result: tidyflow_core::Result<Report>, single: bool) {
        match result {
            Ok(Report::Done { id, rule, done }) => {
                info!("#{id} [{rule}] {}", describe(&done));
                if single && self.cfg.notify {
                    desktop::organized(self.journal_path.clone(), id, &done, &rule);
                }
            }
            Ok(Report::Failed { rule, error }) => {
                warn!("[{rule}] falhou em {}: {error}", path.display());
                if single {
                    self.alert(&format!("{}: {error}", path.display()));
                }
            }
            Ok(_) => {}
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
            {
                debug!("{} sumiu antes da ação: {e}", path.display())
            }
            Err(e) => {
                warn!("{}: {e}", path.display());
                if single {
                    self.alert(&format!("{}: {e}", path.display()));
                }
            }
        }
    }

    fn reload(&mut self) {
        match config::load(&self.config_path) {
            Ok(cfg) => {
                self.cfg = cfg;
                self.next_scan = Instant::now() + self.cfg.age_scan_interval;
                self.rewatch();
                info!("config recarregado: {} regra(s)", self.cfg.rules.len());
            }
            Err(e) => {
                error!("config inválido, mantendo o anterior: {e}");
                self.alert(&format!("Config inválido: {e}"));
            }
        }
    }

    fn alert(&self, msg: &str) {
        if self.cfg.notify {
            desktop::error(msg);
        }
    }
}

fn describe(done: &Done) -> String {
    let name = done.src.file_name().unwrap_or_default().to_string_lossy();
    let place = done
        .dst
        .as_ref()
        .and_then(|d| d.parent())
        .map_or("lixeira".into(), |p| p.display().to_string());
    format!("{name} → {place}")
}
