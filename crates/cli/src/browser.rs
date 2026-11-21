//! Instala o host de native messaging para os navegadores encontrados na home.

use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub const HOST_NAME: &str = "io.github.langendolff.tidyflow";
pub const FIREFOX_ID: &str = "tidyflow@langendolff.github.io";
/// Derivado da `key` em `browser/manifest.base.json` (o teste do build confere).
pub const CHROMIUM_ID: &str = "ofmjjmckchphahpaihfcionnajfllndg";

enum Family {
    Gecko,
    Chromium,
}

/// (raiz do navegador, pasta dos manifests relativa à raiz, família)
const BROWSERS: [(&str, &str, Family); 5] = [
    (".mozilla", "native-messaging-hosts", Family::Gecko),
    (".librewolf", "native-messaging-hosts", Family::Gecko),
    (".config/chromium", "NativeMessagingHosts", Family::Chromium),
    (
        ".config/google-chrome",
        "NativeMessagingHosts",
        Family::Chromium,
    ),
    (
        ".config/BraveSoftware/Brave-Browser",
        "NativeMessagingHosts",
        Family::Chromium,
    ),
];

fn script_path(home: &Path) -> PathBuf {
    home.join(".local/share/tidyflow/native-host")
}

fn manifest_name() -> String {
    format!("{HOST_NAME}.json")
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// O manifest não aceita argumentos: ele aponta para um script que chama o binário.
pub fn install(home: &Path, exe: &Path) -> io::Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    let script = script_path(home);
    fs::create_dir_all(script.parent().expect("caminho com pasta"))?;
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nexec {} native-host \"$@\"\n",
            shell_quote(&exe.to_string_lossy())
        ),
    )?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
    written.push(script.clone());

    for (root, dir, family) in BROWSERS {
        let root = home.join(root);
        if !root.is_dir() {
            continue;
        }
        let mut manifest = serde_json::json!({
            "name": HOST_NAME,
            "description": "TidyFlow: registra downloads para organizar os arquivos",
            "path": script,
            "type": "stdio",
        });
        match family {
            Family::Gecko => manifest["allowed_extensions"] = serde_json::json!([FIREFOX_ID]),
            Family::Chromium => {
                manifest["allowed_origins"] =
                    serde_json::json!([format!("chrome-extension://{CHROMIUM_ID}/")])
            }
        }
        let dir = root.join(dir);
        fs::create_dir_all(&dir)?;
        let file = dir.join(manifest_name());
        fs::write(&file, serde_json::to_string_pretty(&manifest)? + "\n")?;
        written.push(file);
    }
    Ok(written)
}

pub fn uninstall(home: &Path) -> io::Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    let candidates = BROWSERS
        .iter()
        .map(|(root, dir, _)| home.join(root).join(dir).join(manifest_name()))
        .chain([script_path(home)]);
    for file in candidates {
        match fs::remove_file(&file) {
            Ok(()) => removed.push(file),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
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
    fn installs_only_for_present_browsers() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path();
        fs::create_dir_all(home.join(".mozilla")).unwrap();
        fs::create_dir_all(home.join(".config/chromium")).unwrap();
        let exe = Path::new("/opt/it's here/tidyflow");
        let files = install(home, exe).unwrap();
        assert_eq!(files.len(), 3);

        let script = script_path(home);
        let body = fs::read_to_string(&script).unwrap();
        assert_eq!(
            body,
            "#!/bin/sh\nexec '/opt/it'\\''s here/tidyflow' native-host \"$@\"\n"
        );
        assert_eq!(
            fs::metadata(&script).unwrap().permissions().mode() & 0o777,
            0o755
        );

        let ff = json(
            &home
                .join(".mozilla/native-messaging-hosts")
                .join(manifest_name()),
        );
        assert_eq!(ff["name"], HOST_NAME);
        assert_eq!(ff["path"], script.to_str().unwrap());
        assert_eq!(ff["allowed_extensions"][0], FIREFOX_ID);
        assert!(ff.get("allowed_origins").is_none());

        let cr = json(
            &home
                .join(".config/chromium/NativeMessagingHosts")
                .join(manifest_name()),
        );
        assert_eq!(
            cr["allowed_origins"][0],
            format!("chrome-extension://{CHROMIUM_ID}/")
        );
        assert!(!home.join(".config/google-chrome").exists());
        assert!(!home.join(".librewolf").exists());
    }

    #[test]
    fn uninstall_removes_what_install_wrote() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path();
        fs::create_dir_all(home.join(".mozilla/native-messaging-hosts")).unwrap();
        let other = home.join(".mozilla/native-messaging-hosts/outro.json");
        fs::write(&other, "{}").unwrap();
        let mut installed = install(home, Path::new("/bin/tidyflow")).unwrap();
        let mut removed = uninstall(home).unwrap();
        installed.sort();
        removed.sort();
        assert_eq!(installed, removed);
        assert!(other.exists());
        assert!(uninstall(home).unwrap().is_empty());
    }
}
