use crate::{
    Result,
    actions::{Done, Kind},
};
use rusqlite::{Connection, OptionalExtension, Row, params, types::Type};
use std::{
    ffi::OsString,
    fs,
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
CREATE INDEX IF NOT EXISTS actions_src ON actions(src);
CREATE INDEX IF NOT EXISTS actions_ts ON actions(ts);
";

pub(crate) const COLS: &str = "id, ts, rule, kind, src, dst, undone, error, size";

/// v1: coluna `size` (bytes do arquivo no momento da ação).
/// v2: coluna `gone` (o arquivo sumiu de onde a ação o deixou; some do histórico).
/// v3: tabela `downloads` (registro da extensão de navegador e da biblioteca).
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let steps = [
        "ALTER TABLE actions ADD COLUMN size INTEGER;",
        "ALTER TABLE actions ADD COLUMN gone INTEGER NOT NULL DEFAULT 0;",
        crate::downloads::SCHEMA_V3,
    ];
    for (i, step) in steps.iter().enumerate().skip(v.max(0) as usize) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(&format!("{step} PRAGMA user_version = {};", i + 1))?;
        tx.commit()?;
    }
    Ok(())
}

/// Entradas que aparecem no histórico: erros sempre; ações só enquanto o arquivo existe.
const VISIBLE: &str = "gone = 0";

/// Filtros da tela Histórico.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    /// move, copy e rename sem erro e não desfeitos
    Moved,
    /// trash sem erro e não desfeito
    Trashed,
    Undone,
    Errors,
}

