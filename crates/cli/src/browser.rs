//! Instala o host de native messaging para os navegadores.
//!
//! O manifest aponta direto para o executável: o navegador o abre com argumentos
//! próprios (manifest + ID no Firefox, origem no Chromium) e `main` reconhece isso.
//! - Linux e macOS: um manifest na pasta de cada navegador encontrado na home.
//! - Windows: manifests na pasta de dados e uma chave por navegador no Registro.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const HOST_NAME: &str = "io.github.langendolff.tidyflow";
pub const FIREFOX_ID: &str = "tidyflow@langendolff.github.io";
/// Derivado da `key` em `browser/manifest.base.json` (o teste do build confere).
pub const CHROMIUM_ID: &str = "ofmjjmckchphahpaihfcionnajfllndg";

#[derive(Clone, Copy)]
enum Family {
    Gecko,
    Chromium,
}

fn manifest(family: Family, exe: &Path) -> serde_json::Value {
    let mut m = serde_json::json!({
        "name": HOST_NAME,
        "description": "TidyFlow: registra downloads para organizar os arquivos",
        "path": exe,
        "type": "stdio",
    });
    match family {
        Family::Gecko => m["allowed_extensions"] = serde_json::json!([FIREFOX_ID]),
        Family::Chromium => {
            m["allowed_origins"] = serde_json::json!([format!("chrome-extension://{CHROMIUM_ID}/")])
        }
    }
    m
}

fn manifest_name() -> String {
    format!("{HOST_NAME}.json")
}

fn write_manifest(file: &Path, family: Family, exe: &Path) -> io::Result<()> {
    fs::create_dir_all(file.parent().expect("caminho com pasta"))?;
    fs::write(
        file,
        serde_json::to_string_pretty(&manifest(family, exe))? + "\n",
    )
}

fn remove(file: &Path, removed: &mut Vec<PathBuf>) -> io::Result<()> {
    match fs::remove_file(file) {
        Ok(()) => removed.push(file.to_path_buf()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    Ok(())
}

// ---------------------------------------------------------------- Linux e macOS

/// (pasta que indica o navegador instalado, pasta dos manifests, família), relativas à home
#[cfg(all(unix, not(target_os = "macos")))]
const BROWSERS: &[(&str, &str, Family)] = &[
    (".mozilla", ".mozilla/native-messaging-hosts", Family::Gecko),
    (
        ".librewolf",
        ".librewolf/native-messaging-hosts",
        Family::Gecko,
    ),
    (
        ".config/chromium",
        ".config/chromium/NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        ".config/google-chrome",
        ".config/google-chrome/NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        ".config/BraveSoftware/Brave-Browser",
        ".config/BraveSoftware/Brave-Browser/NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        ".config/microsoft-edge",
        ".config/microsoft-edge/NativeMessagingHosts",
        Family::Chromium,
    ),
];

#[cfg(target_os = "macos")]
const BROWSERS: &[(&str, &str, Family)] = &[
    (
        "Library/Application Support/Firefox",
        "Library/Application Support/Mozilla/NativeMessagingHosts",
        Family::Gecko,
    ),
    (
        "Library/Application Support/LibreWolf",
        "Library/Application Support/LibreWolf/NativeMessagingHosts",
        Family::Gecko,
    ),
    (
        "Library/Application Support/Google/Chrome",
        "Library/Application Support/Google/Chrome/NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        "Library/Application Support/Chromium",
        "Library/Application Support/Chromium/NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        "Library/Application Support/BraveSoftware/Brave-Browser",
        "Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        "Library/Application Support/Microsoft Edge",
        "Library/Application Support/Microsoft Edge/NativeMessagingHosts",
        Family::Chromium,
    ),
];

/// Versões até a 0.1 apontavam o manifest para este script.
#[cfg(unix)]
fn legacy_script(home: &Path) -> PathBuf {
    home.join(".local/share/tidyflow/native-host")
}

#[cfg(unix)]
pub fn install(home: &Path, exe: &Path) -> io::Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for (probe, dir, family) in BROWSERS {
        if !home.join(probe).is_dir() {
            continue;
        }
        let file = home.join(dir).join(manifest_name());
        write_manifest(&file, *family, exe)?;
        written.push(file);
    }
    remove(&legacy_script(home), &mut Vec::new())?;
    Ok(written)
}

#[cfg(unix)]
pub fn uninstall(home: &Path) -> io::Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for (_, dir, _) in BROWSERS {
        remove(&home.join(dir).join(manifest_name()), &mut removed)?;
    }
    remove(&legacy_script(home), &mut removed)?;
    Ok(removed)
}

// ---------------------------------------------------------------- Windows

/// Chaves em `HKEY_CURRENT_USER` (sem o nome do host) e a família de cada uma.
#[cfg_attr(not(windows), allow(dead_code))]
const REGISTRY: &[(&str, Family)] = &[
    (r"Software\Mozilla\NativeMessagingHosts", Family::Gecko),
    (
        r"Software\Google\Chrome\NativeMessagingHosts",
        Family::Chromium,
    ),
    (r"Software\Chromium\NativeMessagingHosts", Family::Chromium),
    (
        r"Software\Microsoft\Edge\NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
        Family::Chromium,
    ),
];

