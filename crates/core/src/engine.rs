use crate::{
    Result,
    actions::{self, Kind},
    config::{Config, Rule},
    downloads,
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

/// Download em andamento: pelo registro da extensão (até 24h) ou pela heurística
/// dos arquivos temporários do navegador.
pub fn busy(journal: &Journal, path: &Path, now: SystemTime) -> bool {
    let since = now - Duration::from_secs(downloads::STALE_SECS);
    stability::download_in_progress(path) || journal.download_active(path, since).unwrap_or(false)
}

/// `Facts::read` com origem e página de origem do registro quando os xattrs faltam
/// (sistema de arquivos sem xattr, ou o host não conseguiu gravá-los).
pub fn facts(journal: &Journal, path: &Path) -> Result<Facts> {
    let mut f = Facts::read(path)?;
    if f.origin_url.is_none() || f.referrer_url.is_none() {
        let row = match f.download_id {
            Some(id) => journal.download(id)?,
            None => journal.download_at(path)?,
        };
        if let Some(d) = row {
            f.origin_url = f.origin_url.or(d.url);
            f.referrer_url = f.referrer_url.or(d.referrer);
        }
    }
    Ok(f)
}

pub fn process(
    cfg: &Config,
    journal: &Journal,
    path: &Path,
    dry_run: bool,
    now: SystemTime,
) -> Result<Report> {
    let facts = facts(journal, path)?;
    if !dry_run {
        journal.ensure_download(&facts)?;
    }
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
    apply(rule, &facts, journal)
}

/// Organiza um arquivo escolhido pelo usuário (menu de contexto, "Aplicar agora"):
/// vale qualquer regra ativa, qualquer que seja o `watch`, sem idade mínima, e a
/// marca de desfeito é ignorada e removida (o pedido explícito vence o undo).
pub fn organize(cfg: &Config, journal: &Journal, path: &Path, now: SystemTime) -> Result<Report> {
    let facts = facts(journal, path)?;
    let Some(rule) = rules::first_match_anywhere(&cfg.rules, &facts, now) else {
        return Ok(Report::NoMatch);
    };
    let report = apply(rule, &facts, journal)?;
    if let Report::Done { done, .. } = &report {
        if let Some(dst) = &done.dst {
            actions::clear_keep(dst);
        }
        if done.kind == Kind::Copy {
            actions::clear_keep(&done.src);
        }
    }
    Ok(report)
}

/// Executa → grava no journal (sucesso ou erro).
fn apply(rule: &Rule, facts: &Facts, journal: &Journal) -> Result<Report> {
    let path = facts.path.as_path();
    let kind = actions::kind_of(&rule.action);
    match actions::execute(&rule.action, facts) {
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
            if fresh(&path, now) || busy(journal, &path, now) {
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
            "[[rule]]\nname = \"pdfs\"\nwatch = ['{}']\nwhen = {{ ext = [\"pdf\"] }}\naction = {{ move = '{}' }}\n",
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
            "[[rule]]\nname = \"bak\"\nwatch = ['{}']\nwhen = {{ ext = [\"pdf\"] }}\naction = {{ copy = '{}' }}\n",
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
        // A falha da ação vem primeiro (a mensagem do sistema muda por SO).
        assert!(!err.starts_with("journal quebrado"), "{err}");
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

    fn started(e: &Env, p: &Path, at: i64) {
        e.journal
            .download_started(&downloads::Started {
                browser: "firefox",
                browser_id: 1,
                path: p,
                url: "https://x.com/a.pdf",
                referrer: Some("https://site.com/page"),
                page_title: None,
                mime: None,
                started_at: at,
            })
            .unwrap();
    }

    #[test]
    fn scan_skips_registered_in_progress() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "1 KB, sem .part ao lado").unwrap();
        started(&e, &p, crate::journal::now());
        let later = SystemTime::now() + SCAN_MIN_AGE * 2;
        assert!(scan(&e.cfg, &e.journal, false, later).is_empty());
        assert!(p.exists());
        e.journal.download_finished("firefox", 1, 23).unwrap();
        assert_eq!(scan(&e.cfg, &e.journal, false, later).len(), 1);
        assert!(e.out.join("a.pdf").exists());
    }

    #[test]
    fn busy_ignores_stale_registry() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        let now = SystemTime::now();
        started(&e, &p, crate::journal::now() - 25 * 3600);
        assert!(!busy(&e.journal, &p, now));
        started(&e, &p, crate::journal::now());
        assert!(busy(&e.journal, &p, now));
    }

    #[test]
    fn facts_fall_back_to_registry() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        assert_eq!(facts(&e.journal, &p).unwrap().site(), None);
        started(&e, &p, crate::journal::now());
        let f = facts(&e.journal, &p).unwrap();
        assert_eq!(f.origin_host().as_deref(), Some("x.com"));
        assert_eq!(f.site().as_deref(), Some("site.com"));
    }

    fn row(e: &Env, p: &Path) -> Option<downloads::Download> {
        e.journal.download_at(p).unwrap()
    }

    #[test]
    fn process_registers_folder_file() {
        let e = env();
        let txt = e.watch.join("nota.txt");
        fs::write(&txt, "x").unwrap();
        let now = SystemTime::now();
        process(&e.cfg, &e.journal, &txt, true, now).unwrap();
        assert!(row(&e, &txt).is_none(), "dry-run não grava");
        process(&e.cfg, &e.journal, &txt, false, now).unwrap();
        process(&e.cfg, &e.journal, &txt, false, now).unwrap();
        let d = row(&e, &txt).unwrap();
        assert_eq!(d.source, downloads::Source::Folder);
        assert_eq!(d.state, downloads::DownloadState::Complete);
        let mtime = fs::metadata(&txt).unwrap().modified().unwrap();
        assert_eq!(
            d.started_at,
            mtime
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
        );
        let n: i64 = e
            .journal
            .conn
            .query_row("SELECT COUNT(*) FROM downloads", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn current_path_follows_move_and_undo() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        let Report::Done { id, done, .. } =
            process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap()
        else {
            panic!("esperava Done")
        };
        let dst = done.dst.unwrap();
        let d = row(&e, &dst).expect("registro segue o arquivo");
        assert_eq!(d.path, p);
        undo(&e.journal, Some(id)).unwrap();
        assert_eq!(row(&e, &p).unwrap().id, d.id);
        assert!(row(&e, &dst).is_none());
    }

    fn with_action(e: &Env, action: &str) -> Config {
        config::parse(&format!(
            "[[rule]]\nname = \"r\"\nwatch = ['{}']\nwhen = {{ ext = [\"pdf\"] }}\naction = {action}\n",
            e.watch.display()
        ))
        .unwrap()
    }

    #[test]
    fn trash_rename_and_copy_in_the_library() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();

        let copy = with_action(&e, &format!("{{ copy = '{}' }}", e.out.display()));
        process(&copy, &e.journal, &p, false, SystemTime::now()).unwrap();
        assert_eq!(
            row(&e, &p).unwrap().state,
            downloads::DownloadState::Complete
        );

        let rename = with_action(&e, "{ rename = \"b.{ext}\" }");
        let Report::Done { id, done, .. } =
            process(&rename, &e.journal, &p, false, SystemTime::now()).unwrap()
        else {
            panic!("esperava Done")
        };
        let b = done.dst.unwrap();
        assert!(row(&e, &b).is_some());
        undo(&e.journal, Some(id)).unwrap();
        assert!(row(&e, &p).is_some() && row(&e, &b).is_none());
        actions::clear_keep(&p);
        e.journal.conn.execute("DELETE FROM actions", []).unwrap();

        let trash = with_action(&e, "{ trash = true }");
        let Report::Done { id, .. } =
            process(&trash, &e.journal, &p, false, SystemTime::now()).unwrap()
        else {
            panic!("esperava Done")
        };
        assert_eq!(
            row(&e, &p).unwrap().state,
            downloads::DownloadState::Trashed
        );
        e.journal.sweep_gone().unwrap();
        assert_eq!(
            row(&e, &p).unwrap().state,
            downloads::DownloadState::Trashed,
            "lixeira não vira 'sumiu'"
        );
        undo(&e.journal, Some(id)).unwrap();
        assert_eq!(
            row(&e, &p).unwrap().state,
            downloads::DownloadState::Complete
        );
    }

    #[test]
    fn sweep_marks_missing_files_gone() {
        let e = env();
        let p = e.watch.join("nota.txt");
        fs::write(&p, "x").unwrap();
        process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap();
        fs::remove_file(&p).unwrap();
        e.journal.sweep_gone().unwrap();
        assert_eq!(row(&e, &p).unwrap().state, downloads::DownloadState::Gone);
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

    fn has_keep(p: &Path) -> bool {
        crate::meta::has(p, crate::meta::Tag::Keep)
    }

    fn xattr_supported(dir: &Path) -> bool {
        let p = dir.join(".probe");
        fs::write(&p, "").unwrap();
        let ok = crate::meta::set(&p, crate::meta::Tag::Keep, "1").is_ok();
        fs::remove_file(&p).unwrap();
        ok
    }

    #[test]
    fn organize_ignores_watch_and_min_age() {
        let e = env();
        let other = e.watch.parent().unwrap().join("outra");
        fs::create_dir(&other).unwrap();
        let p = other.join("a.pdf");
        fs::write(&p, "x").unwrap(); // recém-criado: o scan pularia
        let r = organize(&e.cfg, &e.journal, &p, SystemTime::now()).unwrap();
        assert!(matches!(r, Report::Done { .. }), "{r:?}");
        assert!(e.out.join("a.pdf").exists() && !p.exists());
        let entry = e.journal.recent(1).unwrap().remove(0);
        assert_eq!(entry.size, Some(1));
    }

    #[test]
    fn organize_overrides_keep() {
        let e = env();
        if !xattr_supported(&e.watch) {
            eprintln!("fs sem xattr user.*, pulando");
            return;
        }
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap();
        undo(&e.journal, None).unwrap();
        assert!(has_keep(&p));
        let r = organize(&e.cfg, &e.journal, &p, SystemTime::now()).unwrap();
        assert!(matches!(r, Report::Done { .. }), "{r:?}");
        let moved = e.out.join("a.pdf");
        assert!(moved.exists() && !has_keep(&moved));
    }

    #[test]
    fn organize_copy_clears_keep_on_both() {
        let d = tempfile::tempdir().unwrap();
        if !xattr_supported(d.path()) {
            return;
        }
        let out = d.path().join("out");
        let cfg = config::parse(&format!(
            "[[rule]]\nname = \"c\"\nwatch = [\"/nenhuma\"]\nwhen = {{ ext = [\"pdf\"] }}\naction = {{ copy = '{}' }}\n",
            out.display()
        ))
        .unwrap();
        let journal = Journal::open(&d.path().join("j.db")).unwrap();
        let p = d.path().join("a.pdf");
        fs::write(&p, "x").unwrap();
        crate::meta::set(&p, crate::meta::Tag::Keep, "1").unwrap();
        organize(&cfg, &journal, &p, SystemTime::now()).unwrap();
        assert!(!has_keep(&p) && !has_keep(&out.join("a.pdf")));
    }

    #[test]
    fn organize_skips_paused() {
        let mut e = env();
        e.cfg.rules[0].enabled = false;
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        let r = organize(&e.cfg, &e.journal, &p, SystemTime::now()).unwrap();
        assert!(matches!(r, Report::NoMatch), "{r:?}");
        assert!(p.exists());
    }
}
