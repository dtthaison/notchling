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

#[derive(Clone, Debug)]
pub struct Folders {
    pub home: PathBuf,
    pub codex_home: PathBuf,
    pub antigravity: PathBuf,
    pub claude_json: PathBuf,
    pub path: String,
    pub network: bool,
}

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
        agy_trust: Some(super::agy_trust::Files {
            claude_json: folders.claude_json.clone(),
            settings: folders.antigravity.join("settings.json"),
        }),
        recorder: None,
        cancel_grace: Deps::CANCEL_GRACE,
        terminate_after: Deps::TERMINATE_AFTER,
    }
}

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
            claude_json: home.join(".claude.json"),
            path: String::new(),
            network: true,
        };
        let d = deps(&folders, Arc::new(AtomicBool::new(false)));
        assert_eq!(d.codex_media_root, home.join(".codex/generated_images"));
        assert_eq!(d.agy_media_root, home.join(".gemini/antigravity-cli/brain"));
        assert!(d.path.contains("/opt/homebrew/bin"));
        assert!(d
            .codex_binary
            .as_ref()
            .is_none_or(|p| !p.starts_with(&home)));
    }
}
