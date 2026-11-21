//! O que o app lê do sistema: estado do daemon e conteúdo das pastas.

use serde::Serialize;
use std::{fs, path::Path};
use tidyflow_core::{service, stability};

#[derive(Debug, Serialize, PartialEq)]
pub struct DaemonStatus {
    pub active: bool,
    /// unix, quando o serviço entrou em `active`
    pub since: Option<i64>,
    pub folders: usize,
}

pub fn daemon_status(folders: usize) -> DaemonStatus {
    let s = service::status();
    DaemonStatus {
        active: s.active,
        since: s.since,
        folders,
    }
}

/// Inicia; sem serviço instalado (instalação sem `tidyflow service enable`), instala.
/// Espera o daemon pegar a trava para o status já sair certo.
pub fn start_daemon() -> Result<(), String> {
    if let Err(e) = service::start() {
        service::enable(&service::daemon_exe()).map_err(|e2| format!("{e}; {e2}"))?;
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !service::status().active && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(())
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
