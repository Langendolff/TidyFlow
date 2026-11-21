//! Host de native messaging: o navegador abre `tidyflow native-host` e manda, pelo
//! stdin, mensagens com 4 bytes de tamanho (little-endian) seguidos de JSON. Cada
//! mensagem recebe uma resposta no mesmo formato pelo stdout, que é só do protocolo.

use serde::Deserialize;
use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
use tidyflow_core::{
    downloads::{DownloadState, Started},
    facts::{DOWNLOAD_XATTR, ORIGIN_XATTR, REFERRER_XATTR},
    journal::Journal,
};

/// O navegador não manda nada perto disso; acima é erro de protocolo.
const MAX_MESSAGE: u32 = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Started {
        id: i64,
        browser: String,
        path: PathBuf,
        url: String,
        final_url: Option<String>,
        referrer: Option<String>,
        page_title: Option<String>,
        mime: Option<String>,
        started_at: i64,
    },
    Finished {
        id: i64,
        browser: String,
        path: PathBuf,
        size: u64,
    },
    Interrupted {
        id: i64,
        browser: String,
        error: Option<String>,
    },
    Renamed {
        id: i64,
        browser: String,
        path: PathBuf,
    },
}

/// Lê uma mensagem; `None` no fim limpo da entrada.
pub fn read_message(r: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        match r.read(&mut len[got..])? {
            0 if got == 0 => return Ok(None),
            0 => return Err(io::ErrorKind::UnexpectedEof.into()),
            n => got += n,
        }
    }
    let len = u32::from_le_bytes(len);
    if len > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("mensagem de {len} bytes passa do limite"),
        ));
    }
    let mut buf = vec![0; len as usize];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

pub fn write_message(w: &mut impl Write, v: &serde_json::Value) -> io::Result<()> {
    let body = serde_json::to_vec(v)?;
    w.write_all(&(body.len() as u32).to_le_bytes())?;
    w.write_all(&body)?;
    w.flush()
}

fn set_if_absent(path: &Path, name: &str, value: &str) -> io::Result<()> {
    if xattr::get(path, name)?.is_none() {
        xattr::set(path, name, value.as_bytes())?;
    }
    Ok(())
}

pub fn handle(journal: &Journal, msg: Message) -> tidyflow_core::Result<()> {
    match msg {
        Message::Started {
            id,
            browser,
            path,
            url,
            final_url,
            referrer,
            page_title,
            mime,
            started_at,
        } => {
            journal.download_started(&Started {
                browser: &browser,
                browser_id: id,
                path: &path,
                url: final_url.as_deref().unwrap_or(&url),
                referrer: referrer.as_deref().filter(|r| !r.is_empty()),
                page_title: page_title.as_deref().filter(|t| !t.is_empty()),
                mime: mime.as_deref().filter(|m| !m.is_empty()),
                started_at,
            })?;
        }
        Message::Renamed { id, browser, path } => {
            // Sem `started` antes: nada a renomear.
            journal.download_renamed(&browser, id, &path)?;
        }
        Message::Interrupted { id, browser, error } => {
            eprintln!(
                "download {browser}#{id} interrompido: {}",
                error.as_deref().unwrap_or("sem motivo")
            );
            journal.download_interrupted(&browser, id)?
        }
        Message::Finished {
            id,
            browser,
            path,
            size,
        } => {
            // O estado vem antes do xattr: quando o daemon recebe o evento do xattr,
            // o registro já diz que o download terminou.
            let Some(d) = journal.download_finished(&browser, id, size)? else {
                eprintln!("download {browser}#{id} terminou sem registro de início");
                return Ok(());
            };
            if !path.is_file() {
                journal.download_set_state(d.id, DownloadState::Gone)?;
                return Ok(());
            }
            let xattrs = || -> io::Result<()> {
                if let Some(url) = &d.url {
                    set_if_absent(&path, ORIGIN_XATTR, url)?;
                }
                if let Some(r) = &d.referrer {
                    set_if_absent(&path, REFERRER_XATTR, r)?;
                }
                // Sempre gravado: garante o evento de metadados para o daemon.
                xattr::set(&path, DOWNLOAD_XATTR, d.id.to_string().as_bytes())
            };
            if let Err(e) = xattrs() {
                eprintln!("não foi possível gravar xattrs em {}: {e}", path.display());
            }
        }
    }
    Ok(())
}