#[cfg_attr(not(windows), allow(dead_code))]
fn windows_manifest(dir: &Path, family: Family) -> PathBuf {
    dir.join(match family {
        Family::Gecko => "firefox.json",
        Family::Chromium => "chromium.json",
    })
}

/// `home` é ignorada: no Windows os manifests ficam na pasta de dados.
#[cfg(windows)]
pub fn install(_home: &Path, exe: &Path) -> io::Result<Vec<PathBuf>> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let dir = tidyflow_core::data_dir().join("native-host");
    let mut written = Vec::new();
    for family in [Family::Gecko, Family::Chromium] {
        let file = windows_manifest(&dir, family);
        write_manifest(&file, family, exe)?;
        written.push(file);
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for (key, family) in REGISTRY {
        let path = format!(r"{key}\{HOST_NAME}");
        let (k, _) = hkcu.create_subkey(&path)?;
        k.set_value(
            "",
            &windows_manifest(&dir, *family)
                .to_string_lossy()
                .into_owned(),
        )?;
        written.push(PathBuf::from(format!(r"HKEY_CURRENT_USER\{path}")));
    }
    Ok(written)
}

#[cfg(windows)]
pub fn uninstall(_home: &Path) -> io::Result<Vec<PathBuf>> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let dir = tidyflow_core::data_dir().join("native-host");
    let mut removed = Vec::new();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for (key, _) in REGISTRY {
        let path = format!(r"{key}\{HOST_NAME}");
        match hkcu.delete_subkey_all(&path) {
            Ok(()) => removed.push(PathBuf::from(format!(r"HKEY_CURRENT_USER\{path}"))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    for family in [Family::Gecko, Family::Chromium] {
        remove(&windows_manifest(&dir, family), &mut removed)?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(p: &Path) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn manifests_by_family() {
        let exe = Path::new("/opt/it's here/tidyflow");
        let ff = manifest(Family::Gecko, exe);
        assert_eq!(ff["name"], HOST_NAME);
        assert_eq!(ff["path"], "/opt/it's here/tidyflow");
        assert_eq!(ff["allowed_extensions"][0], FIREFOX_ID);
        assert!(ff.get("allowed_origins").is_none());
        let cr = manifest(Family::Chromium, exe);
        assert_eq!(
            cr["allowed_origins"][0],
            format!("chrome-extension://{CHROMIUM_ID}/")
        );
        assert!(cr.get("allowed_extensions").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn installs_only_for_present_browsers() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path();
        let (ff_probe, ff_dir, _) = BROWSERS[0];
        let (cr_probe, cr_dir, _) = BROWSERS
            .iter()
            .find(|b| matches!(b.2, Family::Chromium))
            .unwrap();
        fs::create_dir_all(home.join(ff_probe)).unwrap();
        fs::create_dir_all(home.join(cr_probe)).unwrap();
        let old = legacy_script(home);
        fs::create_dir_all(old.parent().unwrap()).unwrap();
        fs::write(&old, "#!/bin/sh\n").unwrap();

        let exe = Path::new("/opt/tidyflow");
        let files = install(home, exe).unwrap();
        assert_eq!(files.len(), 2, "{files:?}");
        let ff = json(&home.join(ff_dir).join(manifest_name()));
        assert_eq!(ff["path"], "/opt/tidyflow");
        assert_eq!(ff["allowed_extensions"][0], FIREFOX_ID);
        let cr = json(&home.join(cr_dir).join(manifest_name()));
        assert!(cr["allowed_origins"].is_array());
        assert!(!old.exists(), "script antigo ficou");
        let others = BROWSERS
            .iter()
            .filter(|(p, _, _)| *p != ff_probe && p != cr_probe);
        for (_, dir, _) in others {
            assert!(!home.join(dir).exists(), "{dir}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn uninstall_removes_what_install_wrote() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path();
        let (probe, dir, _) = BROWSERS[0];
        fs::create_dir_all(home.join(probe)).unwrap();
        fs::create_dir_all(home.join(dir)).unwrap();
        let other = home.join(dir).join("outro.json");
        fs::write(&other, "{}").unwrap();
        let mut installed = install(home, Path::new("/bin/tidyflow")).unwrap();
        let mut removed = uninstall(home).unwrap();
        installed.sort();
        removed.sort();
        assert_eq!(installed, removed);
        assert!(other.exists());
        assert!(uninstall(home).unwrap().is_empty());
    }

    #[test]
    fn windows_registry_covers_both_families() {
        let dir = Path::new("dados");
        let files: Vec<_> = REGISTRY
            .iter()
            .map(|(_, f)| windows_manifest(dir, *f))
            .collect();
        assert!(files.contains(&dir.join("firefox.json")));
        assert!(files.contains(&dir.join("chromium.json")));
    }
}
