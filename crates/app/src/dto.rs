//! Tipos que o front recebe. Caminhos vêm absolutos e também abreviados (`~`).

use serde::Serialize;
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tidyflow_core::{
    config::{ConfigError, edit::EditError},
    facts::Facts,
    journal::Entry,
    rules::Check,
};

/// Erro de qualquer comando. `kind`: `invalid` (config/regra inválida), `conflict`
/// (config.toml mudou no disco) ou `other`.
#[derive(Debug, Serialize)]
pub struct CmdError {
    pub kind: &'static str,
    pub message: String,
    pub line: Option<usize>,
}

pub type CmdResult<T> = Result<T, CmdError>;

impl CmdError {
    pub fn other(message: impl ToString) -> CmdError {
        CmdError {
            kind: "other",
            message: message.to_string(),
            line: None,
        }
    }
}

impl From<ConfigError> for CmdError {
    fn from(e: ConfigError) -> Self {
        CmdError {
            kind: "invalid",
            message: e.message,
            line: e.line,
        }
    }
}

impl From<EditError> for CmdError {
    fn from(e: EditError) -> Self {
        match e {
            EditError::Invalid(c) => c.into(),
            EditError::Conflict => CmdError {
                kind: "conflict",
                message: EditError::Conflict.to_string(),
                line: None,
            },
            e @ EditError::Io(_) => CmdError::other(e),
        }
    }
}

impl From<tidyflow_core::Error> for CmdError {
    fn from(e: tidyflow_core::Error) -> Self {
        CmdError::other(e)
    }
}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> Self {
        CmdError::other(e)
    }
}

/// `/home/x/Downloads/a.pdf` → `~/Downloads/a.pdf`.
pub fn tilde(p: &Path) -> String {
    tilde_with(p, &std::env::var("HOME").unwrap_or_default())
}

fn tilde_with(p: &Path, home: &str) -> String {
    if home.is_empty() {
        return p.display().to_string();
    }
    match p.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

pub fn unix(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// mtime exato como texto (nanossegundos não cabem num número do JavaScript).
pub fn mtime_token(t: Option<SystemTime>) -> Option<String> {
    t.and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().to_string())
}

#[derive(Serialize)]
pub struct EntryDto {
    pub id: i64,
    pub ts: i64,
    pub rule: String,
    pub kind: &'static str,
    pub name: String,
    pub src: String,
    pub src_display: String,
    pub dst: Option<String>,
    pub dst_display: Option<String>,
    pub size: Option<u64>,
    pub undone: bool,
    pub error: Option<String>,
}

impl From<&Entry> for EntryDto {
    fn from(e: &Entry) -> Self {
        EntryDto {
            id: e.id,
            ts: e.ts,
            rule: e.rule.clone(),
            kind: e.kind.as_str(),
            name: e
                .src
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            src: e.src.display().to_string(),
            src_display: tilde(&e.src),
            dst: e.dst.as_ref().map(|d| d.display().to_string()),
            dst_display: e.dst.as_deref().map(tilde),
            size: e.size,
            undone: e.undone,
            error: e.error.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct FolderDto {
    pub path: String,
    pub path_display: String,
    pub exists: bool,
    pub rules: usize,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Serialize)]
pub struct FactsDto {
    pub path: String,
    pub path_display: String,
    pub name: String,
    pub ext: String,
    pub mime: String,
    pub origin_url: Option<String>,
    pub origin_host: Option<String>,
    pub size: u64,
    pub modified: i64,
    pub keep: bool,
}

impl FactsDto {
    pub fn new(f: &Facts, keep: bool) -> FactsDto {
        FactsDto {
            path: f.path.display().to_string(),
            path_display: tilde(&f.path),
            name: f.name.clone(),
            ext: f.ext.clone(),
            mime: f.mime.clone(),
            origin_url: f.origin_url.clone(),
            origin_host: f.origin_host(),
            size: f.size,
            modified: unix(f.modified),
            keep,
        }
    }
}

#[derive(Serialize)]
pub struct RuleCheck {
    pub name: String,
    pub enabled: bool,
    pub checks: Vec<Check>,
    /// `wins`, `fails`, `untested` ou `paused`
    pub verdict: &'static str,
}

#[derive(Serialize)]
pub struct Outcome {
    pub rule: String,
    pub kind: &'static str,
    pub dst: Option<String>,
    pub dst_display: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviates_home() {
        let h = "/home/x";
        assert_eq!(
            tilde_with(Path::new("/home/x/Downloads/a.pdf"), h),
            "~/Downloads/a.pdf"
        );
        assert_eq!(tilde_with(Path::new("/home/x"), h), "~");
        assert_eq!(tilde_with(Path::new("/home/xy/a"), h), "/home/xy/a");
        assert_eq!(tilde_with(Path::new("/tmp/a"), ""), "/tmp/a");
    }
}
