//! Metadados que o TidyFlow lê e grava no próprio arquivo: de onde veio, a página de
//! origem, o id do download e a marca de desfeito.
//!
//! - Linux e macOS: atributos estendidos (`user.xdg.origin.url`…).
//! - Windows: fluxos alternativos do NTFS (`arquivo:tidyflow.origin`…).
//!
//! Sem marca nossa, a origem e a página vêm do que o sistema grava sozinho:
//! `user.xdg.*` já é o padrão do Chromium no Linux; no macOS, `kMDItemWhereFroms`;
//! no Windows, o `Zone.Identifier` ("Mark of the Web").

use std::{io, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    /// URL do arquivo
    Origin,
    /// Página de onde o download saiu
    Referrer,
    /// Id da linha em `downloads`, gravado pelo host da extensão
    Download,
    /// Gravado pelo undo: o engine não reorganiza arquivos com esta marca
    Keep,
}

impl Tag {
    /// Nome do atributo estendido (Linux/macOS).
    pub fn xattr(self) -> &'static str {
        match self {
            Tag::Origin => "user.xdg.origin.url",
            Tag::Referrer => "user.xdg.referrer.url",
            Tag::Download => "user.tidyflow.download",
            Tag::Keep => "user.tidyflow.keep",
        }
    }

    /// Nome do fluxo alternativo (Windows).
    pub fn stream(self) -> &'static str {
        match self {
            Tag::Origin => "tidyflow.origin",
            Tag::Referrer => "tidyflow.referrer",
            Tag::Download => "tidyflow.download",
            Tag::Keep => "tidyflow.keep",
        }
    }
}

#[cfg(windows)]
const ALL: [Tag; 4] = [Tag::Origin, Tag::Referrer, Tag::Download, Tag::Keep];

/// Valor da marca; para origem e página, cai no que o sistema gravou.
/// Falha de leitura (fs sem suporte) = sem valor.
pub fn get(path: &Path, tag: Tag) -> Option<String> {
    own(path, tag).or_else(|| match tag {
        Tag::Origin => system_origin(path).0,
        Tag::Referrer => system_origin(path).1,
        _ => None,
    })
}

/// Só a marca gravada por nós (ou, no Linux, pelo Chromium com o mesmo nome).
pub fn own(path: &Path, tag: Tag) -> Option<String> {
    raw_get(path, tag)
        .ok()
        .flatten()
        .and_then(|v| String::from_utf8(v).ok())
}

pub fn has(path: &Path, tag: Tag) -> bool {
    raw_get(path, tag).is_ok_and(|v| v.is_some())
}

pub fn set(path: &Path, tag: Tag, value: &str) -> io::Result<()> {
    raw_set(path, tag, value.as_bytes())
}

/// Não sobrescreve o que o navegador já gravou.
pub fn set_if_absent(path: &Path, tag: Tag, value: &str) -> io::Result<()> {
    if raw_get(path, tag)?.is_none() {
        set(path, tag, value)?;
    }
    Ok(())
}

/// Ausente já conta como removido.
pub fn remove(path: &Path, tag: Tag) -> io::Result<()> {
    raw_remove(path, tag)
}

// ---------------------------------------------------------------- Unix

#[cfg(unix)]
fn raw_get(path: &Path, tag: Tag) -> io::Result<Option<Vec<u8>>> {
    xattr::get(path, tag.xattr())
}

#[cfg(unix)]
fn raw_set(path: &Path, tag: Tag, value: &[u8]) -> io::Result<()> {
    xattr::set(path, tag.xattr(), value)
}

#[cfg(unix)]
fn raw_remove(path: &Path, tag: Tag) -> io::Result<()> {
    match xattr::remove(path, tag.xattr()) {
        Err(e) if !has(path, tag) && e.kind() != io::ErrorKind::Unsupported => Ok(()),
        r => r,
    }
}

