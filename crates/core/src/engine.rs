use crate::{
    Result,
    actions::{self, Kind},
    config::Config,
    facts::Facts,
    journal::{Entry, Journal},
    rules, stability,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// Arquivos mais novos que isso ficam para o evento CLOSE_WRITE (que espera estabilizar).
const SCAN_MIN_AGE: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub enum Report {
    NoMatch,
    Planned {
        rule: String,
        kind: Kind,
        dst: Option<PathBuf>,
    },
    Done {
        id: i64,
        rule: String,
        done: actions::Done,
    },
    Failed {
        rule: String,
        error: String,
    },
}

pub fn process(
    cfg: &Config,
    journal: &Journal,
    path: &Path,
    dry_run: bool,
    now: SystemTime,
) -> Result<Report> {
    let facts = Facts::read(path)?;
    if facts.keep || journal.was_undone_src(path)? {
        return Ok(Report::NoMatch); // restaurado por undo
    }
    let Some(rule) = rules::first_match(&cfg.rules, &facts, now) else {
        return Ok(Report::NoMatch);
    };
    let kind = actions::kind_of(&rule.action);
    if kind == Kind::Copy && journal.was_copied_since(path, facts.modified)? {
        return Ok(Report::NoMatch); // já copiado e a origem não mudou
    }
    if dry_run {
        return Ok(Report::Planned {
            rule: rule.name.clone(),
            kind,
            dst: actions::destination(&rule.action, &facts),
        });
    }
    match actions::execute(&rule.action, &facts) {
        Ok(Some(done)) => {
            let id = journal.record(&rule.name, &done, facts.size)?;
            Ok(Report::Done {
                id,
                rule: rule.name.clone(),
                done,
            })
        }
        Ok(None) => Ok(Report::NoMatch),
        Err(e) => {
            let error = e.to_string();
            if let Err(je) = journal.record_error(&rule.name, kind, path, &error) {
                return Err(format!("{error} (e falhou ao gravar no journal: {je})").into());
            }
            Ok(Report::Failed {
                rule: rule.name.clone(),
                error,
            })
        }
    }
}

pub fn scan(
    cfg: &Config,
    journal: &Journal,
    dry_run: bool,
    now: SystemTime,
) -> Vec<(PathBuf, Result<Report>)> {
    let mut dirs: Vec<&PathBuf> = cfg.rules.iter().flat_map(|r| &r.watch).collect();
    dirs.sort();
    dirs.dedup();
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || stability::is_ignored_name(&entry.file_name().to_string_lossy()) {
                continue;
            }
            if fresh(&path, now) {
                continue;
            }
            let report = process(cfg, journal, &path, dry_run, now);
            out.push((path, report));
        }
    }
    out
}

fn fresh(path: &Path, now: SystemTime) -> bool {
    let age = fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|m| now.duration_since(m).unwrap_or_default());
    age.is_ok_and(|a| a < SCAN_MIN_AGE)
}

