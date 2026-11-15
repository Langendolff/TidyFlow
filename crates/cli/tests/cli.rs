use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Env {
    d: tempfile::TempDir,
    watch: PathBuf,
    out: PathBuf,
    cfg: PathBuf,
}

fn env() -> Env {
    let d = tempfile::tempdir().unwrap();
    let watch = d.path().join("in");
    let out = d.path().join("out");
    fs::create_dir(&watch).unwrap();
    let cfg = d.path().join("config.toml");
    fs::write(
        &cfg,
        format!(
            "[settings]\nnotify = false\n\n[[rule]]\nname = \"pdfs\"\nwatch = [\"{}\"]\nwhen = {{ ext = [\"pdf\"] }}\naction = {{ move = \"{}\" }}\n",
            watch.display(),
            out.display()
        ),
    )
    .unwrap();
    Env { d, watch, out, cfg }
}

/// Varreduras ignoram arquivos com menos de 10s; envelhece o mtime.
fn write_old(p: &std::path::Path) {
    fs::write(p, "x").unwrap();
    fs::File::options()
        .write(true)
        .open(p)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
}

fn cmd(e: &Env) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_tidyflow"));
    c.env("XDG_DATA_HOME", e.d.path())
        .arg("--config")
        .arg(&e.cfg);
    c
}

fn run(e: &Env, args: &[&str]) -> Output {
    cmd(e).args(args).output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn run_then_undo() {
    let e = env();
    write_old(&e.watch.join("a.pdf"));
    let o = run(&e, &["run"]);
    assert!(o.status.success(), "{o:?}");
    assert!(e.out.join("a.pdf").exists());

    let o = run(&e, &["history"]);
    assert!(stdout(&o).contains("a.pdf"), "{}", stdout(&o));

    let o = run(&e, &["undo"]);
    assert!(o.status.success(), "{o:?}");
    assert!(e.watch.join("a.pdf").exists() && !e.out.join("a.pdf").exists());
}

#[test]
fn dry_run_changes_nothing() {
    let e = env();
    write_old(&e.watch.join("a.pdf"));
    let o = run(&e, &["run", "--dry-run"]);
    assert!(stdout(&o).contains("a.pdf"));
    assert!(e.watch.join("a.pdf").exists());
}

#[test]
fn check_shows_matching_rule() {
    let e = env();
    let f = e.watch.join("a.pdf");
    fs::write(&f, "x").unwrap();
    let o = run(&e, &["check", f.to_str().unwrap()]);
    assert!(o.status.success());
    assert!(stdout(&o).contains("pdfs"), "{}", stdout(&o));
}

#[test]
fn check_reports_bad_config() {
    let e = env();
    fs::write(&e.cfg, "[settings]\nnotify = \n").unwrap();
    let o = run(&e, &["check"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("linha 2"));
}

#[test]
fn daemon_organizes_new_and_existing_files() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let e = env();
    write_old(&e.watch.join("antes.pdf")); // varredura inicial
    let mut child = cmd(&e).arg("daemon").spawn().unwrap();
    thread::sleep(Duration::from_millis(500));
    fs::write(e.watch.join("depois.pdf"), "x").unwrap(); // evento inotify

    let wanted = [e.out.join("antes.pdf"), e.out.join("depois.pdf")];
    let deadline = Instant::now() + Duration::from_secs(10);
    while !wanted.iter().all(|p| p.exists()) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    for p in &wanted {
        assert!(p.exists(), "{} não chegou", p.display());
    }
}

fn wait_for(deadline_secs: u64, f: impl Fn() -> bool) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(deadline_secs);
    while !f() && std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn daemon_does_not_redo_undone_file() {
    let e = env();
    let mut child = cmd(&e).arg("daemon").spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    fs::write(e.watch.join("a.pdf"), "x").unwrap();
    wait_for(10, || e.out.join("a.pdf").exists());
    let o = run(&e, &["undo"]);
    std::thread::sleep(std::time::Duration::from_secs(4));
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(o.status.success(), "{o:?}");
    assert!(e.watch.join("a.pdf").exists(), "a.pdf não voltou");
    assert!(!e.out.join("a.pdf").exists(), "daemon refez a ação");
}

#[test]
fn daemon_organizes_file_renamed_into_watch_dir() {
    let e = env();
    let mut child = cmd(&e).arg("daemon").spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let outside = e.d.path().join("b.pdf");
    fs::write(&outside, "x").unwrap();
    fs::rename(&outside, e.watch.join("b.pdf")).unwrap();
    wait_for(10, || e.out.join("b.pdf").exists());
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(e.out.join("b.pdf").exists());
}

#[test]
fn check_warns_about_missing_watch_dir() {
    let e = env();
    fs::remove_dir(&e.watch).unwrap();
    let o = run(&e, &["check"]);
    assert!(o.status.success(), "{o:?}");
    assert!(stdout(&o).contains("aviso"), "{}", stdout(&o));
}
