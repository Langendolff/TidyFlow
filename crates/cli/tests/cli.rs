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
    fs::write(e.watch.join("a.pdf"), "x").unwrap();
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
    fs::write(e.watch.join("a.pdf"), "x").unwrap();
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
