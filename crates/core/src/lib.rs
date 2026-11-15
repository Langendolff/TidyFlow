pub mod actions;
pub mod config;
pub mod facts;
pub mod journal;
pub mod rules;
pub mod template;
pub mod units;

use std::path::PathBuf;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// Diretório XDG (`var`) ou `$HOME/<fallback>` quando a variável está vazia ou ausente.
pub(crate) fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home()).join(fallback))
}
