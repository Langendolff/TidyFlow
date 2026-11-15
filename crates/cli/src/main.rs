use clap::{Parser, Subcommand};
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    time::SystemTime,
};
use tidyflow_core::{
    actions, config,
    engine::{self, Report},
    facts::Facts,
    journal::{self, Entry, Journal},
    rules,
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
    /// Varre as pastas monitoradas uma vez
    Run {
        /// Só mostra o que seria feito
        #[arg(long)]
        dry_run: bool,
    },
    /// Desfaz a última ação, ou a ação <ID>
    Undo { id: Option<i64> },
    /// Mostra as últimas ações
    History {
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
    /// Valida o config; com <ARQUIVO>, mostra qual regra casaria
    Check { file: Option<PathBuf> },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let cli = Cli::parse();
    let config_path = cli.config.unwrap_or_else(config::default_path);
    match run(cli.cmd, &config_path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erro: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cmd: Cmd, config_path: &Path) -> tidyflow_core::Result<()> {
    match cmd {
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
        Cmd::Undo { id } => {
            let journal = Journal::open(&journal::default_path())?;
            let e = engine::undo(&journal, id)?;
            println!("desfeito #{}: {}", e.id, e.src.display());
        }
        Cmd::History { limit } => {
            let journal = Journal::open(&journal::default_path())?;
            for e in journal.recent(limit)? {
                println!("{}", format_entry(&e));
            }
        }
        Cmd::Check { file } => {
            let cfg = config::load(config_path)?;
            println!("config ok: {} regra(s)", cfg.rules.len());
            if let Some(file) = file {
                let f = Facts::read(&file)?;
                println!(
                    "mime: {}\nextensão: {}\ntamanho: {} bytes",
                    f.mime, f.ext, f.size
                );
                println!("origem: {}", f.origin_url.as_deref().unwrap_or("(nenhuma)"));
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

fn format_entry(e: &Entry) -> String {
    let when = chrono::DateTime::from_timestamp(e.ts, 0)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default();
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
