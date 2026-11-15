use crate::{config::Action, facts::Facts, template::render};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Move,
    Copy,
    Rename,
    Trash,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Move => "move",
            Kind::Copy => "copy",
            Kind::Rename => "rename",
            Kind::Trash => "trash",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        [Kind::Move, Kind::Copy, Kind::Rename, Kind::Trash]
            .into_iter()
            .find(|k| k.as_str() == s)
    }
}

pub fn kind_of(a: &Action) -> Kind {
    match a {
        Action::Move(_) => Kind::Move,
        Action::Copy(_) => Kind::Copy,
        Action::Rename(_) => Kind::Rename,
        Action::Trash => Kind::Trash,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    pub kind: Kind,
    pub src: PathBuf,
    pub dst: Option<PathBuf>,
}

pub fn destination(a: &Action, f: &Facts) -> Option<PathBuf> {
    match a {
        Action::Move(dir) | Action::Copy(dir) => Some(Path::new(&render(dir, f)).join(&f.name)),
        Action::Rename(name) => Some(f.path.with_file_name(render(name, f))),
        Action::Trash => None,
    }
}

pub fn execute(a: &Action, f: &Facts) -> io::Result<Option<Done>> {
    let kind = kind_of(a);
    let Some(target) = destination(a, f) else {
        trash::delete(&f.path).map_err(|e| io::Error::other(e.to_string()))?;
        return Ok(Some(Done {
            kind,
            src: f.path.clone(),
            dst: None,
        }));
    };
    if target == f.path {
        return Ok(None);
    }
    let dst = free_path(&target);
    if let Some(dir) = dst.parent() {
        fs::create_dir_all(dir)?;
    }
    match kind {
        Kind::Copy => copy_verified(&f.path, &dst)?,
        _ => move_file(&f.path, &dst)?,
    }
    Ok(Some(Done {
        kind,
        src: f.path.clone(),
        dst: Some(dst),
    }))
}

/// Devolve se a marca `KEEP_XATTR` foi gravada no arquivo restaurado.
pub fn undo(kind: Kind, src: &Path, dst: Option<&Path>) -> io::Result<bool> {
    if kind == Kind::Trash {
        restore_from_trash(src)?;
        return Ok(keep(src));
    }
    let dst = dst.ok_or_else(|| io::Error::other("ação sem destino registrado"))?;
    if !dst.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("arquivo não encontrado em {}", dst.display()),
        ));
    }
    if kind == Kind::Copy {
        // A cópia é redundante, mas vai para a lixeira: nunca apagar de vez.
        trash::delete(dst).map_err(|e| io::Error::other(e.to_string()))?;
        return Ok(keep(src));
    }
    let target = free_path(src);
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir)?;
    }
    move_file(dst, &target)?;
    Ok(keep(&target))
}

/// Marca o arquivo restaurado para o engine não reorganizá-lo.
fn keep(path: &Path) -> bool {
    // Falha (fs sem xattr, arquivo somente leitura) não desfaz o undo; o journal cobre.
    xattr::set(path, crate::facts::KEEP_XATTR, b"1").is_ok()
}

fn restore_from_trash(src: &Path) -> io::Result<()> {
    let item = trash::os_limited::list()
        .map_err(|e| io::Error::other(e.to_string()))?
        .into_iter()
        .filter(|i| i.original_path() == src)
        .max_by_key(|i| i.time_deleted)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} não está na lixeira", src.display()),
            )
        })?;
    trash::os_limited::restore_all([item]).map_err(|e| match e {
        trash::Error::RestoreCollision { .. } => io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "já existe um arquivo em {}; o original continua na lixeira",
                src.display()
            ),
        ),
        e => io::Error::other(e.to_string()),
    })
}

// ponytail: verificar e depois usar (TOCTOU). Duas instâncias escrevendo no mesmo
// destino ao mesmo tempo podem colidir. Usar O_EXCL / renameat2(NOREPLACE) se isso acontecer.
fn free_path(p: &Path) -> PathBuf {
    // symlink_metadata: um symlink quebrado também ocupa o nome.
    let taken = |c: &Path| fs::symlink_metadata(c).is_ok();
    if !taken(p) {
        return p.to_path_buf();
    }
    let stem = p.file_stem().unwrap_or_default().to_string_lossy();
    let ext = p.extension().map(|e| e.to_string_lossy());
    (1..)
        .map(|n| match &ext {
            Some(e) => p.with_file_name(format!("{stem} ({n}).{e}")),
            None => p.with_file_name(format!("{stem} ({n})")),
        })
        .find(|c| !taken(c))
        .expect("sufixo livre")
}

fn move_file(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => move_via_copy(src, dst),
        r => r,
    }
}

