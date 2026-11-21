use crate::meta::{self, Tag};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

#[derive(Debug, Clone)]
pub struct Facts {
    pub path: PathBuf,
    pub name: String,
    pub stem: String,
    pub ext: String,
    pub mime: String,
    pub size: u64,
    pub modified: SystemTime,
    pub origin_url: Option<String>,
    pub referrer_url: Option<String>,
    /// id do registro em `downloads` (xattr do host da extensão)
    pub download_id: Option<i64>,
    pub keep: bool,
}

/// Host de uma URL `esquema://[usuário@]host[:porta]/...`, em minúsculas.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?.split(':').next()?;
    (!matches!(host, "" | "." | "..")).then(|| host.to_ascii_lowercase())
}

impl Facts {
    pub fn read(path: &Path) -> io::Result<Facts> {
        let meta = fs::metadata(path)?;
        let lossy = |s: Option<&std::ffi::OsStr>| s.map(|s| s.to_string_lossy().into_owned());
        let mime = match infer::get_from_path(path)? {
            Some(t) => t.mime_type().to_string(),
            None => mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string(),
        };
        Ok(Facts {
            path: path.to_path_buf(),
            name: lossy(path.file_name()).unwrap_or_default(),
            stem: lossy(path.file_stem()).unwrap_or_default(),
            ext: lossy(path.extension()).unwrap_or_default().to_lowercase(),
            mime,
            size: meta.len(),
            modified: meta.modified()?,
            origin_url: meta::get(path, Tag::Origin),
            referrer_url: meta::get(path, Tag::Referrer),
            download_id: meta::own(path, Tag::Download).and_then(|v| v.trim().parse().ok()),
            keep: meta::has(path, Tag::Keep),
        })
    }

    pub fn origin_host(&self) -> Option<String> {
        host_of(self.origin_url.as_deref()?)
    }

    pub fn referrer_host(&self) -> Option<String> {
        host_of(self.referrer_url.as_deref()?)
    }

    /// Site que o usuário reconhece: o da página de origem; sem ela, o host do arquivo.
    /// O arquivo muitas vezes vem de uma CDN (`objects.githubusercontent.com`).
    pub fn site(&self) -> Option<String> {
        self.referrer_host().or_else(|| self.origin_host())
    }

    pub fn age(&self, now: SystemTime) -> Duration {
        now.duration_since(self.modified).unwrap_or_default()
    }

    /// Fatos deduzidos só do caminho, para um arquivo que não existe mais (prévia do
    /// editor de regras): MIME pela extensão, tamanho 0, sem origem.
    pub fn assumed(path: &Path, modified: SystemTime) -> Facts {
        let lossy = |s: Option<&std::ffi::OsStr>| s.map(|s| s.to_string_lossy().into_owned());
        Facts {
            path: path.to_path_buf(),
            name: lossy(path.file_name()).unwrap_or_default(),
            stem: lossy(path.file_stem()).unwrap_or_default(),
            ext: lossy(path.extension()).unwrap_or_default().to_lowercase(),
            mime: mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string(),
            size: 0,
            modified,
            origin_url: None,
            referrer_url: None,
            download_id: None,
            keep: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn sample(name: &str) -> Facts {
        // Caminho real: no macOS a regra guarda /private/var, não o link /var.
        let path = crate::real_path(&std::env::temp_dir().join(name));
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        Facts {
            stem: path.file_stem().unwrap().to_string_lossy().into_owned(),
            name: name.to_string(),
            path,
            ext,
            mime: "application/octet-stream".into(),
            size: 0,
            modified: std::time::UNIX_EPOCH + Duration::from_secs(1_781_524_800), // 2026-06-15 12:00 UTC
            origin_url: None,
            referrer_url: None,
            download_id: None,
            keep: false,
        }
    }
}

/// Tipo do arquivo para os filtros da biblioteca.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Document,
    Image,
    Video,
    Audio,
    Archive,
    Installer,
    Other,
}

const INSTALLERS: [&str; 9] = [
    "deb",
    "rpm",
    "appimage",
    "iso",
    "exe",
    "msi",
    "dmg",
    "flatpak",
    "pkg.tar.zst",
];
const ARCHIVES: [&str; 11] = [
    "zip", "tar", "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "tar.gz", "tar.xz",
];
const DOCUMENTS: [&str; 14] = [
    "pdf", "doc", "docx", "odt", "rtf", "txt", "md", "epub", "xls", "xlsx", "ods", "csv", "ppt",
    "pptx",
];

/// Pela extensão primeiro (instaladores e compactados têm MIMEs genéricos), depois
/// pelo MIME. `name` serve para extensões duplas como `pkg.tar.zst`.
pub fn category(mime: &str, name: &str) -> Category {
    let lower = name.to_lowercase();
    let has = |exts: &[&str]| exts.iter().any(|e| lower.ends_with(&format!(".{e}")));
    if has(&INSTALLERS) {
        return Category::Installer;
    }
    if has(&ARCHIVES) {
        return Category::Archive;
    }
    if has(&DOCUMENTS) {
        return Category::Document;
    }
    match mime.split('/').next().unwrap_or("") {
        "image" => Category::Image,
        "video" => Category::Video,
        "audio" => Category::Audio,
        "text" => Category::Document,
        _ if mime == "application/pdf"
            || mime.starts_with("application/vnd.oasis.opendocument")
            || mime.starts_with("application/vnd.openxmlformats-officedocument")
            || mime == "application/msword" =>
        {
            Category::Document
        }
        _ => Category::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::UNIX_EPOCH};

    #[test]
    fn reads_basic_facts() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("Relatório.Final.PDF");
        fs::write(&p, b"%PDF-1.4 resto").unwrap();
        let f = Facts::read(&p).unwrap();
        assert_eq!(f.name, "Relatório.Final.PDF");
        assert_eq!(f.stem, "Relatório.Final");
        assert_eq!(f.ext, "pdf");
        assert_eq!(f.mime, "application/pdf");
        assert_eq!(f.size, 14);
        assert_eq!(f.origin_url, None);
    }

