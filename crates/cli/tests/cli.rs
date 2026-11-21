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
            "[settings]\nnotify = false\n\n[[rule]]\nname = \"pdfs\"\nwatch = ['{}']\nwhen = {{ ext = [\"pdf\"] }}\naction = {{ move = '{}' }}\n",
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
    c.env("TIDYFLOW_DATA_DIR", e.d.path().join("tidyflow"))
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

#[test]
fn organize_outside_watch() {
    let e = env();
    let other = e.d.path().join("outra");
    fs::create_dir(&other).unwrap();
    let p = other.join("a.pdf");
    fs::write(&p, "x").unwrap(); // recém-criado
    let o = run(&e, &["organize", p.to_str().unwrap()]);
    assert!(o.status.success(), "{o:?}");
    assert!(stdout(&o).starts_with("#1 [pdfs] move "), "{}", stdout(&o));
    assert!(e.out.join("a.pdf").exists() && !p.exists());

    let txt = other.join("b.txt");
    fs::write(&txt, "x").unwrap();
    let o = run(&e, &["organize", txt.to_str().unwrap()]);
    assert!(o.status.success());
    assert!(stdout(&o).contains("nenhuma regra casou"), "{}", stdout(&o));

    let o = run(
        &e,
        &["organize", other.join("nao-existe.pdf").to_str().unwrap()],
    );
    assert!(!o.status.success());
}

fn frame(v: &serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(v).unwrap();
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend(body);
    out
}

/// Envia mensagens ao `native-host` como o navegador faz e devolve as respostas.
fn native_host(e: &Env, input: &[u8], args: &[&str]) -> (Output, Vec<serde_json::Value>) {
    use std::io::Write;
    let mut child = cmd(e)
        .arg("native-host")
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let o = child.wait_with_output().unwrap();
    let mut replies = Vec::new();
    let mut rest = o.stdout.as_slice();
    while rest.len() >= 4 {
        let n = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        replies.push(serde_json::from_slice(&rest[4..4 + n]).unwrap());
        rest = &rest[4 + n..];
    }
    assert!(rest.is_empty(), "stdout com lixo fora do protocolo");
    (o, replies)
}

fn download_msg(kind: &str, path: &std::path::Path) -> serde_json::Value {
    serde_json::json!({
        "type": kind, "id": 1, "browser": "firefox", "path": path,
        "url": "https://cdn.example.com/a.pdf", "referrer": "https://example.com/p",
        "size": 1, "started_at": 1_800_000_000
    })
}

#[test]
fn native_host_speaks_the_protocol() {
    let e = env();
    let p = tidyflow_core::real_path(&e.watch.join("a.pdf"));
    fs::write(&p, "x").unwrap();
    let mut input = frame(&download_msg("started", &p));
    input.extend(4u32.to_le_bytes());
    input.extend(b"nope");
    input.extend(frame(&download_msg("finished", &p)));
    // Argumentos que o Firefox passa ao host.
    let (o, replies) = native_host(
        &e,
        &input,
        &[
            "/x/io.github.langendolff.tidyflow.json",
            "tidyflow@langendolff.github.io",
        ],
    );
    assert!(o.status.success(), "{o:?}");
    assert_eq!(replies.len(), 3);
    assert_eq!(replies[0], serde_json::json!({ "ok": true }));
    assert_eq!(replies[1]["ok"], false);
    assert_eq!(replies[2], serde_json::json!({ "ok": true }));

    let j = tidyflow_core::journal::Journal::open(&e.d.path().join("tidyflow/journal.db")).unwrap();
    let d = j.download_at(&p).unwrap().unwrap();
    assert_eq!(d.state, tidyflow_core::downloads::DownloadState::Complete);

    // Origem do Chromium também é aceita como argumento.
    let (o, replies) = native_host(&e, &[], &["chrome-extension://abc/"]);
    assert!(o.status.success(), "{o:?}");
    assert!(replies.is_empty());
}

#[test]
fn daemon_waits_for_the_browser_to_finish() {
    let e = env();
    let p = e.watch.join("a.pdf");
    let probe = e.d.path().join("probe");
    fs::write(&probe, "x").unwrap();
    if tidyflow_core::meta::set(&probe, tidyflow_core::meta::Tag::Keep, "1").is_err() {
        return; // fs sem xattr user.*: o evento de fim não chega
    }
    let mut child = cmd(&e).arg("daemon").spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let (o, _) = native_host(&e, &frame(&download_msg("started", &p)), &[]);
    assert!(o.status.success(), "{o:?}");
    // Sem .part ao lado: só o registro diz que ainda está baixando.
    fs::write(&p, "x").unwrap();
    std::thread::sleep(std::time::Duration::from_secs(4));
    let still_there = p.exists();
    let (o, _) = native_host(&e, &frame(&download_msg("finished", &p)), &[]);
    assert!(o.status.success(), "{o:?}");
    wait_for(10, || e.out.join("a.pdf").exists());
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(still_there, "daemon moveu um download em andamento");
    assert!(
        e.out.join("a.pdf").exists(),
        "daemon não reagiu ao fim do download"
    );
}