pub(crate) fn move_via_copy(src: &Path, dst: &Path) -> io::Result<()> {
    let snapshot = snapshot(src)?;
    copy_verified(src, dst)?;
    if !unchanged(src, snapshot)? {
        let _ = fs::remove_file(dst); // só a nossa cópia
        return Err(io::Error::other("arquivo de origem mudou durante a cópia"));
    }
    fs::remove_file(src).inspect_err(|_| {
        let _ = fs::remove_file(dst); // a origem continua intacta; não deixar duplicata
    })
}

fn snapshot(src: &Path) -> io::Result<(u64, std::time::SystemTime)> {
    let m = fs::metadata(src)?;
    Ok((m.len(), m.modified()?))
}

fn unchanged(src: &Path, snap: (u64, std::time::SystemTime)) -> io::Result<bool> {
    Ok(snapshot(src)? == snap)
}

fn copy_verified(src: &Path, dst: &Path) -> io::Result<()> {
    // create_new: falha se algo surgiu depois de free_path; nesse caso não removemos nada.
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)?;
    let result = (|| {
        let meta = fs::metadata(src)?;
        let mut input = fs::File::open(src)?;
        io::copy(&mut input, &mut out)?;
        out.set_permissions(meta.permissions())?;
        out.set_modified(meta.modified()?)?;
        out.sync_all()?;
        if fs::metadata(dst)?.len() != meta.len() {
            return Err(io::Error::other("tamanho da cópia não confere"));
        }
        copy_xattrs(src, dst);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(dst); // só a nossa cópia parcial
    }
    result
}

