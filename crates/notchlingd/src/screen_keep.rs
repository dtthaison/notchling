use std::path::{Path, PathBuf};

pub(crate) fn keep_screen(file: &Path, head: &[String], lines: &[String]) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    static WRITES: AtomicU64 = AtomicU64::new(0);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut name = file.as_os_str().to_os_string();
    name.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed)
    ));
    let temp = PathBuf::from(name);
    let Ok(mut out) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
    else {
        return;
    };
    for line in head {
        let _ = writeln!(out, "{line}");
    }
    let _ = writeln!(out, "----- screen ({} lines) -----", lines.len());
    for line in lines {
        let _ = writeln!(out, "{line}");
    }
    drop(out);
    if std::fs::rename(&temp, file).is_err() {
        let _ = std::fs::remove_file(&temp);
    }
}
