pub mod actions;
pub mod config;
pub mod downloads;
pub mod engine;
pub mod facts;
pub mod journal;
pub mod meta;
pub mod rules;
pub mod service;
pub mod stability;
pub mod template;
pub mod units;

use std::path::PathBuf;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

/// Pasta pessoal: `HOME` no Unix, `USERPROFILE` no Windows.
pub fn home() -> String {
    std::env::home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Diretório XDG (`var`) ou `$HOME/<fallback>` quando a variável está vazia ou ausente.
#[cfg(not(any(target_os = "macos", windows)))]
fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home()).join(fallback))
}

/// Caminho na forma que o watcher entrega. No macOS o FSEvents dá caminhos reais
/// (`/private/var/…` para `/var/…`), então caminhos vindos de fora (navegador,
/// linha de comando) passam pela pasta real; nos outros sistemas, inalterado.
pub fn real_path(p: &std::path::Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    if let (Some(dir), Some(name)) = (p.parent(), p.file_name())
        && let Ok(real) = dir.canonicalize()
    {
        return real.join(name);
    }
    p.to_path_buf()
}

/// Pasta do config: `~/.config/tidyflow` (XDG), `~/Library/Application Support/TidyFlow`
/// no macOS e `%APPDATA%\TidyFlow` no Windows.
pub fn config_dir() -> PathBuf {
    #[cfg(not(any(target_os = "macos", windows)))]
    return xdg_dir("XDG_CONFIG_HOME", ".config").join("tidyflow");
    #[cfg(any(target_os = "macos", windows))]
    return app_dir();
}

/// Pasta dos dados (histórico, trava do daemon): `~/.local/share/tidyflow` (XDG);
/// no macOS e no Windows, a mesma do config. `TIDYFLOW_DATA_DIR` sobrepõe (testes).
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("TIDYFLOW_DATA_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(d);
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    return xdg_dir("XDG_DATA_HOME", ".local/share").join("tidyflow");
    #[cfg(any(target_os = "macos", windows))]
    return app_dir();
}

#[cfg(target_os = "macos")]
fn app_dir() -> PathBuf {
    PathBuf::from(home()).join("Library/Application Support/TidyFlow")
}

#[cfg(windows)]
fn app_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home()).join("AppData").join("Roaming"))
        .join("TidyFlow")
}
