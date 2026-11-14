use crate::event_assert::{of, text};
use crate::install_ops::*;
use crate::late_emit::Events;
use crate::switches::{SwitchFlag, INSTALL, INSTALL_OFF};
use notchling_engine::install::ToolRoots;
use notchling_events::{MemorySink, Salt};
use notchling_local_api::ops;
use notchling_platform::paths::Os;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const BINARY: &str = "/opt/notchling/bin/notchling";

fn setup(tag: &str) -> (PathBuf, InstallSetup) {
    let dir = std::env::temp_dir().join(format!("nl-install-events-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for tool in [".claude", ".codex", ".gemini/config"] {
        fs::create_dir_all(dir.join(format!("home/{tool}"))).unwrap();
    }
    let tools = ToolRoots {
        claude: dir.join("home/.claude"),
        codex: dir.join("home/.codex"),
        antigravity: dir.join("home/.gemini/config"),
    };
    let setup = InstallSetup {
        settings: dir.join("home/.claude/settings.json"),
        binary: Some(PathBuf::from(BINARY)),
        os: Os::Mac,
        skills: default_skills(Path::new(BINARY), &tools.codex),
        tools,
    };
    (dir, setup)
}

fn watched(tag: &str, on: bool) -> (PathBuf, InstallOps, Arc<MemorySink>) {
    let (dir, setup) = setup(tag);
    let sink = Arc::new(MemorySink::new());
    let ops = InstallOps::new(setup, SwitchFlag::in_memory(INSTALL, INSTALL_OFF, on))
        .with_events(Events::fixed(sink.clone(), Salt([1; 32])));
    (dir, ops, sink)
}

#[test]
fn install_apply_records_install_hook() {
    let (dir, ops, sink) = watched("apply", true);
    ops.run(ops::INSTALL_APPLY, &json!({})).unwrap();
    ops.run(ops::INSTALL_REMOVE, &json!({})).unwrap();
    let hooks = of(&sink, "install.hook");
    assert_eq!(hooks.len(), 2);
    assert_eq!(text(&hooks[0], "action").as_deref(), Some("installed"));
    assert_eq!(text(&hooks[0], "target").as_deref(), Some("claude"));
    assert_eq!(text(&hooks[0], "result").as_deref(), Some("done"));
    assert_eq!(text(&hooks[1], "action").as_deref(), Some("removed"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_plan_or_an_off_switch_records_nothing() {
    let (dir, ops, sink) = watched("quiet", false);
    ops.run(ops::INSTALL_PLAN, &json!({})).unwrap();
    ops.run(ops::INSTALL_APPLY, &json!({})).unwrap();
    assert!(sink.events().is_empty());
    let _ = fs::remove_dir_all(dir);
}