pub fn run(journal_path: &Path) -> tidyflow_core::Result<()> {
    let journal = Journal::open(journal_path)?;
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    while let Some(raw) = read_message(&mut stdin)? {
        let result = serde_json::from_slice::<Message>(&raw)
            .map_err(|e| tidyflow_core::Error::from(format!("mensagem inválida: {e}")))
            .and_then(|msg| handle(&journal, msg));
        let reply = match result {
            Ok(()) => serde_json::json!({ "ok": true }),
            Err(e) => {
                eprintln!("native-host: {e}");
                serde_json::json!({ "ok": false, "error": e.to_string() })
            }
        };
        write_message(&mut stdout, &reply)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn frame(body: &[u8]) -> Vec<u8> {
        let mut v = (body.len() as u32).to_le_bytes().to_vec();
        v.extend_from_slice(body);
        v
    }

    #[test]
    fn framing_roundtrip() {
        let mut out = Vec::new();
        write_message(&mut out, &serde_json::json!({ "ok": true })).unwrap();
        assert_eq!(&out[..4], &11u32.to_le_bytes());
        let mut r = io::Cursor::new(out);
        assert_eq!(read_message(&mut r).unwrap().unwrap(), br#"{"ok":true}"#);
        assert!(read_message(&mut r).unwrap().is_none());
    }

    #[test]
    fn framing_errors() {
        let mut big = io::Cursor::new((MAX_MESSAGE + 1).to_le_bytes().to_vec());
        assert!(read_message(&mut big).is_err());
        let mut half = io::Cursor::new(vec![3, 0]);
        assert!(read_message(&mut half).is_err());
        let mut short = io::Cursor::new(frame(b"abc")[..5].to_vec());
        assert!(read_message(&mut short).is_err());
    }

    fn started(path: &Path) -> Message {
        serde_json::from_value(serde_json::json!({
            "type": "started", "id": 1, "browser": "firefox", "path": path,
            "url": "https://example.com/redirect", "final_url": "https://cdn.example.com/a.pdf",
            "referrer": "https://example.com/page", "page_title": "", "mime": "application/pdf",
            "started_at": 1_800_000_000
        }))
        .unwrap()
    }

    fn finished(path: &Path) -> Message {
        serde_json::from_value(serde_json::json!({
            "type": "finished", "id": 1, "browser": "firefox", "path": path, "size": 3
        }))
        .unwrap()
    }

    fn xattr_ok(p: &Path) -> bool {
        xattr::set(p, "user.tidyflow.test", b"1").is_ok()
    }

    #[test]
    fn finished_marks_complete_and_writes_xattrs() {
        let d = tempfile::tempdir().unwrap();
        let j = Journal::open(&d.path().join("j.db")).unwrap();
        let p = d.path().join("a.pdf");
        fs::write(&p, "abc").unwrap();
        if !xattr_ok(&p) {
            return;
        }
        handle(&j, started(&p)).unwrap();
        handle(&j, finished(&p)).unwrap();
        let row = j.download_at(&p).unwrap().unwrap();
        assert_eq!(row.state, DownloadState::Complete);
        assert_eq!(row.url.as_deref(), Some("https://cdn.example.com/a.pdf"));
        assert_eq!(row.page_title, None); // vazio não é título
        let get = |n| String::from_utf8(xattr::get(&p, n).unwrap().unwrap()).unwrap();
        assert_eq!(get(DOWNLOAD_XATTR), row.id.to_string());
        assert_eq!(get(ORIGIN_XATTR), "https://cdn.example.com/a.pdf");
        assert_eq!(get(REFERRER_XATTR), "https://example.com/page");
    }

    #[test]
    fn finished_keeps_existing_origin() {
        let d = tempfile::tempdir().unwrap();
        let j = Journal::open(&d.path().join("j.db")).unwrap();
        let p = d.path().join("a.pdf");
        fs::write(&p, "abc").unwrap();
        if !xattr_ok(&p) {
            return;
        }
        xattr::set(&p, ORIGIN_XATTR, b"https://chromium.wrote/this").unwrap();
        handle(&j, started(&p)).unwrap();
        handle(&j, finished(&p)).unwrap();
        assert_eq!(
            xattr::get(&p, ORIGIN_XATTR).unwrap().unwrap(),
            b"https://chromium.wrote/this"
        );
    }

    #[test]
    fn finished_without_file_is_gone() {
        let d = tempfile::tempdir().unwrap();
        let j = Journal::open(&d.path().join("j.db")).unwrap();
        let p = d.path().join("sumiu.pdf");
        handle(&j, started(&p)).unwrap();
        handle(&j, finished(&p)).unwrap();
        assert_eq!(
            j.download_at(&p).unwrap().unwrap().state,
            DownloadState::Gone
        );
    }

    #[test]
    fn unknown_messages_are_harmless() {
        let d = tempfile::tempdir().unwrap();
        let j = Journal::open(&d.path().join("j.db")).unwrap();
        let p = d.path().join("x.pdf");
        handle(&j, finished(&p)).unwrap();
        let renamed: Message = serde_json::from_value(serde_json::json!({
            "type": "renamed", "id": 9, "browser": "chromium", "path": p
        }))
        .unwrap();
        handle(&j, renamed).unwrap();
        assert!(j.download_at(&p).unwrap().is_none());
    }
}
