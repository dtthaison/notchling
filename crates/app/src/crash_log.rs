use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 1024 * 1024;

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("logs").join("crash.log")
}

pub fn entry(when: &str, thread: &str, message: &str, location: &str, backtrace: &str) -> String {
    format!(
        "=== {when} thread '{thread}' panicked at {location}\n{message}\n{}\n\n",
        backtrace.trim_end()
    )
}

fn payload_text(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".to_owned())
}

pub fn append(file: &Path, text: &str) {
    let Some(dir) = file.parent() else {
        return;
    };
    if notchling_platform::files::ensure_private_dir(dir).is_err() {
        return;
    }
    if std::fs::metadata(file).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(file, file.with_extension("log.1"));
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc_no_follow());
    }
    if let Ok(mut f) = options.open(file) {
        let _ = f.write_all(text.as_bytes());
    }
}

#[cfg(unix)]
fn libc_no_follow() -> i32 {
    #[cfg(target_os = "macos")]
    {
        0x0100
    }
    #[cfg(not(target_os = "macos"))]
    {
        0o400000
    }
}

pub fn install(data_dir: PathBuf) {
    let file = path(&data_dir);
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "an unknown place".to_owned());
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let when = chrono_free_now();
        append(
            &file,
            &entry(
                &when,
                thread.name().unwrap_or("unnamed"),
                &payload_text(info),
                &location,
                &backtrace,
            ),
        );
        default(info);
    }));
}

fn chrono_free_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix {secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_names_the_place_and_keeps_the_message_and_backtrace() {
        let e = entry(
            "unix 1",
            "main",
            "byte index 6 is not a char boundary",
            "a.rs:1:2",
            "0: f\n",
        );
        assert_eq!(
            e,
            "=== unix 1 thread 'main' panicked at a.rs:1:2\nbyte index 6 is not a char boundary\n0: f\n\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_log_is_private_appends_and_rolls_over() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("nl-crash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = path(&dir);
        append(&file, "one\n");
        append(&file, "two\n");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n");
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        std::fs::write(&file, vec![b'x'; (MAX_BYTES + 1) as usize]).unwrap();
        append(&file, "three\n");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "three\n");
        assert!(file.with_extension("log.1").exists());

        let linked = dir.join("linked");
        std::fs::create_dir_all(&linked).unwrap();
        let victim = dir.join("victim");
        std::fs::write(&victim, "mine").unwrap();
        std::os::unix::fs::symlink(&victim, linked.join("crash.log")).unwrap();
        append(&linked.join("crash.log"), "x");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "mine");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