    #[test]
    fn mime_from_content_beats_extension() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("foto.txt");
        fs::write(
            &p,
            [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0],
        )
        .unwrap();
        assert_eq!(Facts::read(&p).unwrap().mime, "image/png");
    }

    #[test]
    fn mime_falls_back_to_extension() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("dados.csv");
        fs::write(&p, "a,b\n1,2\n").unwrap();
        assert_eq!(Facts::read(&p).unwrap().mime, "text/csv");
        let p = dir.path().join("sem_extensao");
        fs::write(&p, "x").unwrap();
        let f = Facts::read(&p).unwrap();
        assert_eq!(f.ext, "");
        assert_eq!(f.mime, "application/octet-stream");
    }

    #[test]
    fn reads_origin_xattr() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("fatura.pdf");
        fs::write(&p, "x").unwrap();
        if meta::set(
            &p,
            Tag::Origin,
            "https://user:pw@App.Nubank.com.br:443/x?y=1",
        )
        .is_err()
        {
            return; // fs sem xattr user.*
        }
        let f = Facts::read(&p).unwrap();
        assert_eq!(f.origin_host().as_deref(), Some("app.nubank.com.br"));
    }

    #[test]
    fn reads_referrer_and_download_xattrs() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.zip");
        fs::write(&p, "x").unwrap();
        if meta::set(&p, Tag::Referrer, "https://github.com/a/b/releases").is_err() {
            return; // fs sem xattr user.*
        }
        meta::set(&p, Tag::Download, "42").unwrap();
        let f = Facts::read(&p).unwrap();
        assert_eq!(f.referrer_host().as_deref(), Some("github.com"));
        assert_eq!(f.download_id, Some(42));
    }

    #[test]
    fn site_prefers_referrer() {
        let mut f = Facts::sample("a.zip");
        assert_eq!(f.site(), None);
        f.origin_url = Some("https://objects.githubusercontent.com/x".into());
        assert_eq!(f.site().as_deref(), Some("objects.githubusercontent.com"));
        f.referrer_url = Some("https://github.com/a/b/releases".into());
        assert_eq!(f.site().as_deref(), Some("github.com"));
    }

    #[test]
    fn categories() {
        use Category::*;
        for (mime, name, want) in [
            ("application/pdf", "a.pdf", Document),
            ("application/octet-stream", "a.docx", Document),
            (
                "application/vnd.oasis.opendocument.text",
                "sem-ext",
                Document,
            ),
            ("text/plain", "notas", Document),
            ("image/png", "a.png", Image),
            ("video/mp4", "a.mp4", Video),
            ("audio/flac", "a.flac", Audio),
            ("application/vnd.debian.binary-package", "a.deb", Installer),
            ("application/octet-stream", "App.AppImage", Installer),
            ("application/x-iso9660-image", "a.iso", Installer),
            ("application/zstd", "pacote.pkg.tar.zst", Installer),
            ("application/gzip", "src.tar.gz", Archive),
            ("application/zip", "a.ZIP", Archive),
            ("application/x-7z-compressed", "a.7z", Archive),
            ("application/octet-stream", "blob", Other),
        ] {
            assert_eq!(category(mime, name), want, "{name}");
        }
    }

    #[test]
    fn host_parsing() {
        let mut f = Facts::sample("a");
        for (url, host) in [
            ("https://github.com/a/b", Some("github.com")),
            ("http://x.org", Some("x.org")),
            ("https://h.com#frag", Some("h.com")),
            ("not a url", None),
            ("file:///home/x", None),
            ("http://./x", None),
            ("http://../x", None),
        ] {
            f.origin_url = Some(url.into());
            assert_eq!(f.origin_host().as_deref(), host, "{url}");
        }
    }

    #[test]
    fn age_never_negative() {
        let f = Facts::sample("a");
        assert_eq!(f.age(UNIX_EPOCH), Duration::ZERO);
        assert_eq!(
            f.age(f.modified + Duration::from_secs(10)),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn missing_file_is_not_found() {
        let err = Facts::read(Path::new("/nao/existe/x.pdf")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
