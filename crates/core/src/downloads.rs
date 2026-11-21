//! Registro de downloads: o que a extensão de navegador informa e, a partir da
//! biblioteca, os arquivos que chegam nas pastas monitoradas por outros meios.

use crate::{
    Result,
    actions::Kind,
    facts::{Category, Facts, category, host_of},
    journal::{self, Entry, Journal, bytes, path},
};
use rusqlite::{Connection, OptionalExtension, Row, params, types::Type};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const SCHEMA_V3: &str = "
CREATE TABLE downloads (
    id           INTEGER PRIMARY KEY,
    source       TEXT    NOT NULL,
    browser      TEXT,
    browser_id   INTEGER,
    path         BLOB    NOT NULL,
    current_path BLOB    NOT NULL,
    url          TEXT,
    referrer     TEXT,
    page_title   TEXT,
    mime         TEXT,
    size         INTEGER,
    state        TEXT    NOT NULL,
    started_at   INTEGER NOT NULL,
    finished_at  INTEGER
);
CREATE INDEX downloads_current ON downloads(current_path);
CREATE INDEX downloads_browser ON downloads(browser, browser_id);
CREATE INDEX downloads_started ON downloads(started_at);
";

const COLS: &str = "id, source, browser, browser_id, path, current_path, url, referrer, \
                    page_title, mime, size, state, started_at, finished_at";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    InProgress,
    Complete,
    Interrupted,
    Trashed,
    Gone,
}

