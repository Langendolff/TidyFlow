//! O que o app lê do sistema: estado do serviço systemd e conteúdo das pastas.

use serde::Serialize;
use std::{fs, path::Path, process::Command};
use tidyflow_core::stability;

#[derive(Debug, Serialize, PartialEq)]
pub struct DaemonStatus {
    pub active: bool,
    /// unix, quando o serviço entrou em `active`
    pub since: Option<i64>,
    pub folders: usize,
}

const UNIT: &str = "tidyflow";

pub fn daemon_status(folders: usize) -> DaemonStatus {
    let out = Command::new("systemctl")
        .args(["--user", "show", UNIT, "--timestamp=unix"])
        .args(["-p", "ActiveState", "-p", "ActiveEnterTimestamp"])
        .output();
    match out {
        Ok(o) if o.status.success() => parse_show(&String::from_utf8_lossy(&o.stdout), folders),
        _ => DaemonStatus {
            active: false,
            since: None,
            folders,
        },
    }
}

pub fn start_daemon() -> Result<(), String> {
    let o = Command::new("systemctl")
        .args(["--user", "start", UNIT])
        .output()
        .map_err(|e| format!("não foi possível rodar o systemctl: {e}"))?;
    if o.status.success() {
        Ok(())
    } else {
        Err(format!(
            "systemctl --user start {UNIT} falhou: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ))
    }
}

fn parse_show(out: &str, folders: usize) -> DaemonStatus {
    let mut active = false;
    let mut since = None;
    for line in out.lines() {
        match line.split_once('=') {
            Some(("ActiveState", v)) => active = v.trim() == "active",
            Some(("ActiveEnterTimestamp", v)) => {
                since = v.trim().strip_prefix('@').and_then(|n| n.parse().ok())
            }
            _ => {}
        }
    }
    DaemonStatus {
        active,
        since: since.filter(|_| active),
        folders,
    }
}

/// Arquivos regulares do nível de cima (como a varredura), sem nomes temporários.
pub fn folder_contents(dir: &Path) -> (usize, u64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return (0, 0);
    };
    entries
        .flatten()
        .filter(|e| !stability::is_ignored_name(&e.file_name().to_string_lossy()))
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .fold((0, 0), |(n, b), m| (n + 1, b + m.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_systemctl_show() {
        let s = parse_show("ActiveState=active\nActiveEnterTimestamp=@1763251200\n", 2);
        assert_eq!(
            s,
            DaemonStatus {
                active: true,
                since: Some(1_763_251_200),
                folders: 2
            }
        );
        let s = parse_show("ActiveState=inactive\nActiveEnterTimestamp=\n", 1);
        assert!(!s.active && s.since.is_none());
    }

    #[test]
    fn counts_top_level_files() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("a"), "123").unwrap();
        fs::write(d.path().join("b.part"), "1").unwrap();
        fs::create_dir(d.path().join("sub")).unwrap();
        fs::write(d.path().join("sub/c"), "1").unwrap();
        assert_eq!(folder_contents(d.path()), (1, 3));
        assert_eq!(folder_contents(&d.path().join("nao")), (0, 0));
    }
}