// Atributos estendidos (ex.: user.xdg.origin.url); falhas por atributo são ignoradas
// porque nem todo sistema de arquivos suporta xattr.
fn copy_xattrs(src: &Path, dst: &Path) {
    let Ok(names) = xattr::list(src) else { return };
    for n in names {
        if let Ok(Some(v)) = xattr::get(src, &n) {
            let _ = xattr::set(dst, &n, &v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(dir: &Path, name: &str, body: &str) -> Facts {
        let p = dir.join(name);
        fs::write(&p, body).unwrap();
        Facts::read(&p).unwrap()
    }

    fn mv(dir: &Path) -> Action {
        Action::Move(format!("{}/out/{{ext}}", dir.display()))
    }

    #[test]
    fn kind_roundtrip() {
        for k in [Kind::Move, Kind::Copy, Kind::Rename, Kind::Trash] {
            assert_eq!(Kind::parse(k.as_str()), Some(k));
        }
        assert_eq!(Kind::parse("x"), None);
    }

    #[test]
    fn move_creates_dirs_and_undo_restores() {
        let d = tempfile::tempdir().unwrap();
        let f = file(d.path(), "a.pdf", "1");
        let done = execute(&mv(d.path()), &f).unwrap().unwrap();
        let dst = d.path().join("out/pdf/a.pdf");
        assert_eq!(
            done,
            Done {
                kind: Kind::Move,
                src: f.path.clone(),
                dst: Some(dst.clone())
            }
        );
        assert!(dst.exists() && !f.path.exists());

        undo(done.kind, &done.src, done.dst.as_deref()).unwrap();
        assert!(f.path.exists() && !dst.exists());
    }

    #[test]
    fn never_overwrites() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("out/pdf")).unwrap();
        fs::write(d.path().join("out/pdf/a.pdf"), "velho").unwrap();
        fs::write(d.path().join("out/pdf/a (1).pdf"), "velho").unwrap();
        let f = file(d.path(), "a.pdf", "novo");
        let done = execute(&mv(d.path()), &f).unwrap().unwrap();
        assert_eq!(done.dst.unwrap(), d.path().join("out/pdf/a (2).pdf"));
        assert_eq!(
            fs::read_to_string(d.path().join("out/pdf/a.pdf")).unwrap(),
            "velho"
        );
    }

    #[test]
    fn copy_keeps_source_and_undo_trashes_copy() {
        let d = tempfile::tempdir().unwrap();
        let f = file(d.path(), "a.txt", "x");
        let a = Action::Copy(format!("{}/bak", d.path().display()));
        let done = execute(&a, &f).unwrap().unwrap();
        let dst = done.dst.clone().unwrap();
        assert!(f.path.exists() && dst.exists());
        undo(done.kind, &done.src, done.dst.as_deref()).unwrap();
        assert!(f.path.exists() && !dst.exists());
    }

    #[test]
    fn rename_and_noop_when_already_named() {
        let d = tempfile::tempdir().unwrap();
        let f = file(d.path(), "a.txt", "x");
        let a = Action::Rename("doc-{name}.{ext}".into());
        let done = execute(&a, &f).unwrap().unwrap();
        assert_eq!(done.dst.unwrap(), d.path().join("doc-a.txt"));

        let g = file(d.path(), "b.txt", "x");
        assert_eq!(
            execute(&Action::Rename("{name}.{ext}".into()), &g).unwrap(),
            None
        );
    }

    #[test]
    fn cross_device_path_copies_then_removes() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a.bin");
        let dst = d.path().join("b.bin");
        fs::write(&src, vec![7u8; 10_000]).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(86_400 * 30);
        fs::File::options()
            .write(true)
            .open(&src)
            .unwrap()
            .set_modified(old)
            .unwrap();
        xattr::set(&src, "user.xdg.origin.url", b"http://x").unwrap();
        move_via_copy(&src, &dst).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::metadata(&dst).unwrap().modified().unwrap(), old);
        assert_eq!(
            xattr::get(&dst, "user.xdg.origin.url").unwrap().unwrap(),
            b"http://x"
        );
        assert_eq!(fs::read(&dst).unwrap(), vec![7u8; 10_000]);
    }

    #[test]
    fn unchanged_detects_len_and_mtime_changes() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a.bin");
        fs::write(&src, "x").unwrap();
        let snap = snapshot(&src).unwrap();
        assert!(unchanged(&src, snap).unwrap());
        fs::File::options()
            .write(true)
            .open(&src)
            .unwrap()
            .set_modified(snap.1 + std::time::Duration::from_secs(5))
            .unwrap();
        assert!(!unchanged(&src, snap).unwrap());
        let snap = snapshot(&src).unwrap();
        fs::write(&src, "xyz").unwrap();
        assert!(!unchanged(&src, snap).unwrap());
    }

    #[test]
    fn failed_source_removal_removes_our_copy() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let ro = d.path().join("ro");
        fs::create_dir(&ro).unwrap();
        let src = ro.join("a.bin");
        let dst = d.path().join("b.bin");
        fs::write(&src, "x").unwrap();
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o555)).unwrap();
        let r = move_via_copy(&src, &dst);
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(r.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert!(src.exists() && !dst.exists());
    }

    #[test]
    fn failed_copy_leaves_source_and_no_partial() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a.bin");
        fs::write(&src, "x").unwrap();
        let dst = d.path().join("nao/existe/b.bin"); // pasta pai ausente → fs::copy falha
        assert!(move_via_copy(&src, &dst).is_err());
        assert!(src.exists() && !dst.exists());
    }

    #[test]
    fn undo_with_occupied_source_uses_suffix() {
        let d = tempfile::tempdir().unwrap();
        let f = file(d.path(), "a.pdf", "1");
        let done = execute(&mv(d.path()), &f).unwrap().unwrap();
        fs::write(&f.path, "outro").unwrap();
        undo(done.kind, &done.src, done.dst.as_deref()).unwrap();
        assert_eq!(fs::read_to_string(d.path().join("a (1).pdf")).unwrap(), "1");
        assert_eq!(fs::read_to_string(&f.path).unwrap(), "outro");
    }

    #[test]
    fn undo_with_missing_destination_errors() {
        let err = undo(
            Kind::Move,
            Path::new("/tmp/x"),
            Some(Path::new("/nao/existe")),
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("/nao/existe"));
    }

    #[test]
    fn trash_and_restore() {
        let d = tempfile::tempdir().unwrap();
        let f = file(d.path(), "lixo.txt", "x");
        let done = execute(&Action::Trash, &f).unwrap().unwrap();
        assert!(!f.path.exists() && done.dst.is_none());
        undo(done.kind, &done.src, None).unwrap();
        assert!(f.path.exists());
    }

    #[test]
    fn trash_restore_with_occupied_src_is_explicit() {
        let d = tempfile::tempdir().unwrap();
        let f = file(d.path(), "ocupado.txt", "x");
        let done = execute(&Action::Trash, &f).unwrap().unwrap();
        fs::write(&f.path, "outro").unwrap();
        let err = undo(done.kind, &done.src, None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(
            err.to_string().contains("o original continua na lixeira"),
            "{err}"
        );
        assert_eq!(fs::read_to_string(&f.path).unwrap(), "outro");
    }

    #[test]
    fn copy_does_not_touch_preexisting_dst() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a.bin");
        let dst = d.path().join("b.bin");
        fs::write(&src, "novo").unwrap();
        fs::write(&dst, "velho").unwrap();
        assert!(move_via_copy(&src, &dst).is_err());
        assert_eq!(fs::read_to_string(&src).unwrap(), "novo");
        assert_eq!(fs::read_to_string(&dst).unwrap(), "velho");
    }

    #[test]
    fn dangling_symlink_counts_as_occupied() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("out/pdf")).unwrap();
        let link = d.path().join("out/pdf/a.pdf");
        std::os::unix::fs::symlink("/nao/existe/alvo", &link).unwrap();
        let f = file(d.path(), "a.pdf", "1");
        let done = execute(&mv(d.path()), &f).unwrap().unwrap();
        assert_eq!(done.dst.unwrap(), d.path().join("out/pdf/a (1).pdf"));
        assert_eq!(fs::read_link(&link).unwrap(), Path::new("/nao/existe/alvo"));
    }
}
