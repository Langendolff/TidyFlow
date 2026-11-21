pub mod browser;
pub mod daemon;
pub mod desktop;
pub mod native_host;
use clap::{Parser, Subcommand};
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    time::SystemTime,
};
use tidyflow_core::{
    actions, config,
    engine::{self, Report},
    journal::{self, Entry, Journal},
    meta::{self, Tag},
    rules, service,
};

#[derive(Parser)]
#[command(
    name = "tidyflow",
    version,
    about = "Organiza arquivos automaticamente por regras"
)]
struct Cli {
    /// Caminho do config (padrão: ~/.config/tidyflow/config.toml)
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Monitora as pastas e organiza em tempo real
    Daemon,
    /// Varre as pastas monitoradas uma vez
    Run {
        /// Só mostra o que seria feito
        #[arg(long)]
        dry_run: bool,
    },
    /// Organiza os arquivos dados com a primeira regra ativa que casar,
    /// mesmo fora das pastas monitoradas (usado pelo menu de contexto)
    Organize {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Desfaz a última ação, ou a ação <ID>
    Undo { id: Option<i64> },
    /// Mostra as últimas ações
    History {
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
    /// Valida o config; com <ARQUIVO>, mostra qual regra casaria
    Check {
        file: Option<PathBuf>,
        /// Tira de <ARQUIVO> a marca de desfeito (o TidyFlow volta a organizá-lo)
        #[arg(long, requires = "file")]
        clear_keep: bool,
    },
    /// Host de native messaging (aberto pelo navegador, não pelo usuário)
    #[command(hide = true)]
    NativeHost {
        /// O Firefox passa o manifest e o ID da extensão; o Chromium, a origem.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        _args: Vec<String>,
    },
    /// Integração com a extensão de navegador
    Browser {
        #[command(subcommand)]
        cmd: BrowserCmd,
    },
    /// Daemon em segundo plano (systemd, launchd ou login do Windows)
    Service {
        #[command(subcommand)]
        cmd: ServiceCmd,
    },
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// Liga o daemon no login e inicia agora
    Enable,
    /// Desliga o daemon e tira do login
    Disable,
    /// Inicia o daemon
    Start,
    /// Mostra se o daemon está rodando
    Status,
}

#[derive(Subcommand)]
enum BrowserCmd {
    /// Instala o host de native messaging para os navegadores encontrados
    Install,
    /// Remove o host de native messaging
    Uninstall,
}

/// O navegador abre o host com argumentos próprios: o Firefox passa o caminho do
/// manifest e o ID da extensão; o Chromium, a origem `chrome-extension://…/`.
fn browser_args(args: &[std::ffi::OsString]) -> bool {
    args.get(1)
        .and_then(|a| a.to_str())
        .is_some_and(|a| a.starts_with("chrome-extension://") || a.ends_with(".json"))
}

pub fn main() -> ExitCode {
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if browser_args(&args) {
        args.insert(1, "native-host".into());
    }
    let cli = Cli::parse_from(args);
    init_log(matches!(cli.cmd, Cmd::Daemon));
    let config_path = cli.config.unwrap_or_else(config::default_path);
    match run(cli.cmd, &config_path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erro: {e}");
            ExitCode::FAILURE
        }
    }
}

fn init_log(daemon: bool) {
    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    // No Windows o daemon roda sem console: o log vai para um arquivo.
    if daemon && cfg!(windows) {
        let dir = tidyflow_core::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(f) = std::fs::File::options()
            .create(true)
            .append(true)
            .open(dir.join("daemon.log"))
        {
            tracing_subscriber::fmt()
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(f))
                .with_env_filter(filter)
                .init();
            return;
        }
    }
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();
}