/// Copia todos os atributos estendidos; falhas por atributo são ignoradas porque nem
/// todo sistema de arquivos aceita xattr.
#[cfg(unix)]
pub fn copy_all(src: &Path, dst: &Path) {
    let Ok(names) = xattr::list(src) else { return };
    for n in names {
        if let Ok(Some(v)) = xattr::get(src, &n) {
            let _ = xattr::set(dst, &n, &v);
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn system_origin(_: &Path) -> (Option<String>, Option<String>) {
    (None, None)
}

/// `kMDItemWhereFroms`: plist binário com `[url, página]`, gravado pelo Safari,
/// Chrome e Firefox.
#[cfg(target_os = "macos")]
fn system_origin(path: &Path) -> (Option<String>, Option<String>) {
    let froms = xattr::get(path, "com.apple.metadata:kMDItemWhereFroms")
        .ok()
        .flatten()
        .and_then(|b| plist::from_bytes::<Vec<String>>(&b).ok())
        .unwrap_or_default();
    where_froms(froms)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn where_froms(froms: Vec<String>) -> (Option<String>, Option<String>) {
    let mut it = froms.into_iter().filter(|s| s.contains("://"));
    (it.next(), it.next())
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
fn stream_path(path: &Path, stream: &str) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(":");
    s.push(stream);
    s.into()
}

#[cfg(windows)]
fn read_stream(path: &Path, stream: &str) -> io::Result<Option<Vec<u8>>> {
    match std::fs::read(stream_path(path, stream)) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(windows)]
fn raw_get(path: &Path, tag: Tag) -> io::Result<Option<Vec<u8>>> {
    read_stream(path, tag.stream())
}

#[cfg(windows)]
fn raw_set(path: &Path, tag: Tag, value: &[u8]) -> io::Result<()> {
    std::fs::write(stream_path(path, tag.stream()), value)
}

#[cfg(windows)]
fn raw_remove(path: &Path, tag: Tag) -> io::Result<()> {
    match std::fs::remove_file(stream_path(path, tag.stream())) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

/// Copia as nossas marcas e o `Zone.Identifier`; falhas são ignoradas (FAT, rede).
#[cfg(windows)]
pub fn copy_all(src: &Path, dst: &Path) {
    let streams = ALL.iter().map(|t| t.stream()).chain(["Zone.Identifier"]);
    for s in streams {
        if let Ok(Some(v)) = read_stream(src, s) {
            let _ = std::fs::write(stream_path(dst, s), v);
        }
    }
}

#[cfg(windows)]
fn system_origin(path: &Path) -> (Option<String>, Option<String>) {
    match read_stream(path, "Zone.Identifier") {
        Ok(Some(b)) => zone_identifier(&String::from_utf8_lossy(&b)),
        _ => (None, None),
    }
}

/// `HostUrl=` e `ReferrerUrl=` da seção `[ZoneTransfer]`.
#[cfg_attr(not(windows), allow(dead_code))]
fn zone_identifier(s: &str) -> (Option<String>, Option<String>) {
    let value = |key: &str| {
        s.lines()
            .filter_map(|l| l.trim().split_once('='))
            .find(|(k, _)| k.trim().eq_ignore_ascii_case(key))
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| v.contains("://"))
    };
    (value("HostUrl"), value("ReferrerUrl"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O fs de testes pode não aceitar xattr (tmpfs antigo); aí o teste não se aplica.
    fn supported(p: &Path) -> bool {
        set(p, Tag::Keep, "1").is_ok() && remove(p, Tag::Keep).is_ok()
    }

    #[test]
    fn round_trip() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.pdf");
        std::fs::write(&p, "x").unwrap();
        if !supported(&p) {
            return;
        }
        assert_eq!(get(&p, Tag::Origin), None);
        assert!(!has(&p, Tag::Keep));
        set(&p, Tag::Origin, "https://a.example/x.pdf").unwrap();
        set_if_absent(&p, Tag::Origin, "https://b.example/").unwrap();
        assert_eq!(get(&p, Tag::Origin).unwrap(), "https://a.example/x.pdf");
        set(&p, Tag::Keep, "1").unwrap();
        assert!(has(&p, Tag::Keep));
        remove(&p, Tag::Keep).unwrap();
        assert!(!has(&p, Tag::Keep));
        remove(&p, Tag::Keep).unwrap();

        let q = d.path().join("b.pdf");
        std::fs::write(&q, "x").unwrap();
        copy_all(&p, &q);
        assert_eq!(get(&q, Tag::Origin).unwrap(), "https://a.example/x.pdf");
    }

    #[test]
    fn parses_zone_identifier() {
        let z = "[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://github.com/a/b/releases\r\nHostUrl=https://objects.githubusercontent.com/x.zip\r\n";
        assert_eq!(
            zone_identifier(z),
            (
                Some("https://objects.githubusercontent.com/x.zip".into()),
                Some("https://github.com/a/b/releases".into())
            )
        );
        // Firefox grava `about:internet` quando não sabe a origem.
        assert_eq!(
            zone_identifier("[ZoneTransfer]\nZoneId=3\nHostUrl=about:internet\n"),
            (None, None)
        );
    }

    #[test]
    fn parses_where_froms() {
        assert_eq!(
            where_froms(vec![
                "https://cdn.example/a.dmg".into(),
                "https://example.com/download".into()
            ]),
            (
                Some("https://cdn.example/a.dmg".into()),
                Some("https://example.com/download".into())
            )
        );
        assert_eq!(where_froms(vec![]), (None, None));
    }
}
