//! O daemon em segundo plano em cada sistema:
//!
//! - Linux: unit `tidyflow.service` do systemd do usuário.
//! - macOS: LaunchAgent `io.github.tidyflow.daemon`.
//! - Windows: valor `TidyFlow` em `HKCU\…\CurrentVersion\Run` (sobe no login).
//!
//! Em todos, o daemon segura `daemon.lock` na pasta de dados enquanto roda: impede
//! dois daemons e, fora do Linux, é como o app sabe se ele está ativo.

use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, UNIX_EPOCH},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub active: bool,
    /// unix, quando o daemon começou
    pub since: Option<i64>,
}

pub const UNIT: &str = "tidyflow";
pub const LAUNCHD_LABEL: &str = "io.github.tidyflow.daemon";
pub const RUN_VALUE: &str = "TidyFlow";

pub fn lock_path() -> PathBuf {
    crate::data_dir().join("daemon.lock")
}

fn pid_path() -> PathBuf {
    crate::data_dir().join("daemon.pid")
}

/// Trava do daemon; solte-a fechando o arquivo. Espera um pouco: o app pode estar
/// conferindo a trava no mesmo instante.
pub fn hold_lock() -> crate::Result<File> {
    let path = lock_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let f = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match f.try_lock() {
            Ok(()) => break,
            Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(fs::TryLockError::WouldBlock) => {
                return Err("já existe um daemon do TidyFlow rodando".into());
            }
            Err(fs::TryLockError::Error(e)) => return Err(e.into()),
        }
    }
    // Regravar atualiza o mtime: é o "desde quando" do status.
    f.set_len(0)?;
    (&f).write_all(std::process::id().to_string().as_bytes())?;
    // O Windows não deixa ler o arquivo travado; o pid vai também à parte.
    let _ = fs::write(pid_path(), std::process::id().to_string());
    Ok(f)
}

/// Pela trava: ativo se outro processo a segura.
pub fn lock_status() -> Status {
    let path = lock_path();
    let Ok(f) = File::open(&path) else {
        return Status {
            active: false,
            since: None,
        };
    };
    let active = matches!(f.try_lock_shared(), Err(fs::TryLockError::WouldBlock));
    let since = fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    Status {
        active,
        since: since.filter(|_| active),
    }
}

