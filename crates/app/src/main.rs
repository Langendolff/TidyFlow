// Sem janela de console no Windows (em debug ela fica, para ver os logs).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod dto;
mod system;

use commands::AppState;
use tauri::{Emitter, Manager};
use tidyflow_core::{config, journal};

/// `--test <arquivo>` (menu de contexto "Testar no TidyFlow").
fn test_arg(args: &[String]) -> Option<String> {
    let i = args.iter().position(|a| a == "--test")?;
    args.get(i + 1).cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    tauri::Builder::default()
        // Precisa ser o primeiro plugin: uma segunda execução só foca a janela aberta.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            if let Some(path) = test_arg(&argv) {
                let _ = app.emit("test-file", path);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            config_path: config::default_path(),
            journal_path: journal::default_path(),
            initial_test_file: test_arg(&args),
        })
        .invoke_handler(tauri::generate_handler![
            commands::overview,
            commands::scan,
            commands::history,
            commands::undo,
            commands::rules_list,
            commands::save_rule,
            commands::delete_rule,
            commands::move_rule,
            commands::set_enabled,
            commands::preview,
            commands::open_config,
            commands::inspect,
            commands::organize,
            commands::initial_test_file,
            commands::daemon_status,
            commands::start_daemon,
            commands::live,
            commands::library,
            commands::library_sites,
            commands::open_file,
            commands::reveal,
            commands::open_url,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o TidyFlow");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_test_arg() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            test_arg(&a(&["app", "--test", "/x y.pdf"])).as_deref(),
            Some("/x y.pdf")
        );
        assert_eq!(test_arg(&a(&["app"])), None);
        assert_eq!(test_arg(&a(&["app", "--test"])), None);
    }
}