impl DownloadState {
    pub fn as_str(self) -> &'static str {
        match self {
            DownloadState::InProgress => "in_progress",
            DownloadState::Complete => "complete",
            DownloadState::Interrupted => "interrupted",
            DownloadState::Trashed => "trashed",
            DownloadState::Gone => "gone",
        }
    }

    pub fn parse(s: &str) -> Option<DownloadState> {
        Some(match s {
            "in_progress" => DownloadState::InProgress,
            "complete" => DownloadState::Complete,
            "interrupted" => DownloadState::Interrupted,
            "trashed" => DownloadState::Trashed,
            "gone" => DownloadState::Gone,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// informado pela extensão de navegador
    Browser,
    /// visto pelo daemon numa pasta monitorada, sem registro do navegador
    Folder,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Source::Browser => "browser",
            Source::Folder => "folder",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Download {
    pub id: i64,
    pub source: Source,
    pub browser: Option<String>,
    pub browser_id: Option<i64>,
    /// onde o arquivo chegou
    pub path: PathBuf,
    /// onde ele está agora (ou onde estava antes de ir para a lixeira)
    pub current_path: PathBuf,
    pub url: Option<String>,
    pub referrer: Option<String>,
    pub page_title: Option<String>,
    pub mime: Option<String>,
    pub size: Option<u64>,
    pub state: DownloadState,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

/// Início de um download, como a extensão informa.
pub struct Started<'a> {
    pub browser: &'a str,
    pub browser_id: i64,
    pub path: &'a Path,
    pub url: &'a str,
    pub referrer: Option<&'a str>,
    pub page_title: Option<&'a str>,
    pub mime: Option<&'a str>,
    pub started_at: i64,
}

/// Downloads em andamento há mais que isso são tratados como abandonados
/// (navegador fechado no meio).
pub const STALE_SECS: u64 = 24 * 3600;

/// Onde o arquivo está, para os filtros da biblioteca.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    All,
    /// ainda numa pasta monitorada
    InWatch,
    /// fora das pastas monitoradas
    Organized,
    Trashed,
    /// não está mais onde o TidyFlow o viu
    Missing,
    Downloading,
}

pub struct LibraryQuery<'a> {
    pub place: Place,
    pub category: Option<Category>,
    pub site: Option<&'a str>,
    /// só os que chegaram a partir deste timestamp unix
    pub since: Option<i64>,
    pub search: &'a str,
    pub before_id: Option<i64>,
    pub limit: usize,
    pub watch: &'a [PathBuf],
}

#[derive(Debug, Clone)]
pub struct LibraryItem {
    pub download: Download,
    pub category: Category,
    pub site: Option<String>,
    /// ação mais recente do TidyFlow sobre o arquivo no caminho atual
    pub last_action: Option<Entry>,
}

impl Download {
    /// Mesma regra de `Facts::site`: a página de origem; sem ela, o host do arquivo.
    pub fn site(&self) -> Option<String> {
        self.referrer
            .as_deref()
            .and_then(host_of)
            .or_else(|| self.url.as_deref().and_then(host_of))
    }

    pub fn category(&self) -> Category {
        let name = self
            .current_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        category(self.mime.as_deref().unwrap_or(""), &name)
    }

    fn in_watch(&self, watch: &[PathBuf]) -> bool {
        self.current_path
            .parent()
            .is_some_and(|p| watch.iter().any(|w| w == p))
    }
}

/// Uma linha por caminho (a mais recente) e sem downloads interrompidos: uma
/// interrupção não deixa arquivo.
const LIBRARY_ROWS: &str = "state != 'interrupted' \
     AND id IN (SELECT MAX(id) FROM downloads GROUP BY current_path)";

fn secs(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

/// Acompanha uma ação do engine no registro mais recente do arquivo: move e rename
/// mudam o caminho atual, lixeira muda o estado. `undo` faz o caminho inverso.
pub(crate) fn follow(
    conn: &Connection,
    kind: Kind,
    src: &Path,
    dst: Option<&Path>,
    undo: bool,
) -> rusqlite::Result<()> {
    let latest = "(SELECT id FROM downloads WHERE current_path = ?1 ORDER BY id DESC LIMIT 1)";
    match (kind, dst) {
        (Kind::Move | Kind::Rename, Some(dst)) => {
            let (from, to) = if undo { (dst, src) } else { (src, dst) };
            conn.execute(
                &format!(
                    "UPDATE downloads SET current_path = ?2 WHERE id = {latest} AND state = 'complete'"
                ),
                params![bytes(from), bytes(to)],
            )?;
        }
        (Kind::Trash, _) => {
            let (from, to) = if undo {
                ("trashed", "complete")
            } else {
                ("complete", "trashed")
            };
            conn.execute(
                &format!("UPDATE downloads SET state = ?2 WHERE id = {latest} AND state = ?3"),
                params![bytes(src), to, from],
            )?;
        }
        // Copiar não muda onde o original está.
        _ => {}
    }
    Ok(())
}

fn download(r: &Row) -> rusqlite::Result<Download> {
    let source: String = r.get(1)?;
    let state: String = r.get(11)?;
    Ok(Download {
        id: r.get(0)?,
        source: match source.as_str() {
            "browser" => Source::Browser,
            "folder" => Source::Folder,
            _ => return Err(rusqlite::Error::InvalidColumnType(1, source, Type::Text)),
        },
        browser: r.get(2)?,
        browser_id: r.get(3)?,
        path: path(r.get(4)?),
        current_path: path(r.get(5)?),
        url: r.get(6)?,
        referrer: r.get(7)?,
        page_title: r.get(8)?,
        mime: r.get(9)?,
        size: r.get::<_, Option<i64>>(10)?.map(|n| n.max(0) as u64),
        state: DownloadState::parse(&state).ok_or(rusqlite::Error::InvalidColumnType(
            11,
            state,
            Type::Text,
        ))?,
        started_at: r.get(12)?,
        finished_at: r.get(13)?,
    })
}

impl Journal {
    /// Registra o início. Um `started` repetido (o navegador dispara criado e mudou
    /// quase juntos) devolve a mesma linha; um download antigo ainda em andamento com
    /// o mesmo id (de outra sessão do navegador) é dado como interrompido.
    pub fn download_started(&self, s: &Started) -> Result<i64> {
        let existing: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT id, started_at FROM downloads WHERE browser = ?1 AND browser_id = ?2 \
                 AND state = 'in_progress' ORDER BY id DESC LIMIT 1",
                params![s.browser, s.browser_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match existing {
            Some((id, at)) if at == s.started_at => {
                self.conn.execute(
                    "UPDATE downloads SET path = ?2, current_path = ?2, \
                     referrer = COALESCE(referrer, ?3), page_title = COALESCE(page_title, ?4), \
                     mime = COALESCE(mime, ?5) WHERE id = ?1",
                    params![id, bytes(s.path), s.referrer, s.page_title, s.mime],
                )?;
                return Ok(id);
            }
            Some((id, _)) => self.download_set_state(id, DownloadState::Interrupted)?,
            None => {}
        }
        self.conn.execute(
            "INSERT INTO downloads (source, browser, browser_id, path, current_path, url, \
             referrer, page_title, mime, state, started_at) \
             VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                Source::Browser.as_str(),
                s.browser,
                s.browser_id,
                bytes(s.path),
                s.url,
                s.referrer,
                s.page_title,
                s.mime,
                DownloadState::InProgress.as_str(),
                s.started_at
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Linha `in_progress` mais recente daquele navegador e id (os ids do navegador
    /// podem se repetir entre sessões).
    fn in_progress_id(&self, browser: &str, browser_id: i64) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM downloads WHERE browser = ?1 AND browser_id = ?2 \
                 AND state = 'in_progress' ORDER BY id DESC LIMIT 1",
                params![browser, browser_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn download_renamed(
        &self,
        browser: &str,
        browser_id: i64,
        new_path: &Path,
    ) -> Result<Option<i64>> {
        let Some(id) = self.in_progress_id(browser, browser_id)? else {
            return Ok(None);
        };
        self.conn.execute(
            "UPDATE downloads SET path = ?2, current_path = ?2 WHERE id = ?1",
            params![id, bytes(new_path)],
        )?;
        Ok(Some(id))
    }

    pub fn download_finished(
        &self,
        browser: &str,
        browser_id: i64,
        size: u64,
    ) -> Result<Option<Download>> {
        let Some(id) = self.in_progress_id(browser, browser_id)? else {
            return Ok(None);
        };
        self.conn.execute(
            "UPDATE downloads SET state = 'complete', size = ?2, finished_at = ?3 WHERE id = ?1",
            params![id, size as i64, crate::journal::now()],
        )?;
        self.download(id)
    }

    pub fn download_interrupted(&self, browser: &str, browser_id: i64) -> Result<()> {
        if let Some(id) = self.in_progress_id(browser, browser_id)? {
            self.download_set_state(id, DownloadState::Interrupted)?;
        }
        Ok(())
    }

    pub fn download_set_state(&self, id: i64, state: DownloadState) -> Result<()> {
        self.conn.execute(
            "UPDATE downloads SET state = ?2 WHERE id = ?1",
            params![id, state.as_str()],
        )?;
        Ok(())
    }

    pub fn download(&self, id: i64) -> Result<Option<Download>> {
        let sql = format!("SELECT {COLS} FROM downloads WHERE id = ?1");
        Ok(self.conn.query_row(&sql, [id], download).optional()?)
    }

    /// O registro mais recente deste caminho é um download em andamento, iniciado
    /// depois de `since`. Só o mais recente conta: uma linha antiga que ficou presa
    /// em andamento não pode segurar um arquivo que já terminou de baixar.
    pub fn download_active(&self, path: &Path, since: SystemTime) -> Result<bool> {
        Ok(self
            .download_at(path)?
            .is_some_and(|d| d.state == DownloadState::InProgress && d.started_at >= secs(since)))
    }

    /// Há um download `complete` para este caminho que terminou depois de `since`.
    pub fn download_finished_since(&self, path: &Path, since: SystemTime) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM downloads WHERE current_path = ?1 \
             AND state = 'complete' AND finished_at >= ?2)",
            params![bytes(path), secs(since)],
            |r| r.get(0),
        )?)
    }

    /// Registro do arquivo para a biblioteca. Arquivos que chegam numa pasta monitorada
    /// sem a extensão (Chromium sem ela, `wget`, cópia manual) ganham uma linha
    /// `source = 'folder'`. Devolve o id.
    pub fn ensure_download(&self, f: &Facts) -> Result<i64> {
        if let Some(d) = f
            .download_id
            .map(|id| self.download(id))
            .transpose()?
            .flatten()
        {
            if d.current_path != f.path {
                // movido à mão para cá: o xattr acompanha o arquivo
                self.conn.execute(
                    "UPDATE downloads SET current_path = ?2, state = 'complete' WHERE id = ?1",
                    params![d.id, bytes(&f.path)],
                )?;
            }
            return Ok(d.id);
        }
        if let Some(d) = self.download_at(&f.path)?
            && matches!(d.state, DownloadState::Complete | DownloadState::InProgress)
        {
            return Ok(d.id);
        }
        let mtime = secs(f.modified);
        self.conn.execute(
            "INSERT INTO downloads (source, path, current_path, url, referrer, mime, size, \
             state, started_at, finished_at) VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                Source::Folder.as_str(),
                bytes(&f.path),
                f.origin_url,
                f.referrer_url,
                f.mime,
                f.size as i64,
                DownloadState::Complete.as_str(),
                mtime
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Marca `gone` nos registros cujo arquivo não está mais no caminho atual. Os em
    /// andamento só depois de [`STALE_SECS`]; os da lixeira não são tocados.
    pub(crate) fn sweep_gone_downloads(&self) -> Result<usize> {
        let stale = crate::journal::now() - STALE_SECS as i64;
        let mut stmt = self.conn.prepare(
            "SELECT id, current_path FROM downloads WHERE state = 'complete' \
             OR (state = 'in_progress' AND started_at < ?1)",
        )?;
        let gone: Vec<i64> = stmt
            .query_map([stale], |r| Ok((r.get::<_, i64>(0)?, path(r.get(1)?))))?
            .filter_map(|r| r.ok())
            .filter(|(_, p)| std::fs::symlink_metadata(p).is_err())
            .map(|(id, _)| id)
            .collect();
        let tx = self.conn.unchecked_transaction()?;
        for id in &gone {
            tx.execute("UPDATE downloads SET state = 'gone' WHERE id = ?1", [id])?;
        }
        tx.commit()?;
        Ok(gone.len())
    }

    /// Página da biblioteca, mais novos primeiro. Estado, período e busca são filtrados
    /// no SQL; pasta, categoria e site em Rust, pedindo blocos até completar `limit`.
    pub fn library(&self, q: &LibraryQuery) -> Result<Vec<LibraryItem>> {
        let stale = journal::now() - STALE_SECS as i64;
        let state = match q.place {
            Place::All => format!("(state != 'in_progress' OR started_at >= {stale})"),
            Place::InWatch | Place::Organized => "state = 'complete'".into(),
            Place::Trashed => "state = 'trashed'".into(),
            Place::Missing => "state = 'gone'".into(),
            Place::Downloading => format!("state = 'in_progress' AND started_at >= {stale}"),
        };
        let sql = format!(
            "SELECT {COLS} FROM downloads WHERE {LIBRARY_ROWS} AND {state} AND id < ?1 \
             AND started_at >= ?2 AND (?3 = '%%' OR CAST(current_path AS TEXT) LIKE ?3 ESCAPE '\\' \
             OR url LIKE ?3 ESCAPE '\\' OR page_title LIKE ?3 ESCAPE '\\' \
             OR referrer LIKE ?3 ESCAPE '\\') ORDER BY id DESC LIMIT ?4"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let pattern = journal::like(q.search);
        let keep = |d: &Download| {
            (match q.place {
                Place::InWatch => d.in_watch(q.watch),
                Place::Organized => !d.in_watch(q.watch),
                _ => true,
            }) && q.category.is_none_or(|c| d.category() == c)
                && q.site.is_none_or(|s| d.site().as_deref() == Some(s))
        };
        let mut out = Vec::new();
        let mut before = q.before_id.unwrap_or(i64::MAX);
        let block = (q.limit * 4).max(100) as i64;
        while out.len() < q.limit {
            let rows: Vec<Download> = stmt
                .query_map(
                    params![before, q.since.unwrap_or(i64::MIN), pattern, block],
                    download,
                )?
                .collect::<rusqlite::Result<_>>()?;
            let Some(last) = rows.last() else { break };
            before = last.id;
            let full = rows.len() as i64 == block;
            for d in rows.into_iter().filter(|d| keep(d)) {
                if out.len() == q.limit {
                    break;
                }
                out.push(LibraryItem {
                    category: d.category(),
                    site: d.site(),
                    last_action: self.last_action_at(&d.current_path)?,
                    download: d,
                });
            }
            if !full {
                break;
            }
        }
        Ok(out)
    }

    fn last_action_at(&self, p: &Path) -> Result<Option<Entry>> {
        let sql = format!(
            "SELECT {} FROM actions WHERE (dst = ?1 OR src = ?1) AND error IS NULL \
             ORDER BY id DESC LIMIT 1",
            journal::COLS
        );
        Ok(self
            .conn
            .query_row(&sql, [bytes(p)], journal::entry)
            .optional()?)
    }

    /// Sites com mais arquivos na biblioteca (os chips do filtro).
    pub fn top_sites(&self, limit: usize) -> Result<Vec<(String, u64)>> {
        let sql = format!("SELECT {COLS} FROM downloads WHERE {LIBRARY_ROWS}");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut counts = std::collections::HashMap::<String, u64>::new();
        for d in stmt.query_map([], download)? {
            if let Some(site) = d?.site() {
                *counts.entry(site).or_default() += 1;
            }
        }
        let mut v: Vec<_> = counts.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v.truncate(limit);
        Ok(v)
    }

    /// Muda quando a biblioteca ganha um registro ou um download termina; o app usa
    /// junto com `max_id` para se atualizar sozinho.
    pub fn library_token(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(id), 0) || ':' || COALESCE(MAX(finished_at), 0) || ':' || \
             SUM(state = 'gone') FROM downloads",
                [],
                |r| r.get::<_, Option<String>>(0),
            )?
            .unwrap_or_default())
    }

    /// Registro mais recente do arquivo que está (ou estava) neste caminho.
    pub fn download_at(&self, path: &Path) -> Result<Option<Download>> {
        let sql = format!(
            "SELECT {COLS} FROM downloads WHERE current_path = ?1 ORDER BY id DESC LIMIT 1"
        );
        Ok(self
            .conn
            .query_row(&sql, [bytes(path)], download)
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::time::Duration;

    fn open() -> (tempfile::TempDir, Journal) {
        let d = tempfile::tempdir().unwrap();
        let j = Journal::open(&d.path().join("journal.db")).unwrap();
        (d, j)
    }

    fn started<'a>(path: &'a Path, browser_id: i64, at: i64) -> Started<'a> {
        Started {
            browser: "firefox",
            browser_id,
            path,
            url: "https://cdn.example.com/a.pdf",
            referrer: Some("https://example.com/page"),
            page_title: Some("Página"),
            mime: Some("application/pdf"),
            started_at: at,
        }
    }

    fn ago(s: u64) -> SystemTime {
        SystemTime::now() - Duration::from_secs(s)
    }

    #[test]
    fn migrates_v2_database() {
        let d = tempfile::tempdir().unwrap();
        let db = d.path().join("j.db");
        {
            let c = Connection::open(&db).unwrap();
            c.execute_batch(
                "CREATE TABLE actions (id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, \
                 rule TEXT NOT NULL, kind TEXT NOT NULL, src BLOB NOT NULL, dst BLOB, \
                 undone INTEGER NOT NULL DEFAULT 0, error TEXT, size INTEGER, \
                 gone INTEGER NOT NULL DEFAULT 0); \
                 INSERT INTO actions (ts, rule, kind, src) VALUES (1, 'r', 'trash', x'2f61'); \
                 PRAGMA user_version = 2;",
            )
            .unwrap();
        }
        let j = Journal::open(&db).unwrap();
        let v: i64 = j
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 3);
        assert_eq!(j.recent(10).unwrap().len(), 1);
        assert!(j.download_at(Path::new("/x")).unwrap().is_none());
        drop(j);
        Journal::open(&db).unwrap();
    }

    #[test]
    fn started_then_finished() {
        let (_d, j) = open();
        let p = Path::new("/dl/a.pdf");
        j.download_started(&started(p, 7, crate::journal::now()))
            .unwrap();
        assert!(j.download_active(p, ago(3600)).unwrap());
        assert!(!j.download_finished_since(p, ago(60)).unwrap());

        let d = j.download_finished("firefox", 7, 1234).unwrap().unwrap();
        assert_eq!(d.state, DownloadState::Complete);
        assert_eq!(d.size, Some(1234));
        assert!(d.finished_at.is_some());
        assert_eq!(d.source, Source::Browser);
        assert_eq!(d.page_title.as_deref(), Some("Página"));
        assert!(!j.download_active(p, ago(3600)).unwrap());
        assert!(j.download_finished_since(p, ago(60)).unwrap());
        assert_eq!(j.download_at(p).unwrap().unwrap().id, d.id);
    }

    #[test]
    fn renamed_updates_path_and_current_path() {
        let (_d, j) = open();
        let id = j
            .download_started(&started(Path::new("/dl/a.pdf"), 1, crate::journal::now()))
            .unwrap();
        let new = Path::new("/dl/a (1).pdf");
        assert_eq!(j.download_renamed("firefox", 1, new).unwrap(), Some(id));
        let d = j.download(id).unwrap().unwrap();
        assert_eq!(d.path, new);
        assert_eq!(d.current_path, new);
        assert!(j.download_active(new, ago(60)).unwrap());
    }

    #[test]
    fn finished_matches_latest_in_progress() {
        let (_d, j) = open();
        let now = crate::journal::now();
        let first = j
            .download_started(&started(Path::new("/dl/a.pdf"), 3, now))
            .unwrap();
        j.download_finished("firefox", 3, 1).unwrap();
        let second = j
            .download_started(&started(Path::new("/dl/b.pdf"), 3, now))
            .unwrap();
        let d = j.download_finished("firefox", 3, 2).unwrap().unwrap();
        assert_eq!(d.id, second);
        assert_eq!(j.download(first).unwrap().unwrap().size, Some(1));
    }

    #[test]
    fn repeated_started_is_the_same_download() {
        let (_d, j) = open();
        let p = Path::new("/dl/README.md");
        let now = crate::journal::now();
        let mut s = started(p, 2, now);
        s.page_title = None;
        let a = j.download_started(&s).unwrap();
        s.page_title = Some("Título");
        let b = j.download_started(&s).unwrap();
        assert_eq!(a, b);
        assert_eq!(
            j.download(a).unwrap().unwrap().page_title.as_deref(),
            Some("Título")
        );
        j.download_finished("firefox", 2, 10).unwrap();
        assert!(!j.download_active(p, ago(60)).unwrap());
    }

    #[test]
    fn same_id_from_an_older_session_is_interrupted() {
        let (_d, j) = open();
        let now = crate::journal::now();
        let old = j
            .download_started(&started(Path::new("/dl/a.pdf"), 2, now - 600))
            .unwrap();
        let new = j
            .download_started(&started(Path::new("/dl/b.pdf"), 2, now))
            .unwrap();
        assert_ne!(old, new);
        assert_eq!(
            j.download(old).unwrap().unwrap().state,
            DownloadState::Interrupted
        );
    }

    #[test]
    fn only_the_latest_row_of_a_path_counts() {
        let (_d, j) = open();
        let p = Path::new("/dl/a.pdf");
        let now = crate::journal::now();
        // linha antiga presa em andamento (ex.: banco gravado pela versão com o bug)
        j.conn
            .execute(
                "INSERT INTO downloads (source, browser, browser_id, path, current_path, state, started_at) \
                 VALUES ('browser', 'firefox', 9, ?1, ?1, 'in_progress', ?2)",
                params![bytes(p), now],
            )
            .unwrap();
        assert!(j.download_active(p, ago(60)).unwrap());
        j.download_started(&started(p, 2, now)).unwrap();
        j.download_finished("firefox", 2, 1).unwrap();
        assert!(!j.download_active(p, ago(60)).unwrap());
    }

    /// Insere um registro já terminado em `dir/name`, vindo de `referrer`.
    fn add(j: &Journal, dir: &Path, name: &str, referrer: &str, mime: &str) -> i64 {
        let p = dir.join(name);
        std::fs::write(&p, "x").unwrap();
        let id = j
            .download_started(&Started {
                browser: "firefox",
                browser_id: j.max_id().unwrap() + crate::journal::now() % 1000 + name.len() as i64,
                path: &p,
                url: "https://cdn.net/x",
                referrer: Some(referrer),
                page_title: Some("Título da página"),
                mime: Some(mime),
                started_at: crate::journal::now(),
            })
            .unwrap();
        j.conn
            .execute(
                "UPDATE downloads SET state = 'complete' WHERE id = ?1",
                [id],
            )
            .unwrap();
        id
    }

    fn q<'a>(place: Place, watch: &'a [PathBuf]) -> LibraryQuery<'a> {
        LibraryQuery {
            place,
            category: None,
            site: None,
            since: None,
            search: "",
            before_id: None,
            limit: 50,
            watch,
        }
    }

    fn ids(items: &[LibraryItem]) -> Vec<i64> {
        items.iter().map(|i| i.download.id).collect()
    }

    #[test]
    fn library_places_categories_sites_and_search() {
        let (d, j) = open();
        let watch_dir = d.path().join("Downloads");
        let docs = d.path().join("Docs");
        std::fs::create_dir_all(&watch_dir).unwrap();
        std::fs::create_dir_all(&docs).unwrap();
        let watch = vec![watch_dir.clone()];
        let a = add(
            &j,
            &watch_dir,
            "fatura.pdf",
            "https://nubank.com.br/f",
            "application/pdf",
        );
        let b = add(&j, &docs, "foto.png", "https://github.com/x", "image/png");
        let c = add(
            &j,
            &watch_dir,
            "app.deb",
            "https://github.com/y",
            "application/octet-stream",
        );
        j.download_set_state(c, DownloadState::Trashed).unwrap();
        let running = j
            .download_started(&started(
                &watch_dir.join("big.iso"),
                999,
                crate::journal::now(),
            ))
            .unwrap();
        let interrupted = j
            .download_started(&started(
                &watch_dir.join("x.zip"),
                998,
                crate::journal::now(),
            ))
            .unwrap();
        j.download_set_state(interrupted, DownloadState::Interrupted)
            .unwrap();

        assert_eq!(
            ids(&j.library(&q(Place::All, &watch)).unwrap()),
            vec![running, c, b, a]
        );
        assert_eq!(
            ids(&j.library(&q(Place::InWatch, &watch)).unwrap()),
            vec![a]
        );
        assert_eq!(
            ids(&j.library(&q(Place::Organized, &watch)).unwrap()),
            vec![b]
        );
        assert_eq!(
            ids(&j.library(&q(Place::Trashed, &watch)).unwrap()),
            vec![c]
        );
        assert_eq!(
            ids(&j.library(&q(Place::Downloading, &watch)).unwrap()),
            vec![running]
        );

        let mut by = q(Place::All, &watch);
        by.category = Some(Category::Installer);
        assert_eq!(ids(&j.library(&by).unwrap()), vec![running, c]); // .iso também é instalador
        let mut by = q(Place::All, &watch);
        by.site = Some("github.com");
        assert_eq!(ids(&j.library(&by).unwrap()), vec![c, b]);
        for (search, want) in [
            ("fatura", vec![a]),
            ("cdn.net", vec![c, b, a]),
            ("título", vec![c, b, a]),
            ("example.com/page", vec![running]),
            ("%", vec![]),
        ] {
            let mut by = q(Place::All, &watch);
            by.search = search;
            assert_eq!(ids(&j.library(&by).unwrap()), want, "{search}");
        }

        let items = j.library(&q(Place::All, &watch)).unwrap();
        let png = items.iter().find(|i| i.download.id == b).unwrap();
        assert_eq!(png.category, Category::Image);
        assert_eq!(png.site.as_deref(), Some("github.com"));

        assert_eq!(
            j.top_sites(5).unwrap(),
            vec![
                ("github.com".to_string(), 2),
                ("example.com".into(), 1),
                ("nubank.com.br".into(), 1)
            ]
        );
    }

    #[test]
    fn library_paginates_and_dedupes_paths() {
        let (d, j) = open();
        let dir = d.path().to_path_buf();
        let watch = vec![dir.clone()];
        let ids_in: Vec<i64> = (0..5)
            .map(|i| {
                add(
                    &j,
                    &dir,
                    &format!("f{i}.txt"),
                    "https://s.com/",
                    "text/plain",
                )
            })
            .collect();
        let mut page = q(Place::All, &watch);
        page.limit = 2;
        assert_eq!(ids(&j.library(&page).unwrap()), vec![ids_in[4], ids_in[3]]);
        page.before_id = Some(ids_in[3]);
        assert_eq!(ids(&j.library(&page).unwrap()), vec![ids_in[2], ids_in[1]]);

        // dois registros para o mesmo caminho: só o mais novo aparece
        let again = add(&j, &dir, "f0.txt", "https://s.com/", "text/plain");
        let all = ids(&j.library(&q(Place::All, &watch)).unwrap());
        assert!(all.contains(&again) && !all.contains(&ids_in[0]));
    }

    #[test]
    fn last_action_and_token() {
        let (d, j) = open();
        let token0 = j.library_token().unwrap();
        let id = add(&j, d.path(), "a.pdf", "https://s.com/", "application/pdf");
        assert_ne!(j.library_token().unwrap(), token0);
        let src = d.path().join("a.pdf");
        let dst = d.path().join("out.pdf");
        std::fs::rename(&src, &dst).unwrap();
        let done = crate::actions::Done {
            kind: Kind::Move,
            src: src.clone(),
            dst: Some(dst.clone()),
        };
        let action = j.record("Docs", &done, 1).unwrap();
        let items = j.library(&q(Place::All, &[])).unwrap();
        assert_eq!(items[0].download.id, id);
        assert_eq!(items[0].download.current_path, dst);
        assert_eq!(items[0].last_action.as_ref().unwrap().id, action);
    }

    #[test]
    fn active_ignores_stale_rows() {
        let (_d, j) = open();
        let p = Path::new("/dl/a.pdf");
        j.download_started(&started(p, 1, secs(ago(25 * 3600))))
            .unwrap();
        assert!(!j.download_active(p, ago(STALE_SECS)).unwrap());
    }

    #[test]
    fn unknown_finished_returns_none() {
        let (_d, j) = open();
        assert!(j.download_finished("chromium", 99, 1).unwrap().is_none());
        assert!(
            j.download_renamed("chromium", 99, Path::new("/x"))
                .unwrap()
                .is_none()
        );
        j.download_interrupted("chromium", 99).unwrap();
    }

    #[test]
    fn interrupted_sets_state() {
        let (_d, j) = open();
        let p = Path::new("/dl/a.pdf");
        let id = j
            .download_started(&started(p, 5, crate::journal::now()))
            .unwrap();
        j.download_interrupted("firefox", 5).unwrap();
        assert_eq!(
            j.download(id).unwrap().unwrap().state,
            DownloadState::Interrupted
        );
        assert!(!j.download_active(p, ago(60)).unwrap());
    }
}