/// Executável do daemon: ao lado de quem chama (`tidyflowd.exe` no Windows, para não
/// abrir console; `tidyflow` nos outros), senão pelo PATH.
pub fn daemon_exe() -> PathBuf {
    let name = if cfg!(windows) {
        "tidyflowd.exe"
    } else {
        "tidyflow"
    };
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(name)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn run(cmd: &mut Command) -> Result<String, String> {
    let shown = format!("{cmd:?}");
    let o = cmd
        .output()
        .map_err(|e| format!("não foi possível rodar {shown}: {e}"))?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    } else {
        Err(format!(
            "{shown} falhou: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ))
    }
}

// ---------------------------------------------------------------- Linux

#[cfg(not(any(target_os = "macos", windows)))]
pub fn status() -> Status {
    let out = Command::new("systemctl")
        .args(["--user", "show", UNIT, "--timestamp=unix"])
        .args(["-p", "ActiveState", "-p", "ActiveEnterTimestamp"])
        .output();
    match out {
        Ok(o) if o.status.success() => parse_show(&String::from_utf8_lossy(&o.stdout)),
        // Sem systemd (container, WSL): o daemon pode estar rodando à mão.
        _ => lock_status(),
    }
}

#[cfg_attr(any(target_os = "macos", windows), allow(dead_code))]
fn parse_show(out: &str) -> Status {
    let mut active = false;
    let mut since = None;
    for line in out.lines() {
        match line.split_once('=') {
            Some(("ActiveState", v)) => active = v.trim() == "active",
            Some(("ActiveEnterTimestamp", v)) => {
                since = v.trim().strip_prefix('@').and_then(|n| n.parse().ok())
            }
            _ => {}
        }
    }
    Status {
        active,
        since: since.filter(|_| active),
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn start() -> Result<(), String> {
    run(Command::new("systemctl").args(["--user", "start", UNIT])).map(drop)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn unit_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(crate::home()).join(".config"))
        .join("systemd/user/tidyflow.service")
}

#[cfg_attr(any(target_os = "macos", windows), allow(dead_code))]
fn unit(exe: &Path) -> String {
    format!(
        "[Unit]\nDescription=TidyFlow: organizador automático de arquivos\n\n[Service]\nExecStart=\"{}\" daemon\nRestart=on-failure\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n",
        exe.display()
    )
}

/// Instala o serviço, liga no login e inicia agora. Devolve o arquivo gravado.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn enable(exe: &Path) -> Result<PathBuf, String> {
    let path = unit_path();
    write_file(&path, &unit(exe))?;
    run(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
    run(Command::new("systemctl").args(["--user", "enable", "--now", UNIT]))?;
    Ok(path)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn disable() -> Result<(), String> {
    run(Command::new("systemctl").args(["--user", "disable", "--now", UNIT])).map(drop)
}

#[cfg_attr(windows, allow(dead_code))]
fn write_file(path: &Path, body: &str) -> Result<(), String> {
    let err = |e: io::Error| format!("não foi possível gravar {}: {e}", path.display());
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(err)?;
    }
    fs::write(path, body).map_err(err)
}

// ---------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
pub fn status() -> Status {
    lock_status()
}

#[cfg(target_os = "macos")]
fn domain() -> Result<String, String> {
    let uid = run(Command::new("id").arg("-u"))?;
    Ok(format!("gui/{}", uid.trim()))
}

#[cfg(target_os = "macos")]
fn plist_path() -> PathBuf {
    PathBuf::from(crate::home()).join(format!("Library/LaunchAgents/{LAUNCHD_LABEL}.plist"))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn launch_agent(exe: &Path, log: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>daemon</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>ProcessType</key>
  <string>Background</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
</dict>
</plist>
"#,
        xml_escape(&exe.to_string_lossy()),
        xml_escape(&log.to_string_lossy())
    )
}

#[cfg(target_os = "macos")]
pub fn start() -> Result<(), String> {
    if !plist_path().is_file() {
        return Err("o serviço não está instalado; rode: tidyflow service enable".into());
    }
    let target = format!("{}/{LAUNCHD_LABEL}", domain()?);
    if run(Command::new("launchctl").args(["kickstart", &target])).is_err() {
        // Desligado com `bootout` (ou nunca carregado nesta sessão).
        let plist = plist_path();
        run(Command::new("launchctl")
            .args(["bootstrap", &domain()?])
            .arg(&plist))?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn enable(exe: &Path) -> Result<PathBuf, String> {
    let path = plist_path();
    let log = crate::data_dir().join("daemon.log");
    let _ = fs::create_dir_all(crate::data_dir());
    write_file(&path, &launch_agent(exe, &log))?;
    let d = domain()?;
    // Recarrega se já estava instalado (o executável pode ter mudado).
    let _ = run(Command::new("launchctl").args(["bootout", &format!("{d}/{LAUNCHD_LABEL}")]));
    run(Command::new("launchctl").args(["bootstrap", &d]).arg(&path))?;
    Ok(path)
}

#[cfg(target_os = "macos")]
pub fn disable() -> Result<(), String> {
    let d = domain()?;
    let _ = run(Command::new("launchctl").args(["bootout", &format!("{d}/{LAUNCHD_LABEL}")]));
    match fs::remove_file(plist_path()) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg_attr(not(windows), allow(dead_code))]
fn run_command(exe: &Path) -> String {
    format!("\"{}\" daemon", exe.display())
}

#[cfg(windows)]
pub fn status() -> Status {
    lock_status()
}

#[cfg(windows)]
pub fn start() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let exe = daemon_exe();
    Command::new(&exe)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
        .spawn()
        .map(drop)
        .map_err(|e| format!("não foi possível iniciar {}: {e}", exe.display()))
}

#[cfg(windows)]
pub fn enable(exe: &Path) -> Result<PathBuf, String> {
    let (key, _) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .create_subkey(RUN_KEY)
        .map_err(|e| format!("não foi possível abrir o registro: {e}"))?;
    key.set_value(RUN_VALUE, &run_command(exe))
        .map_err(|e| format!("não foi possível gravar no registro: {e}"))?;
    if !lock_status().active {
        start()?;
    }
    Ok(PathBuf::from(format!(
        r"HKEY_CURRENT_USER\{RUN_KEY}\{RUN_VALUE}"
    )))
}

#[cfg(windows)]
pub fn disable() -> Result<(), String> {
    if let Ok(key) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, winreg::enums::KEY_SET_VALUE)
    {
        let _ = key.delete_value(RUN_VALUE);
    }
    if lock_status().active
        && let Ok(pid) = fs::read_to_string(pid_path())
    {
        run(Command::new("taskkill").args(["/PID", pid.trim(), "/F"]))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_systemctl_show() {
        let s = parse_show("ActiveState=active\nActiveEnterTimestamp=@1763251200\n");
        assert_eq!(
            s,
            Status {
                active: true,
                since: Some(1_763_251_200),
            }
        );
        let s = parse_show("ActiveState=inactive\nActiveEnterTimestamp=\n");
        assert!(!s.active && s.since.is_none());
    }

    #[test]
    fn service_files() {
        let u = unit(Path::new("/opt/Tidy Flow/tidyflow"));
        assert!(
            u.contains("ExecStart=\"/opt/Tidy Flow/tidyflow\" daemon\n"),
            "{u}"
        );
        let p = launch_agent(Path::new("/Apps/A&B/tidyflow"), Path::new("/l/daemon.log"));
        assert!(p.contains("<string>/Apps/A&amp;B/tidyflow</string>"), "{p}");
        assert!(p.contains("<string>io.github.tidyflow.daemon</string>"));
        assert_eq!(
            run_command(Path::new(r"C:\P\tidyflowd.exe")),
            r#""C:\P\tidyflowd.exe" daemon"#
        );
    }

    #[test]
    fn lock_tells_if_daemon_runs() {
        let d = tempfile::tempdir().unwrap();
        // SAFETY: os testes do core não leem TIDYFLOW_DATA_DIR em paralelo.
        unsafe { std::env::set_var("TIDYFLOW_DATA_DIR", d.path()) };
        assert!(!lock_status().active);
        let held = hold_lock().unwrap();
        let s = lock_status();
        assert!(s.active && s.since.is_some(), "{s:?}");
        drop(held);
        assert!(!lock_status().active);
        unsafe { std::env::remove_var("TIDYFLOW_DATA_DIR") };
    }
}
