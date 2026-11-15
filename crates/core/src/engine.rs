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
    time::SystemTime,
};

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
    let Some(rule) = rules::first_match(&cfg.rules, &facts, now) else {
        return Ok(Report::NoMatch);
    };
    let kind = actions::kind_of(&rule.action);
    if dry_run {
        return Ok(Report::Planned {
            rule: rule.name.clone(),
            kind,
            dst: actions::destination(&rule.action, &facts),
        });
    }
    match actions::execute(&rule.action, &facts) {
        Ok(Some(done)) => {
            let id = journal.record(&rule.name, &done)?;
            Ok(Report::Done {
                id,
                rule: rule.name.clone(),
                done,
            })
        }
        Ok(None) => Ok(Report::NoMatch),
        Err(e) => {
            let error = e.to_string();
            journal.record_error(&rule.name, kind, path, &error)?;
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
            let report = process(cfg, journal, &path, dry_run, now);
            out.push((path, report));
        }
    }
    out
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
    actions::undo(entry.kind, &entry.src, entry.dst.as_deref())?;
    journal.mark_undone(entry.id)?;
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
    fn scan_skips_ignored_and_dirs() {
        let e = env();
        fs::write(e.watch.join("a.pdf"), "x").unwrap();
        fs::write(e.watch.join("b.pdf.part"), "x").unwrap();
        fs::write(e.watch.join(".c.pdf"), "x").unwrap();
        fs::create_dir(e.watch.join("d.pdf")).unwrap();
        let results = scan(&e.cfg, &e.journal, false, SystemTime::now());
        assert_eq!(results.len(), 1);
        assert!(e.out.join("a.pdf").exists());
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
