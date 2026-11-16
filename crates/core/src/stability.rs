use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant, SystemTime},
};

const TEMP_SUFFIXES: [&str; 3] = [".part", ".crdownload", ".tmp"];

pub fn is_ignored_name(name: &str) -> bool {
    name.starts_with('.') || TEMP_SUFFIXES.iter().any(|s| name.ends_with(s))
}

/// Firefox cria `X` vazio como marcador ao lado de `X.part` enquanto baixa;
/// mover o marcador faz o arquivo real colidir com ele depois.
pub fn download_in_progress(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    TEMP_SUFFIXES.iter().any(|s| {
        let mut sibling = name.to_os_string();
        sibling.push(s);
        path.with_file_name(sibling).exists()
    })
}

fn snapshot(path: &Path) -> Option<(u64, SystemTime)> {
    let m = fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

pub fn wait_stable(path: &Path, quiet: Duration, max_wait: Duration) -> bool {
    let start = Instant::now();
    let Some(mut last) = snapshot(path) else {
        return false;
    };
    loop {
        thread::sleep(quiet);
        let Some(now) = snapshot(path) else {
            return false;
        };
        if now == last {
            return true;
        }
        if start.elapsed() >= max_wait {
            return false;
        }
        last = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write, thread};

    #[test]
    fn ignored_names() {
        for n in [".hidden", "a.part", "a.pdf.crdownload", "x.tmp"] {
            assert!(is_ignored_name(n), "{n}");
        }
        for n in ["a.pdf", "partitura.pdf", "tmp.txt"] {
            assert!(!is_ignored_name(n), "{n}");
        }
    }

    #[test]
    fn placeholder_with_part_sibling() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.deb");
        fs::write(&p, "").unwrap();
        assert!(!download_in_progress(&p));
        fs::write(d.path().join("a.deb.part"), "x").unwrap();
        assert!(download_in_progress(&p));
    }

    #[test]
    fn stable_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a");
        fs::write(&p, "x").unwrap();
        assert!(wait_stable(
            &p,
            Duration::from_millis(50),
            Duration::from_secs(1)
        ));
    }

    #[test]
    fn missing_file() {
        assert!(!wait_stable(
            Path::new("/nao/existe"),
            Duration::from_millis(10),
            Duration::from_secs(1)
        ));
    }

    #[test]
    fn growing_file_times_out() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a");
        let mut f = fs::File::create(&p).unwrap();
        let writer = thread::spawn(move || {
            for _ in 0..30 {
                f.write_all(b"x").unwrap();
                f.flush().unwrap();
                thread::sleep(Duration::from_millis(20));
            }
        });
        assert!(!wait_stable(
            &p,
            Duration::from_millis(100),
            Duration::from_millis(300)
        ));
        writer.join().unwrap();
    }
}