fn run(cmd: Cmd, config_path: &Path) -> tidyflow_core::Result<()> {
    match cmd {
        Cmd::Daemon => daemon::run(config_path, &journal::default_path())?,
        Cmd::NativeHost { .. } => native_host::run(&journal::default_path())?,
        Cmd::Browser { cmd } => {
            let home = std::env::home_dir().ok_or("pasta pessoal desconhecida")?;
            match cmd {
                BrowserCmd::Install => {
                    // No Windows, o `tidyflowd.exe`: o navegador não abre janela de console.
                    let exe = if cfg!(windows) {
                        service::daemon_exe()
                    } else {
                        std::env::current_exe()?
                    };
                    for f in browser::install(&home, &exe)? {
                        println!("gravado: {}", f.display());
                    }
                    println!("Agora instale a extensão do TidyFlow no navegador (veja o README).");
                }
                BrowserCmd::Uninstall => {
                    for f in browser::uninstall(&home)? {
                        println!("removido: {}", f.display());
                    }
                }
            }
        }
        Cmd::Service { cmd } => match cmd {
            ServiceCmd::Enable => {
                let path = service::enable(&service::daemon_exe())?;
                println!("serviço instalado: {}", path.display());
                println!("o daemon sobe no login e já foi iniciado");
            }
            ServiceCmd::Disable => {
                service::disable()?;
                println!("daemon desligado e fora do login");
            }
            ServiceCmd::Start => service::start()?,
            ServiceCmd::Status => {
                let s = service::status();
                match (s.active, s.since) {
                    (true, Some(t)) => println!("ativo desde {}", local_time(t)),
                    (true, None) => println!("ativo"),
                    (false, _) => println!("parado"),
                }
            }
        },
        Cmd::Run { dry_run } => {
            let cfg = config::load(config_path)?;
            let journal = Journal::open(&journal::default_path())?;
            for (path, report) in engine::scan(&cfg, &journal, dry_run, SystemTime::now()) {
                match report {
                    Ok(Report::NoMatch) => {}
                    Ok(Report::Planned { rule, kind, dst }) => {
                        println!(
                            "[{rule}] {} {} → {}",
                            kind.as_str(),
                            path.display(),
                            target(dst.as_deref())
                        )
                    }
                    Ok(Report::Done { id, rule, done }) => println!(
                        "#{id} [{rule}] {} {} → {}",
                        done.kind.as_str(),
                        path.display(),
                        target(done.dst.as_deref())
                    ),
                    Ok(Report::Failed { rule, error }) => {
                        eprintln!("[{rule}] falhou em {}: {error}", path.display())
                    }
                    Err(e) => eprintln!("{}: {e}", path.display()),
                }
            }
        }
        Cmd::Organize { files } => {
            let cfg = config::load(config_path)?;
            let journal_path = journal::default_path();
            let journal = Journal::open(&journal_path)?;
            let mut failed = false;
            for path in files {
                match engine::organize(&cfg, &journal, &path, SystemTime::now()) {
                    Ok(Report::Done { id, rule, done }) => {
                        println!(
                            "#{id} [{rule}] {} {} → {}",
                            done.kind.as_str(),
                            path.display(),
                            target(done.dst.as_deref())
                        );
                        if cfg.notify {
                            desktop::organized(journal_path.clone(), id, &done, &rule);
                        }
                    }
                    Ok(Report::Failed { rule, error }) => {
                        failed = true;
                        eprintln!("[{rule}] falhou em {}: {error}", path.display());
                        if cfg.notify {
                            desktop::error(&format!("{}: {error}", path.display()));
                        }
                    }
                    Ok(_) => {
                        println!("nenhuma regra casou: {}", path.display());
                        if cfg.notify {
                            desktop::summary(&format!(
                                "Nenhuma regra casou com {}",
                                path.file_name().unwrap_or_default().to_string_lossy()
                            ));
                        }
                    }
                    Err(e) => {
                        failed = true;
                        eprintln!("{}: {e}", path.display());
                    }
                }
            }
            // O botão Desfazer só funciona enquanto o processo estiver vivo.
            desktop::wait(std::time::Duration::from_secs(300));
            if failed {
                return Err("algum arquivo não foi organizado".into());
            }
        }
        Cmd::Undo { id } => {
            let journal = Journal::open(&journal::default_path())?;
            let e = engine::undo(&journal, id)?;
            println!("desfeito #{}: {}", e.id, e.src.display());
        }
        Cmd::History { limit } => {
            let journal = Journal::open(&journal::default_path())?;
            journal.sweep_gone()?;
            for e in journal.recent(limit)? {
                println!("{}", format_entry(&e));
            }
        }
        Cmd::Check {
            file: Some(file),
            clear_keep: true,
        } => {
            meta::remove(&file, Tag::Keep)?;
            println!("marca de desfeito removida: {}", file.display());
        }
        Cmd::Check { file, .. } => {
            let cfg = config::load(config_path)?;
            println!("config ok: {} regra(s)", cfg.rules.len());
            for r in cfg.rules.iter().filter(|r| !r.enabled) {
                println!("pausada: {}", r.name);
            }
            for dir in cfg.missing_watch_dirs() {
                println!("aviso: pasta watch não existe: {}", dir.display());
            }
            if let Some(file) = file {
                let journal = Journal::open(&journal::default_path())?;
                let f = engine::facts(&journal, &file)?;
                println!(
                    "mime: {}\nextensão: {}\ntamanho: {} bytes",
                    f.mime, f.ext, f.size
                );
                println!("origem: {}", f.origin_url.as_deref().unwrap_or("(nenhuma)"));
                println!(
                    "página de origem: {}",
                    f.referrer_url.as_deref().unwrap_or("(nenhuma)")
                );
                println!("site: {}", f.site().as_deref().unwrap_or("(desconhecido)"));
                let by_journal = journal.was_undone_src(&file)?;
                if f.keep || by_journal {
                    println!(
                        "marcado como desfeito: o TidyFlow não vai mexer neste arquivo (remova com: tidyflow check --clear-keep <arquivo>)"
                    );
                    if !f.keep {
                        println!(
                            "(marca pelo histórico: só vale enquanto o histórico tiver essa entrada)"
                        );
                    }
                }
                match rules::first_match(&cfg.rules, &f, SystemTime::now()) {
                    Some(r) => println!(
                        "regra: {} → {}",
                        r.name,
                        target(actions::destination(&r.action, &f).as_deref())
                    ),
                    None => println!("nenhuma regra casa com este arquivo"),
                }
            }
        }
    }
    Ok(())
}

fn target(dst: Option<&Path>) -> String {
    dst.map_or("lixeira".into(), |d| d.display().to_string())
}

fn local_time(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

fn format_entry(e: &Entry) -> String {
    let when = local_time(e.ts);
    let status = match (&e.error, e.undone) {
        (Some(err), _) => format!("  (erro: {err})"),
        (None, true) => "  (desfeito)".into(),
        (None, false) => String::new(),
    };
    format!(
        "#{} {when} [{}] {} {} → {}{status}",
        e.id,
        e.rule,
        e.kind.as_str(),
        e.src.display(),
        target(e.dst.as_deref())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_browser_launch() {
        let a = |v: &[&str]| {
            v.iter()
                .map(Into::into)
                .collect::<Vec<std::ffi::OsString>>()
        };
        assert!(browser_args(&a(&["tidyflow", "chrome-extension://abc/"])));
        assert!(browser_args(&a(&[
            "tidyflow",
            r"C:\Users\x\AppData\Roaming\TidyFlow\native-host\firefox.json",
            "tidyflow@langendolff.github.io"
        ])));
        assert!(!browser_args(&a(&["tidyflow", "run"])));
        assert!(!browser_args(&a(&["tidyflow"])));
    }
}
