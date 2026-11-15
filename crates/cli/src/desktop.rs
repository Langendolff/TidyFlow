use notify_rust::Notification;
use std::path::PathBuf;
use tidyflow_core::{engine, journal::Journal};
use tracing::warn;

// ponytail: uma thread por notificação, bloqueada até ela fechar. Trocar por um
// único loop D-Bus se muitas notificações simultâneas virarem problema.
pub fn organized(journal_path: PathBuf, id: i64, body: String) {
    std::thread::spawn(move || {
        let handle = match Notification::new()
            .appname("TidyFlow")
            .summary("TidyFlow")
            .body(&body)
            .action("undo", "Desfazer")
            .show()
        {
            Ok(h) => h,
            Err(e) => return warn!("notificação falhou: {e}"),
        };
        handle.wait_for_action(|action| {
            if action != "undo" {
                return;
            }
            // Cada thread abre a sua conexão; o WAL resolve a concorrência com o daemon.
            if let Err(e) = Journal::open(&journal_path).and_then(|j| engine::undo(&j, Some(id))) {
                error(&format!("Não foi possível desfazer: {e}"));
            }
        });
    });
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
