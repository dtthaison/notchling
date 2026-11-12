//! Fakes and scratch folders for the delegate tests. No test runs `codex`, `agy` or `claude`.

use super::deps::Deps;
use super::options::{Options, Tool};
use crate::tools::acp::fake::FakeSpawner as AcpFake;
use crate::tools::acp::Spawner as AcpSpawner;
use crate::tools::codex::fake::FakeSpawner as CodexFake;
use crate::tools::codex::plan::TurnSpec;
use crate::tools::codex::process::{RealSpawner, SpawnError, ToolProcess, ToolSpawner};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A fresh empty folder under the temp dir.
pub fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "nl-delegate-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

pub struct Rig {
    pub deps: Deps,
    pub codex: Arc<CodexFake>,
    pub agy: Arc<AcpFake>,
    /// The folder the tool works in, which exists.
    pub folder: PathBuf,
    pub root: PathBuf,
}

impl Rig {
    pub fn codex_media(&self) -> PathBuf {
        self.deps.codex_media_root.clone()
    }

    pub fn agy_media(&self) -> PathBuf {
        self.deps.agy_media_root.clone()
    }

    pub fn options(&self, tool: Tool, prompt: &str) -> Options {
        Options {
            tool,
            prompt: prompt.into(),
            model: None,
            effort: None,
            folder: self.folder.to_string_lossy().into_owned(),
            out: None,
            write: false,
            session: None,
            timeout: Duration::from_secs(30),
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Deps over fakes and a scratch folder; both tools "installed" at fake paths.
pub fn rig(agy: AcpFake) -> Rig {
    let root = scratch("rig");
    let folder = root.join("work");
    std::fs::create_dir_all(&folder).expect("work folder");
    let codex = Arc::new(CodexFake::new());
    let agy = Arc::new(agy);
    let deps = Deps {
        codex_binary: Some(PathBuf::from("/fake/codex")),
        codex: codex.clone(),
        codex_home: root.join("codex-home"),
        codex_media_root: root.join("codex-home/generated_images"),
        path: "/fake/bin".into(),
        network: true,
        agy_binary: Some("/fake/agy".into()),
        agy: agy.clone() as Arc<dyn AcpSpawner>,
        agy_media_root: root.join("agy/brain"),
        capture: Arc::new(|_, _| None),
        depth: 1,
        stop: Arc::new(AtomicBool::new(false)),
        recorder: None,
        cancel_grace: Duration::from_millis(100),
        terminate_after: Duration::from_millis(200),
    };
    Rig {
        deps,
        codex,
        agy,
        folder,
        root,
    }
}

/// A Codex "binary" that is really `/bin/sh -c <script>`: the real process machinery, a script
/// instead of the tool.
pub struct ShScript {
    pub script: String,
}

impl ToolSpawner for ShScript {
    fn spawn(&self, spec: &TurnSpec) -> Result<Arc<dyn ToolProcess>, SpawnError> {
        RealSpawner.spawn(&TurnSpec {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), self.script.clone()],
            cwd: spec.cwd.clone(),
            // The rig's PATH is a fake; a script needs the real tools.
            env: spec
                .env
                .iter()
                .filter(|(k, _)| k != "PATH")
                .cloned()
                .chain([("PATH".to_string(), Some("/usr/bin:/bin".to_string()))])
                .collect(),
        })
    }
}
