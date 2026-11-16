use notify_rust::Notification;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tidyflow_core::{
    actions::{Done, Kind},
    engine,
    journal::Journal,
};
use tracing::warn;

/// Threads das notificações abertas, para `wait` (a CLI não pode sair antes do clique).
static OPEN: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

// ponytail: uma thread por notificação, bloqueada até ela fechar. Trocar por um
// único loop D-Bus se muitas notificações simultâneas virarem problema.
pub fn organized(journal_path: PathBuf, id: i64, done: &Done, rule: &str) {
    let (summary, body) = message(done, rule);
    let folder = done
        .dst
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let handle = std::thread::spawn(move || {
        let mut n = Notification::new();
        n.appname("TidyFlow")
            .summary(&summary)
            .body(&body)
            .action("undo", "Desfazer");
        if folder.is_some() {
            n.action("open", "Abrir pasta");
        }
        let handle = match n.show() {
            Ok(h) => h,
            Err(e) => return warn!("notificação falhou: {e}"),
        };
        handle.wait_for_action(|action| match action {
            "undo" => {
                // Cada thread abre a sua conexão; o WAL resolve a concorrência com o daemon.
                if let Err(e) =
                    Journal::open(&journal_path).and_then(|j| engine::undo(&j, Some(id)))
                {
                    error(&format!("Não foi possível desfazer: {e}"));
                }
            }
            "open" => {
                if let Some(dir) = &folder
                    && let Err(e) = std::process::Command::new("xdg-open").arg(dir).spawn()
                {
                    warn!("xdg-open falhou: {e}");
                }
            }
            _ => {}
        });
    });
    if let Ok(mut open) = OPEN.lock() {
        open.retain(|h| !h.is_finished());
        open.push(handle);
    }
}

/// Espera as notificações abertas fecharem (no máximo `limit`).
pub fn wait(limit: Duration) {
    let start = Instant::now();
    loop {
        let pending = OPEN
            .lock()
            .map(|mut v| {
                v.retain(|h| !h.is_finished());
                v.len()
            })
            .unwrap_or(0);
        if pending == 0 || start.elapsed() >= limit {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Título e corpo da notificação, como no design.
pub fn message(done: &Done, rule: &str) -> (String, String) {
    let name = done.src.file_name().unwrap_or_default().to_string_lossy();
    let dir = |p: &Path| p.parent().map(tilde).unwrap_or_default();
    let body = match (done.kind, done.dst.as_deref()) {
        (Kind::Move, Some(d)) => format!("Movido para {} pela regra \"{rule}\"", dir(d)),
        (Kind::Copy, Some(d)) => format!("Copiado para {} pela regra \"{rule}\"", dir(d)),
        (Kind::Rename, Some(d)) => format!(
            "Renomeado para {} pela regra \"{rule}\"",
            d.file_name().unwrap_or_default().to_string_lossy()
        ),
        _ => format!("Enviado à lixeira pela regra \"{rule}\""),
    };
    (format!("{name} organizado"), body)
}

fn tilde(p: &Path) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    match p.strip_prefix(&home) {
        Ok(rest) if !home.is_empty() => format!("~/{}", rest.display()),
        _ => p.display().to_string(),
    }
}

pub fn error(body: &str) {
    if let Err(e) = Notification::new()
        .appname("TidyFlow")
        .summary("TidyFlow: erro")
        .body(body)
        .show()
    {
        warn!("notificação falhou: {e}");
    }
}

pub fn summary(body: &str) {
    if let Err(e) = Notification::new()
        .appname("TidyFlow")
        .summary("TidyFlow")
        .body(body)
        .show()
    {
        warn!("notificação falhou: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages() {
        let home = std::env::var("HOME").unwrap();
        let mv = Done {
            kind: Kind::Move,
            src: "/dl/fatura.pdf".into(),
            dst: Some(format!("{home}/Docs/2026/fatura.pdf").into()),
        };
        assert_eq!(
            message(&mv, "Faturas"),
            (
                "fatura.pdf organizado".to_string(),
                "Movido para ~/Docs/2026 pela regra \"Faturas\"".to_string()
            )
        );
        let tr = Done {
            kind: Kind::Trash,
            src: "/dl/a.iso".into(),
            dst: None,
        };
        assert_eq!(message(&tr, "r").1, "Enviado à lixeira pela regra \"r\"");
        let rn = Done {
            kind: Kind::Rename,
            src: "/dl/a.xml".into(),
            dst: Some("/dl/2026-a.xml".into()),
        };
        assert_eq!(
            message(&rn, "r").1,
            "Renomeado para 2026-a.xml pela regra \"r\""
        );
    }
}
