use crate::{
    Result,
    actions::{Done, Kind},
};
use rusqlite::{Connection, OptionalExtension, Row, params, types::Type};
use std::{
    ffi::OsString,
    fs,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS actions (
    id     INTEGER PRIMARY KEY,
    ts     INTEGER NOT NULL,
    rule   TEXT    NOT NULL,
    kind   TEXT    NOT NULL,
    src    BLOB    NOT NULL,
    dst    BLOB,
    undone INTEGER NOT NULL DEFAULT 0,
    error  TEXT
);
CREATE INDEX IF NOT EXISTS actions_dst ON actions(dst);
";

const COLS: &str = "id, ts, rule, kind, src, dst, undone, error";

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: i64,
    pub ts: i64,
    pub rule: String,
    pub kind: Kind,
    pub src: PathBuf,
    pub dst: Option<PathBuf>,
    pub undone: bool,
    pub error: Option<String>,
}

pub struct Journal {
    conn: Connection,
}

pub fn default_path() -> PathBuf {
    crate::xdg_dir("XDG_DATA_HOME", ".local/share").join("tidyflow/journal.db")
}

impl Journal {
    pub fn open(path: &Path) -> Result<Journal> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Journal { conn })
    }

    pub fn record(&self, rule: &str, done: &Done) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO actions (ts, rule, kind, src, dst) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                now(),
                rule,
                done.kind.as_str(),
                bytes(&done.src),
                done.dst.as_deref().map(bytes)
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn record_error(&self, rule: &str, kind: Kind, src: &Path, error: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO actions (ts, rule, kind, src, error) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now(), rule, kind.as_str(), bytes(src), error],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get(&self, id: i64) -> Result<Option<Entry>> {
        let sql = format!("SELECT {COLS} FROM actions WHERE id = ?1");
        Ok(self.conn.query_row(&sql, [id], entry).optional()?)
    }

    pub fn last_undoable(&self) -> Result<Option<Entry>> {
        let sql = format!(
            "SELECT {COLS} FROM actions WHERE undone = 0 AND error IS NULL ORDER BY id DESC LIMIT 1"
        );
        Ok(self.conn.query_row(&sql, [], entry).optional()?)
    }

    pub fn recent(&self, limit: usize) -> Result<Vec<Entry>> {
        let sql = format!("SELECT {COLS} FROM actions ORDER BY id DESC LIMIT ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([limit as i64], entry)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn mark_undone(&self, id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE actions SET undone = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Algum registro desfeito tem este caminho como origem (fallback quando o xattr falha).
    pub fn was_undone_src(&self, path: &Path) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM actions WHERE src = ?1 AND undone = 1)",
            params![bytes(path)],
            |r| r.get(0),
        )?)
    }

    pub fn is_dst_since(&self, path: &Path, since: SystemTime) -> Result<bool> {
        let since = since
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM actions WHERE dst = ?1 AND ts >= ?2)",
            params![bytes(path), since],
            |r| r.get(0),
        )?)
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn bytes(p: &Path) -> &[u8] {
    p.as_os_str().as_bytes()
}

fn path(b: Vec<u8>) -> PathBuf {
    PathBuf::from(OsString::from_vec(b))
}

fn entry(r: &Row) -> rusqlite::Result<Entry> {
    let kind: String = r.get(3)?;
    Ok(Entry {
        id: r.get(0)?,
        ts: r.get(1)?,
        rule: r.get(2)?,
        kind: Kind::parse(&kind).ok_or(rusqlite::Error::InvalidColumnType(3, kind, Type::Text))?,
        src: path(r.get(4)?),
        dst: r.get::<_, Option<Vec<u8>>>(5)?.map(path),
        undone: r.get(6)?,
        error: r.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    fn open() -> (tempfile::TempDir, Journal) {
        let d = tempfile::tempdir().unwrap();
        let j = Journal::open(&d.path().join("sub/journal.db")).unwrap();
        (d, j)
    }

    fn done(src: &str, dst: &str) -> Done {
        Done {
            kind: Kind::Move,
            src: src.into(),
            dst: Some(dst.into()),
        }
    }

    #[test]
    fn record_and_read_back() {
        let (_d, j) = open();
        let id = j.record("pdfs", &done("/a/x.pdf", "/b/x.pdf")).unwrap();
        let e = j.get(id).unwrap().unwrap();
        assert_eq!(e.rule, "pdfs");
        assert_eq!(e.kind, Kind::Move);
        assert_eq!(e.src, PathBuf::from("/a/x.pdf"));
        assert_eq!(e.dst, Some(PathBuf::from("/b/x.pdf")));
        assert!(!e.undone && e.error.is_none());
        assert!(j.get(999).unwrap().is_none());
    }

    #[test]
    fn non_utf8_paths_survive() {
        let (_d, j) = open();
        let weird = PathBuf::from(OsStr::from_bytes(b"/a/\xff.bin"));
        let id = j
            .record(
                "r",
                &Done {
                    kind: Kind::Trash,
                    src: weird.clone(),
                    dst: None,
                },
            )
            .unwrap();
        let e = j.get(id).unwrap().unwrap();
        assert_eq!(e.src, weird);
        assert_eq!(e.dst, None);
    }

    #[test]
    fn last_undoable_skips_undone_and_errors() {
        let (_d, j) = open();
        let a = j.record("r", &done("/a", "/b")).unwrap();
        let b = j.record("r", &done("/c", "/d")).unwrap();
        j.record_error("r", Kind::Move, Path::new("/e"), "sem permissão")
            .unwrap();
        assert_eq!(j.last_undoable().unwrap().unwrap().id, b);
        j.mark_undone(b).unwrap();
        assert!(j.get(b).unwrap().unwrap().undone);
        assert_eq!(j.last_undoable().unwrap().unwrap().id, a);
    }

    #[test]
    fn recent_is_newest_first() {
        let (_d, j) = open();
        for i in 0..5 {
            j.record("r", &done(&format!("/s{i}"), "/d")).unwrap();
        }
        let ids: Vec<i64> = j.recent(3).unwrap().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![5, 4, 3]);
    }

    #[test]
    fn was_undone_src_only_after_undo() {
        let (_d, j) = open();
        let id = j.record("r", &done("/a/x", "/b/x")).unwrap();
        assert!(!j.was_undone_src(Path::new("/a/x")).unwrap());
        j.mark_undone(id).unwrap();
        assert!(j.was_undone_src(Path::new("/a/x")).unwrap());
        assert!(!j.was_undone_src(Path::new("/b/x")).unwrap());
    }

    #[test]
    fn recent_dst() {
        let (_d, j) = open();
        j.record("r", &done("/a", "/b/x")).unwrap();
        let since = SystemTime::now() - std::time::Duration::from_secs(5);
        assert!(j.is_dst_since(Path::new("/b/x"), since).unwrap());
        assert!(!j.is_dst_since(Path::new("/b/y"), since).unwrap());
        let future = SystemTime::now() + std::time::Duration::from_secs(60);
        assert!(!j.is_dst_since(Path::new("/b/x"), future).unwrap());
    }

    #[test]
    fn two_connections_share_state() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("j.db");
        let a = Journal::open(&path).unwrap();
        let b = Journal::open(&path).unwrap();
        let id = a.record("r", &done("/a", "/b")).unwrap();
        b.mark_undone(id).unwrap();
        assert!(a.get(id).unwrap().unwrap().undone);
    }
}
