use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// Gravado por Chromium/Chrome, `wget --xattr` e `curl --xattr`.
pub const ORIGIN_XATTR: &str = "user.xdg.origin.url";

/// Gravado pelo undo: o engine não reorganiza arquivos com esta marca.
pub const KEEP_XATTR: &str = "user.tidyflow.keep";

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
    pub keep: bool,
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
        // Falha de xattr (fs sem suporte) = sem origem.
        let origin_url = xattr::get(path, ORIGIN_XATTR)
            .ok()
            .flatten()
            .and_then(|v| String::from_utf8(v).ok());
        Ok(Facts {
            path: path.to_path_buf(),
            name: lossy(path.file_name()).unwrap_or_default(),
            stem: lossy(path.file_stem()).unwrap_or_default(),
            ext: lossy(path.extension()).unwrap_or_default().to_lowercase(),
            mime,
            size: meta.len(),
            modified: meta.modified()?,
            origin_url,
            keep: xattr::get(path, KEEP_XATTR).is_ok_and(|v| v.is_some()),
        })
    }

    pub fn origin_host(&self) -> Option<String> {
        let rest = self.origin_url.as_deref()?.split_once("://")?.1;
        let authority = rest.split(['/', '?', '#']).next()?;
        let host = authority.rsplit('@').next()?.split(':').next()?;
        (!matches!(host, "" | "." | "..")).then(|| host.to_ascii_lowercase())
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
            keep: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn sample(name: &str) -> Facts {
        let path = std::env::temp_dir().join(name);
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
            keep: false,
        }
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
        xattr::set(
            &p,
            ORIGIN_XATTR,
            b"https://user:pw@App.Nubank.com.br:443/x?y=1",
        )
        .unwrap();
        let f = Facts::read(&p).unwrap();
        assert_eq!(f.origin_host().as_deref(), Some("app.nubank.com.br"));
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
