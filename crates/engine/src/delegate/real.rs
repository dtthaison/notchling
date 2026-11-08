//! The real [`Deps`]: the tools found on this machine and started as child processes. Which
//! folders are read is the caller's choice ([`Folders`]), so a fixture root can stand in for the
//! user's own.

use super::deps::Deps;
use crate::tools::acp::RealSpawner as AcpSpawner;
use crate::tools::codex::plan::{augmented_path, find_binary};
use crate::tools::codex::process::RealSpawner as CodexSpawner;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Where things are on this machine.
#[derive(Clone, Debug)]
pub struct Folders {
    /// The home folder: the tools' usual install places are looked for under it.
    pub home: PathBuf,
    /// Codex's own folder (`$CODEX_HOME` or `~/.codex`).
    pub codex_home: PathBuf,
    /// `~/.gemini/antigravity-cli`.
    pub antigravity: PathBuf,
    /// The process's PATH.
    pub path: String,
    /// "Allow Network in Codex Turns".
    pub network: bool,
}

/// How long `agy models` may take.
const MODELS_TIMEOUT: Duration = Duration::from_secs(60);

pub fn deps(folders: &Folders, stop: Arc<AtomicBool>) -> Deps {
    let path = augmented_path(&folders.path, &folders.home);
    Deps {
        codex_binary: find_binary(&folders.home, &path, Path::is_file),
        codex: Arc::new(CodexSpawner),
        codex_home: folders.codex_home.clone(),
        codex_media_root: folders.codex_home.join("generated_images"),
        path,
        network: folders.network,
        agy_binary: crate::tools::agy::find_binary(&folders.home)
            .map(|p| p.to_string_lossy().into_owned()),
        agy: Arc::new(AcpSpawner::new(&folders.home)),
        agy_media_root: folders.antigravity.join("brain"),
        capture: Arc::new(|program, args| capture(program, args, MODELS_TIMEOUT)),
        depth: 1,
        stop,
        cancel_grace: Deps::CANCEL_GRACE,
        terminate_after: Deps::TERMINATE_AFTER,
    }
}

/// Runs `program` with `args` and no input, and returns its standard output if it exits 0 within
/// `timeout`; a program that does not start, fails or hangs gives `None` (and is killed).
pub fn capture(program: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        let _ = stdout.read_to_end(&mut data);
        data
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let data = reader.join().ok()?;
    status
        .filter(|s| s.success())
        .map(|_| String::from_utf8_lossy(&data).into_owned())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn capture_returns_what_a_successful_program_printed() {
        let out = capture(
            Path::new("/bin/sh"),
            &["-c", "echo one; echo two"],
            Duration::from_secs(10),
        );
        assert_eq!(out.as_deref(), Some("one\ntwo\n"));
    }

    #[test]
    fn a_failing_missing_or_hanging_program_gives_nothing() {
        let t = Duration::from_secs(10);
        assert_eq!(
            capture(Path::new("/bin/sh"), &["-c", "echo x; exit 3"], t),
            None
        );
        assert_eq!(capture(Path::new("/no/such/program"), &[], t), None);
        let began = Instant::now();
        assert_eq!(
            capture(
                Path::new("/bin/sh"),
                &["-c", "exec sleep 30"],
                Duration::from_millis(200)
            ),
            None
        );
        assert!(began.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn the_real_deps_find_nothing_in_an_empty_home() {
        let home = std::env::temp_dir().join(format!("nl-delegate-real-{}", std::process::id()));
        let folders = Folders {
            home: home.clone(),
            codex_home: home.join(".codex"),
            antigravity: home.join(".gemini/antigravity-cli"),
            path: String::new(),
            network: true,
        };
        let d = deps(&folders, Arc::new(AtomicBool::new(false)));
        assert_eq!(d.codex_media_root, home.join(".codex/generated_images"));
        assert_eq!(d.agy_media_root, home.join(".gemini/antigravity-cli/brain"));
        assert!(d.path.contains("/opt/homebrew/bin"));
        // The usual places outside the home may hold a real install on a developer's machine;
        // what is checked here is only that nothing under the empty home was found.
        assert!(d
            .codex_binary
            .as_ref()
            .is_none_or(|p| !p.starts_with(&home)));
    }
}