pub fn undo(journal: &Journal, id: Option<i64>) -> Result<Entry> {
    let entry = match id {
        Some(id) => journal.get(id)?.ok_or(format!("ação #{id} não existe"))?,
        None => journal.last_undoable()?.ok_or("nada para desfazer")?,
    };
    if entry.undone {
        return Err(format!("ação #{} já foi desfeita", entry.id).into());
    }
    if entry.error.is_some() {
        return Err(format!("ação #{} falhou, não há o que desfazer", entry.id).into());
    }
    let marked = actions::undo(entry.kind, &entry.src, entry.dst.as_deref())?;
    journal.mark_undone(entry.id, marked)?;
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use std::fs;

    struct Env {
        _d: tempfile::TempDir,
        watch: PathBuf,
        out: PathBuf,
        cfg: Config,
        journal: Journal,
    }

    fn env() -> Env {
        let d = tempfile::tempdir().unwrap();
        let watch = d.path().join("in");
        let out = d.path().join("out");
        fs::create_dir(&watch).unwrap();
        let cfg = config::parse(&format!(
            "[[rule]]\nname = \"pdfs\"\nwatch = [\"{}\"]\nwhen = {{ ext = [\"pdf\"] }}\naction = {{ move = \"{}\" }}\n",
            watch.display(),
            out.display()
        ))
        .unwrap();
        let journal = Journal::open(&d.path().join("j.db")).unwrap();
        Env {
            _d: d,
            watch,
            out,
            cfg,
            journal,
        }
    }

    #[test]
    fn process_moves_and_records() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        let Report::Done { id, rule, done } =
            process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap()
        else {
            panic!("esperava Done");
        };
        assert_eq!(rule, "pdfs");
        assert_eq!(done.dst.as_deref(), Some(e.out.join("a.pdf").as_path()));
        assert_eq!(e.journal.get(id).unwrap().unwrap().src, p);
    }

    #[test]
    fn undone_file_is_left_alone() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap();
        undo(&e.journal, None).unwrap();
        assert!(p.exists());
        for dry in [false, true] {
            let r = process(&e.cfg, &e.journal, &p, dry, SystemTime::now()).unwrap();
            assert!(matches!(r, Report::NoMatch), "{r:?}");
        }
        assert!(p.exists() && !e.out.join("a.pdf").exists());
    }

    #[test]
    fn undone_file_is_left_alone_when_xattr_failed() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        let Report::Done { id, done, .. } =
            process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap()
        else {
            panic!("esperava Done");
        };
        // Simula undo em fs sem xattr: restaura à mão e registra sem marca.
        fs::rename(done.dst.unwrap(), &p).unwrap();
        e.journal.mark_undone(id, false).unwrap();
        let r = process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap();
        assert!(matches!(r, Report::NoMatch), "{r:?}");
        assert!(p.exists() && !e.out.join("a.pdf").exists());
    }

    #[test]
    fn new_file_at_same_path_after_marked_undo_is_processed() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap();
        undo(&e.journal, None).unwrap();
        fs::remove_file(&p).unwrap();
        fs::write(&p, "novo").unwrap(); // arquivo novo, sem xattr
        let r = process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap();
        assert!(matches!(r, Report::Done { .. }), "{r:?}");
    }

    #[test]
    fn copy_rule_copies_once_until_source_changes() {
        let e = env();
        let cfg = config::parse(&format!(
            "[[rule]]\nname = \"bak\"\nwatch = [\"{}\"]\nwhen = {{ ext = [\"pdf\"] }}\naction = {{ copy = \"{}\" }}\n",
            e.watch.display(),
            e.out.display()
        ))
        .unwrap();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        let now = SystemTime::now();
        let r = process(&cfg, &e.journal, &p, false, now).unwrap();
        assert!(matches!(r, Report::Done { .. }), "{r:?}");
        let r = process(&cfg, &e.journal, &p, false, now).unwrap();
        assert!(matches!(r, Report::NoMatch), "{r:?}");
        assert_eq!(fs::read_dir(&e.out).unwrap().count(), 1);
        // Download novo no mesmo caminho: mtime mais recente.
        fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(now + std::time::Duration::from_secs(60))
            .unwrap();
        let r = process(&cfg, &e.journal, &p, false, now).unwrap();
        assert!(matches!(r, Report::Done { .. }), "{r:?}");
        assert_eq!(fs::read_dir(&e.out).unwrap().count(), 2);
    }

    #[test]
    fn dry_run_changes_nothing() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        let r = process(&e.cfg, &e.journal, &p, true, SystemTime::now()).unwrap();
        assert!(matches!(
            r,
            Report::Planned {
                kind: Kind::Move,
                ..
            }
        ));
        assert!(p.exists());
        assert!(e.journal.recent(10).unwrap().is_empty());
    }

    #[test]
    fn no_match() {
        let e = env();
        let p = e.watch.join("a.txt");
        fs::write(&p, "x").unwrap();
        assert!(matches!(
            process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap(),
            Report::NoMatch
        ));
    }

    #[test]
    fn failure_is_reported_and_recorded() {
        let e = env();
        fs::write(&e.out, "sou um arquivo, não uma pasta").unwrap();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        let r = process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap();
        assert!(matches!(r, Report::Failed { .. }), "{r:?}");
        assert!(p.exists());
        assert!(e.journal.recent(1).unwrap()[0].error.is_some());
    }

    #[test]
    fn journal_failure_after_action_error_keeps_both_errors() {
        let e = env();
        fs::write(&e.out, "sou um arquivo, não uma pasta").unwrap();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        rusqlite::Connection::open(e._d.path().join("j.db"))
            .unwrap()
            .execute_batch("CREATE TRIGGER t BEFORE INSERT ON actions BEGIN SELECT RAISE(ABORT, \"journal quebrado\"); END")
            .unwrap();
        let err = process(&e.cfg, &e.journal, &p, false, SystemTime::now())
            .unwrap_err()
            .to_string();
        assert!(err.contains("journal quebrado"), "{err}");
        assert!(err.contains("File exists"), "{err}");
    }

    #[test]
    fn scan_skips_ignored_and_dirs() {
        let e = env();
        fs::write(e.watch.join("a.pdf"), "x").unwrap();
        fs::write(e.watch.join("b.pdf.part"), "x").unwrap();
        fs::write(e.watch.join(".c.pdf"), "x").unwrap();
        fs::create_dir(e.watch.join("d.pdf")).unwrap();
        let results = scan(
            &e.cfg,
            &e.journal,
            false,
            SystemTime::now() + SCAN_MIN_AGE * 2,
        );
        assert_eq!(results.len(), 1);
        assert!(e.out.join("a.pdf").exists());
    }

    #[test]
    fn scan_skips_fresh_files() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        let now = SystemTime::now();
        assert!(scan(&e.cfg, &e.journal, false, now).is_empty());
        assert!(p.exists());
        assert_eq!(
            scan(&e.cfg, &e.journal, false, now + SCAN_MIN_AGE * 2).len(),
            1
        );
    }

    #[test]
    fn undo_last_and_twice() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap();
        let entry = undo(&e.journal, None).unwrap();
        assert!(p.exists());
        assert!(
            undo(&e.journal, Some(entry.id))
                .unwrap_err()
                .to_string()
                .contains("já foi desfeita")
        );
        assert!(
            undo(&e.journal, None)
                .unwrap_err()
                .to_string()
                .contains("nada para desfazer")
        );
        assert!(
            undo(&e.journal, Some(42))
                .unwrap_err()
                .to_string()
                .contains("não existe")
        );
    }
}
