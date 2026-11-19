use super::keys::Probe;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

pub const WAIT: Duration = Duration::from_secs(15);

pub struct CommandProbe {
    pub binary: PathBuf,
    pub home: Option<PathBuf>,
}

impl Probe for CommandProbe {
    fn accepts(&self, key: &str, raw: &str) -> bool {
        let mut command = Command::new(&self.binary);
        command
            .args(["features", "list", "-c", &format!("{key}={raw}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(home) = &self.home {
            command.env("CODEX_HOME", home);
        }
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("notchling-codex-probe".into())
            .spawn(move || {
                let _ = tx.send(command.status().map(|s| s.success()).unwrap_or(false));
            });
        spawned.is_ok() && rx.recv_timeout(WAIT).unwrap_or(false)
    }
}

pub struct NoProbe;

impl Probe for NoProbe {
    fn accepts(&self, _key: &str, _raw: &str) -> bool {
        false
    }
}