impl Filter {
    fn sql(self) -> &'static str {
        match self {
            Filter::All => "1",
            Filter::Moved => "kind IN ('move', 'copy', 'rename') AND error IS NULL AND undone = 0",
            Filter::Trashed => "kind = 'trash' AND error IS NULL AND undone = 0",
            Filter::Undone => "undone > 0",
            Filter::Errors => "error IS NOT NULL",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Counts {
    pub all: u64,
    pub moved: u64,
    pub trashed: u64,
    pub undone: u64,
    pub errors: u64,
}

/// Início (timestamp unix) de cada período das estatísticas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Periods {
    pub today: i64,
    pub yesterday: i64,
    /// hoje e os 6 dias anteriores
    pub week: i64,
}

impl Periods {
    /// Meia-noite local de hoje, de ontem e de 6 dias atrás.
    pub fn local(now: SystemTime) -> Periods {
        use chrono::{DateTime, Days, Local, NaiveDate};
        let today: NaiveDate = DateTime::<Local>::from(now).date_naive();
        let start = |d: NaiveDate| {
            d.and_hms_opt(0, 0, 0)
                .and_then(|t| t.and_local_timezone(Local).earliest())
                .map_or(0, |t| t.timestamp())
        };
        let back = |n| today.checked_sub_days(Days::new(n)).unwrap_or(today);
        Periods {
            today: start(today),
            yesterday: start(back(1)),
            week: start(back(6)),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Stats {
    pub organized_today: u64,
    pub organized_yesterday: u64,
    pub trashed_week: u64,
    /// bytes de move/trash não desfeitos na semana, saídos de uma pasta monitorada
    pub freed_week: u64,
    pub errors_week: u64,
}

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
    /// `None` nas entradas da Fase 1 e nos erros
    pub size: Option<u64>,
}

pub struct Journal {
    pub(crate) conn: Connection,
}

pub fn default_path() -> PathBuf {
    crate::data_dir().join("journal.db")
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
        migrate(&conn)?;
        Ok(Journal { conn })
    }

    /// Grava a ação e, na mesma transação, move o registro do arquivo na biblioteca.
    pub fn record(&self, rule: &str, done: &Done, size: u64) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO actions (ts, rule, kind, src, dst, size) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                now(),
                rule,
                done.kind.as_str(),
                bytes(&done.src),
                done.dst.as_deref().map(bytes),
                size as i64
            ],
        )?;
        let id = tx.last_insert_rowid();
        crate::downloads::follow(&tx, done.kind, &done.src, done.dst.as_deref(), false)?;
        tx.commit()?;
        Ok(id)
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
            "SELECT {COLS} FROM actions WHERE {VISIBLE} AND undone = 0 AND error IS NULL \
             ORDER BY id DESC LIMIT 1"
        );
        Ok(self.conn.query_row(&sql, [], entry).optional()?)
    }

    pub fn recent(&self, limit: usize) -> Result<Vec<Entry>> {
        let sql = format!("SELECT {COLS} FROM actions WHERE {VISIBLE} ORDER BY id DESC LIMIT ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([limit as i64], entry)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// `marked`: a marca xattr foi gravada (undone = 1); senão undone = 2 e o journal é o fallback.
    pub fn mark_undone(&self, id: i64, marked: bool) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE actions SET undone = ?2 WHERE id = ?1",
            params![id, if marked { 1 } else { 2 }],
        )?;
        if let Some(e) = self.get(id)?
            && e.error.is_none()
        {
            crate::downloads::follow(&tx, e.kind, &e.src, e.dst.as_deref(), true)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Algum undo sem marca xattr (undone = 2) tem este caminho como origem.
    pub fn was_undone_src(&self, path: &Path) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM actions WHERE src = ?1 AND undone = 2)",
            params![bytes(path)],
            |r| r.get(0),
        )?)
    }

    /// Já existe uma cópia bem-sucedida e não desfeita de `path` feita depois de `mtime`.
    pub fn was_copied_since(&self, path: &Path, mtime: SystemTime) -> Result<bool> {
        let since = mtime
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM actions WHERE kind = 'copy' AND src = ?1 \
             AND undone = 0 AND error IS NULL AND ts >= ?2)",
            params![bytes(path), since],
            |r| r.get(0),
        )?)
    }

    /// Página do histórico, mais novas primeiro. `search` procura na regra e nos caminhos.
    pub fn page(
        &self,
        filter: Filter,
        search: &str,
        before_id: Option<i64>,
        limit: usize,
    ) -> Result<Vec<Entry>> {
        let sql = format!(
            "SELECT {COLS} FROM actions WHERE {VISIBLE} AND ({}) AND {SEARCH} AND id < ?2 \
             ORDER BY id DESC LIMIT ?3",
            filter.sql()
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![like(search), before_id.unwrap_or(i64::MAX), limit as i64],
            entry,
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Quantas entradas cada filtro tem, com a mesma busca.
    pub fn counts(&self, search: &str) -> Result<Counts> {
        let n = |f: Filter| -> Result<u64> {
            let sql = format!(
                "SELECT COUNT(*) FROM actions WHERE {VISIBLE} AND ({}) AND {SEARCH}",
                f.sql()
            );
            Ok(self
                .conn
                .query_row(&sql, params![like(search)], |r| r.get::<_, i64>(0))?
                as u64)
        };
        Ok(Counts {
            all: n(Filter::All)?,
            moved: n(Filter::Moved)?,
            trashed: n(Filter::Trashed)?,
            undone: n(Filter::Undone)?,
            errors: n(Filter::Errors)?,
        })
    }

    pub fn stats(&self, p: &Periods, watch: &[PathBuf]) -> Result<Stats> {
        let ok = "error IS NULL AND undone = 0";
        let count = |sql: &str, a: i64, b: i64| -> Result<u64> {
            Ok(self
                .conn
                .query_row(sql, params![a, b], |r| r.get::<_, i64>(0))? as u64)
        };
        let organized = format!("SELECT COUNT(*) FROM actions WHERE {ok} AND ts >= ?1 AND ts < ?2");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT src, size FROM actions WHERE {ok} AND kind IN ('move', 'trash') \
             AND size IS NOT NULL AND ts >= ?1"
        ))?;
        let freed = stmt
            .query_map([p.week], |r| Ok((path(r.get(0)?), r.get::<_, i64>(1)?)))?
            .filter_map(|r| r.ok())
            .filter(|(src, _)| src.parent().is_some_and(|d| watch.iter().any(|w| w == d)))
            .map(|(_, size)| size.max(0) as u64)
            .sum();
        Ok(Stats {
            organized_today: count(&organized, p.today, i64::MAX)?,
            organized_yesterday: count(&organized, p.yesterday, p.today)?,
            trashed_week: count(
                &format!(
                    "SELECT COUNT(*) FROM actions WHERE {ok} AND kind = 'trash' AND ts >= ?1 AND ts < ?2"
                ),
                p.week,
                i64::MAX,
            )?,
            freed_week: freed,
            errors_week: count(
                "SELECT COUNT(*) FROM actions WHERE error IS NOT NULL AND ts >= ?1 AND ts < ?2",
                p.week,
                i64::MAX,
            )?,
        })
    }

    /// Última ação bem-sucedida e não desfeita da regra (prévia do editor).
    pub fn last_for_rule(&self, rule: &str) -> Result<Option<Entry>> {
        let sql = format!(
            "SELECT {COLS} FROM actions WHERE {VISIBLE} AND rule = ?1 AND error IS NULL AND undone = 0 \
             ORDER BY id DESC LIMIT 1"
        );
        Ok(self.conn.query_row(&sql, [rule], entry).optional()?)
    }

    /// Marca `gone` nas ações cujo arquivo não está mais onde a ação o deixou
    /// (no destino; na origem, se desfeita): apagado, movido, lixeira esvaziada,
    /// ou substituído pelo de uma ação mais nova no mesmo caminho.
    /// Também marca na biblioteca os arquivos que sumiram do caminho atual.
    /// Devolve quantas ações foram marcadas.
    pub fn sweep_gone(&self) -> Result<usize> {
        let mut stmt = self.conn.prepare(
            "SELECT id, src, dst, undone FROM actions WHERE gone = 0 AND error IS NULL \
             ORDER BY id DESC",
        )?;
        let rows: Vec<(i64, Option<PathBuf>)> = stmt
            .query_map([], |r| {
                let undone: i64 = r.get(3)?;
                let dst = r.get::<_, Option<Vec<u8>>>(2)?.map(path);
                let here = if undone > 0 {
                    Some(path(r.get(1)?))
                } else {
                    dst
                };
                Ok((r.get(0)?, here))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut newer = std::collections::HashSet::new();
        let gone: Vec<i64> = rows
            .into_iter()
            .filter(|(_, here)| {
                here.as_ref()
                    .is_some_and(|p| !newer.insert(p.clone()) || fs::symlink_metadata(p).is_err())
            })
            .map(|(id, _)| id)
            .collect();
        let tx = self.conn.unchecked_transaction()?;
        for id in &gone {
            tx.execute("UPDATE actions SET gone = 1 WHERE id = ?1", [id])?;
        }
        tx.commit()?;
        self.sweep_gone_downloads()?;
        Ok(gone.len())
    }

    /// Maior id do journal (0 se vazio): o app compara para saber se houve ação nova.
    pub fn max_id(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COALESCE(MAX(id), 0) FROM actions", [], |r| r.get(0))?)
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

/// Condição de busca; `?1` é o padrão LIKE (vazio casa tudo).
const SEARCH: &str = "(?1 = '%%' OR rule LIKE ?1 ESCAPE '\\' \
     OR CAST(src AS TEXT) LIKE ?1 ESCAPE '\\' OR CAST(dst AS TEXT) LIKE ?1 ESCAPE '\\')";

pub(crate) fn like(search: &str) -> String {
    let mut s = String::from("%");
    for c in search.trim().chars() {
        if matches!(c, '\\' | '%' | '_') {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('%');
    s
}

pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub(crate) fn bytes(p: &Path) -> &[u8] {
    p.as_os_str().as_encoded_bytes()
}

pub(crate) fn path(b: Vec<u8>) -> PathBuf {
    // SAFETY: os bytes saem de `bytes()` neste mesmo sistema (o banco é local).
    PathBuf::from(unsafe { OsString::from_encoded_bytes_unchecked(b) })
}

pub(crate) fn entry(r: &Row) -> rusqlite::Result<Entry> {
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
        size: r.get::<_, Option<i64>>(8)?.map(|n| n.max(0) as u64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn sweep_hides_entries_whose_file_is_gone() {
        let (d, j) = open();
        let kept = d.path().join("kept.pdf");
        fs::write(&kept, "x").unwrap();
        let src = d.path().join("src.pdf");
        let a = j
            .record("r", &done("/a/kept.pdf", kept.to_str().unwrap()), 0)
            .unwrap();
        let b = j
            .record("r", &done("/a/x.pdf", "/nao/existe.pdf"), 0)
            .unwrap();
        // desfeita: o arquivo voltou para a origem, que existe
        fs::write(&src, "x").unwrap();
        let c = j
            .record("r", &done(src.to_str().unwrap(), "/nao/existe2.pdf"), 0)
            .unwrap();
        j.mark_undone(c, true).unwrap();
        let e = j
            .record_error("r", Kind::Move, Path::new("/nao/existe3.pdf"), "falhou")
            .unwrap();

        assert_eq!(j.sweep_gone().unwrap(), 1);
        let ids: Vec<i64> = j.recent(10).unwrap().iter().map(|e| e.id).collect();
        assert_eq!(ids, [e, c, a]);
        assert!(!ids.contains(&b));
        assert_eq!(j.counts("").unwrap().all, 3);
        assert_eq!(j.last_undoable().unwrap().map(|e| e.id), Some(a));
        // idempotente
        assert_eq!(j.sweep_gone().unwrap(), 0);
        // ação mais nova no mesmo destino: a antiga foi substituída
        let f = j
            .record("r", &done("/a/kept2.pdf", kept.to_str().unwrap()), 0)
            .unwrap();
        assert_eq!(j.sweep_gone().unwrap(), 1);
        assert!(j.recent(10).unwrap().iter().all(|e| e.id != a));
        assert!(j.recent(10).unwrap().iter().any(|e| e.id == f));
    }

    #[test]
    fn migrates_v1_journal() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("j.db");
        {
            let c = Connection::open(&p).unwrap();
            c.execute_batch(SCHEMA).unwrap();
            c.execute_batch(
                "ALTER TABLE actions ADD COLUMN size INTEGER; PRAGMA user_version = 1;",
            )
            .unwrap();
        }
        let j = Journal::open(&p).unwrap();
        j.record("r", &done("/a", "/b"), 1).unwrap();
        assert_eq!(j.recent(1).unwrap().len(), 1);
    }

    #[test]
    fn record_and_read_back() {
        let (_d, j) = open();
        let id = j.record("pdfs", &done("/a/x.pdf", "/b/x.pdf"), 0).unwrap();
        let e = j.get(id).unwrap().unwrap();
        assert_eq!(e.rule, "pdfs");
        assert_eq!(e.kind, Kind::Move);
        assert_eq!(e.src, PathBuf::from("/a/x.pdf"));
        assert_eq!(e.dst, Some(PathBuf::from("/b/x.pdf")));
        assert!(!e.undone && e.error.is_none());
        assert!(j.get(999).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_survive() {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
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
                0,
            )
            .unwrap();
        let e = j.get(id).unwrap().unwrap();
        assert_eq!(e.src, weird);
        assert_eq!(e.dst, None);
    }

    #[test]
    fn last_undoable_skips_undone_and_errors() {
        let (_d, j) = open();
        let a = j.record("r", &done("/a", "/b"), 0).unwrap();
        let b = j.record("r", &done("/c", "/d"), 0).unwrap();
        j.record_error("r", Kind::Move, Path::new("/e"), "sem permissão")
            .unwrap();
        assert_eq!(j.last_undoable().unwrap().unwrap().id, b);
        j.mark_undone(b, true).unwrap();
        assert!(j.get(b).unwrap().unwrap().undone);
        assert_eq!(j.last_undoable().unwrap().unwrap().id, a);
    }

    #[test]
    fn was_copied_since_checks_kind_src_and_time() {
        let (_d, j) = open();
        let src = Path::new("/a/x.pdf");
        let t = |s| UNIX_EPOCH + Duration::from_secs(s);
        assert!(!j.was_copied_since(src, t(0)).unwrap());
        j.record("r", &done("/a/x.pdf", "/b/x.pdf"), 0).unwrap(); // move, não conta
        assert!(!j.was_copied_since(src, t(0)).unwrap());
        let c = Done {
            kind: Kind::Copy,
            src: src.into(),
            dst: Some("/b/x.pdf".into()),
        };
        let id = j.record("r", &c, 0).unwrap();
        assert!(j.was_copied_since(src, t(0)).unwrap());
        assert!(!j.was_copied_since(src, t(now() as u64 + 100)).unwrap());
        j.mark_undone(id, true).unwrap();
        assert!(!j.was_copied_since(src, t(0)).unwrap());
    }

    #[test]
    fn recent_is_newest_first() {
        let (_d, j) = open();
        for i in 0..5 {
            j.record("r", &done(&format!("/s{i}"), "/d"), 0).unwrap();
        }
        let ids: Vec<i64> = j.recent(3).unwrap().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![5, 4, 3]);
    }

    #[test]
    fn was_undone_src_only_after_undo() {
        let (_d, j) = open();
        let id = j.record("r", &done("/a/x", "/b/x"), 0).unwrap();
        assert!(!j.was_undone_src(Path::new("/a/x")).unwrap());
        j.mark_undone(id, true).unwrap();
        assert!(!j.was_undone_src(Path::new("/a/x")).unwrap());
        assert!(j.get(id).unwrap().unwrap().undone);
        j.mark_undone(id, false).unwrap();
        assert!(j.get(id).unwrap().unwrap().undone); // 2 também lê como true
        assert!(j.was_undone_src(Path::new("/a/x")).unwrap());
        assert!(!j.was_undone_src(Path::new("/b/x")).unwrap());
    }

    #[test]
    fn recent_dst() {
        let (_d, j) = open();
        j.record("r", &done("/a", "/b/x"), 0).unwrap();
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
        let id = a.record("r", &done("/a", "/b"), 0).unwrap();
        b.mark_undone(id, true).unwrap();
        assert!(a.get(id).unwrap().unwrap().undone);
    }

    fn kind_done(kind: Kind, src: &str, dst: Option<&str>) -> Done {
        Done {
            kind,
            src: src.into(),
            dst: dst.map(Into::into),
        }
    }

    fn ids(v: Vec<Entry>) -> Vec<i64> {
        v.iter().map(|e| e.id).collect()
    }

    #[test]
    fn migrates_phase1_database() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("j.db");
        {
            let c = Connection::open(&p).unwrap();
            c.execute_batch(
                "CREATE TABLE actions (id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, \
                 rule TEXT NOT NULL, kind TEXT NOT NULL, src BLOB NOT NULL, dst BLOB, \
                 undone INTEGER NOT NULL DEFAULT 0, error TEXT); \
                 INSERT INTO actions (ts, rule, kind, src) VALUES (1, 'r', 'trash', X'2F61');",
            )
            .unwrap();
        }
        let j = Journal::open(&p).unwrap();
        let e = j.get(1).unwrap().unwrap();
        assert_eq!(e.src, PathBuf::from("/a"));
        assert_eq!(e.size, None);
        let v: i64 = j
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 3);
        drop(j);
        Journal::open(&p).unwrap(); // idempotente
    }

    #[test]
    fn record_stores_size() {
        let (_d, j) = open();
        let id = j.record("r", &done("/a", "/b"), 1234).unwrap();
        assert_eq!(j.get(id).unwrap().unwrap().size, Some(1234));
    }

    #[test]
    fn filters_and_counts() {
        let (_d, j) = open();
        let mv = j
            .record("r", &kind_done(Kind::Move, "/a", Some("/b")), 1)
            .unwrap();
        let cp = j
            .record("r", &kind_done(Kind::Copy, "/c", Some("/d")), 1)
            .unwrap();
        let rn = j
            .record("r", &kind_done(Kind::Rename, "/e", Some("/f")), 1)
            .unwrap();
        let tr = j
            .record("r", &kind_done(Kind::Trash, "/g", None), 1)
            .unwrap();
        let er = j
            .record_error("r", Kind::Move, Path::new("/h"), "x")
            .unwrap();
        let un = j
            .record("r", &kind_done(Kind::Move, "/i", Some("/j")), 1)
            .unwrap();
        j.mark_undone(un, false).unwrap();
        let page = |f| ids(j.page(f, "", None, 50).unwrap());
        assert_eq!(page(Filter::All), vec![un, er, tr, rn, cp, mv]);
        assert_eq!(page(Filter::Moved), vec![rn, cp, mv]);
        assert_eq!(page(Filter::Trashed), vec![tr]);
        assert_eq!(page(Filter::Undone), vec![un]);
        assert_eq!(page(Filter::Errors), vec![er]);
        assert_eq!(
            j.counts("").unwrap(),
            Counts {
                all: 6,
                moved: 3,
                trashed: 1,
                undone: 1,
                errors: 1
            }
        );
    }

    #[test]
    fn search_matches_rule_or_path() {
        let (_d, j) = open();
        let a = j
            .record("Nubank", &done("/dl/x.pdf", "/docs/x.pdf"), 1)
            .unwrap();
        let b = j
            .record("Imagens", &done("/dl/fatura_1.png", "/img/fatura_1.png"), 1)
            .unwrap();
        let c = j
            .record("Imagens", &done("/dl/faturaX1.png", "/img/faturaX1.png"), 1)
            .unwrap();
        assert_eq!(
            ids(j.page(Filter::All, "nubank", None, 50).unwrap()),
            vec![a]
        );
        assert_eq!(
            ids(j.page(Filter::All, "fatura", None, 50).unwrap()),
            vec![c, b]
        );
        // `_` é literal, não curinga
        assert_eq!(
            ids(j.page(Filter::All, "fatura_", None, 50).unwrap()),
            vec![b]
        );
        assert_eq!(
            ids(j.page(Filter::All, "%", None, 50).unwrap()),
            Vec::<i64>::new()
        );
        assert_eq!(j.counts("imagens").unwrap().all, 2);
    }

    #[test]
    fn page_paginates_by_id() {
        let (_d, j) = open();
        for i in 0..5 {
            j.record("r", &done(&format!("/s{i}"), "/d"), 0).unwrap();
        }
        assert_eq!(ids(j.page(Filter::All, "", None, 2).unwrap()), vec![5, 4]);
        assert_eq!(
            ids(j.page(Filter::All, "", Some(4), 2).unwrap()),
            vec![3, 2]
        );
    }

    #[test]
    fn stats_by_period() {
        let (_d, j) = open();
        let at = |id: i64, ts: i64| {
            j.conn
                .execute("UPDATE actions SET ts = ?2 WHERE id = ?1", params![id, ts])
                .unwrap();
        };
        let p = Periods {
            today: 1000,
            yesterday: 900,
            week: 400,
        };
        let watch = vec![PathBuf::from("/dl")];
        let t1 = j
            .record("r", &kind_done(Kind::Move, "/dl/a", Some("/x/a")), 100)
            .unwrap();
        at(t1, 1001);
        let t2 = j
            .record("r", &kind_done(Kind::Trash, "/dl/b", None), 50)
            .unwrap();
        at(t2, 1500);
        let t3 = j
            .record("r", &kind_done(Kind::Copy, "/dl/c", Some("/x/c")), 7)
            .unwrap();
        at(t3, 1200); // copy não libera espaço
        let t4 = j
            .record("r", &kind_done(Kind::Move, "/outra/d", Some("/x/d")), 9)
            .unwrap();
        at(t4, 1200); // fora das pastas monitoradas
        let y = j
            .record("r", &kind_done(Kind::Trash, "/dl/e", None), 5)
            .unwrap();
        at(y, 950);
        let old = j
            .record("r", &kind_done(Kind::Trash, "/dl/f", None), 1000)
            .unwrap();
        at(old, 100); // antes da semana
        let un = j
            .record("r", &kind_done(Kind::Move, "/dl/g", Some("/x/g")), 1000)
            .unwrap();
        at(un, 1100);
        j.mark_undone(un, true).unwrap();
        let er = j
            .record_error("r", Kind::Move, Path::new("/dl/h"), "x")
            .unwrap();
        at(er, 500);
        assert_eq!(
            j.stats(&p, &watch).unwrap(),
            Stats {
                organized_today: 4,
                organized_yesterday: 1,
                trashed_week: 2,
                freed_week: 155,
                errors_week: 1,
            }
        );
    }

    #[test]
    fn last_for_rule_skips_errors_and_undone() {
        let (_d, j) = open();
        assert!(j.last_for_rule("r").unwrap().is_none());
        let a = j.record("r", &done("/a", "/b"), 0).unwrap();
        let b = j.record("r", &done("/c", "/d"), 0).unwrap();
        j.record("outra", &done("/e", "/f"), 0).unwrap();
        j.record_error("r", Kind::Move, Path::new("/g"), "x")
            .unwrap();
        assert_eq!(j.last_for_rule("r").unwrap().unwrap().id, b);
        j.mark_undone(b, true).unwrap();
        assert_eq!(j.last_for_rule("r").unwrap().unwrap().id, a);
    }

    #[test]
    fn max_id() {
        let (_d, j) = open();
        assert_eq!(j.max_id().unwrap(), 0);
        j.record("r", &done("/a", "/b"), 0).unwrap();
        assert_eq!(j.max_id().unwrap(), 1);
    }

    #[test]
    fn local_periods() {
        let now = SystemTime::now();
        let p = Periods::local(now);
        let n = now.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        assert!(p.today <= n && n - p.today < 86_400 + 3_600);
        assert!((23 * 3600..=25 * 3600).contains(&(p.today - p.yesterday)));
        assert!((6 * 86_400 - 3600..=6 * 86_400 + 3600).contains(&(p.today - p.week)));
    }
}
