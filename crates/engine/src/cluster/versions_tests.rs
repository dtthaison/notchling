use super::id::Tool;
use super::testkit::versions;
use super::versions::{compat, compat_with, Compat, Purpose};
use crate::versions::{self as vs, Version};

fn v(s: &str) -> Version {
    Version::parse(s).expect("version")
}

#[test]
fn link_refuses_other_proto() {
    let mac = versions("macos");
    let mut other = versions("linux");
    other.proto = 2;
    assert_eq!(
        compat(&mac, &other, "m1", Purpose::Link),
        Compat::Refuse("m1 speaks node protocol 2, this Mac 1".to_string())
    );
    assert_eq!(
        compat(&mac, &versions("linux"), "m1", Purpose::Link),
        Compat::Ok
    );
}

#[test]
fn dispatch_refuses_missing_tool() {
    let mac = versions("macos");
    let mut node = versions("macos");
    node.tools.remove(&vs::Tool::Codex);
    assert_eq!(
        compat(&mac, &node, "m1", Purpose::Dispatch(Tool::Codex)),
        Compat::Refuse("Codex isn't installed on m1".to_string())
    );
}

#[test]
fn absent_tool_still_refuses() {
    let mut mac = versions("macos");
    mac.tools.remove(&vs::Tool::Agy);
    let mut node = versions("macos");
    node.tools.remove(&vs::Tool::Agy);
    assert!(matches!(
        compat(&mac, &node, "m1", Purpose::Dispatch(Tool::Agy)),
        Compat::Refuse(_)
    ));
}

#[test]
fn dispatch_warns_on_a_tool_mismatch_until_phase_6() {
    let mac = versions("macos");
    let mut node = versions("macos");
    node.tools.insert(vs::Tool::Claude, v("2.1.323"));
    assert_eq!(
        compat(&mac, &node, "m1", Purpose::Dispatch(Tool::Claude)),
        Compat::Warn("m1 runs Claude Code 2.1.323, the Mac 2.1.324".to_string())
    );
}

#[test]
fn dispatch_warns_on_notchling_mismatch() {
    let mac = versions("macos");
    let mut node = versions("macos");
    node.notchling = "0.0.9".to_string();
    assert_eq!(
        compat(&mac, &node, "m1", Purpose::Dispatch(Tool::Codex)),
        Compat::Warn("m1 runs Notchling 0.0.9, the Mac 0.1.0".to_string())
    );
    node.tools.insert(vs::Tool::Codex, v("0.165.0"));
    assert_eq!(
        compat(&mac, &node, "m1", Purpose::Dispatch(Tool::Codex)),
        Compat::Warn(
            "m1 runs Codex 0.165.0, the Mac 0.166.0; m1 runs Notchling 0.0.9, the Mac 0.1.0"
                .to_string()
        )
    );
}

#[test]
fn shell_skips_the_tool_rules() {
    let mac = versions("macos");
    let mut node = versions("linux");
    node.tools.clear();
    assert_eq!(
        compat(&mac, &node, "w1", Purpose::Dispatch(Tool::Shell)),
        Compat::Ok
    );
}

#[test]
fn dispatch_refuses_linux_claude_until_proven() {
    let mac = versions("macos");
    let node = versions("linux");
    assert_eq!(
        compat_with(&mac, &node, "w1", Purpose::Dispatch(Tool::Claude), false),
        Compat::Refuse("Claude Code on Linux isn't proven yet".to_string())
    );
    assert_eq!(
        compat_with(&mac, &node, "w1", Purpose::Dispatch(Tool::Claude), true),
        Compat::Ok
    );
    assert_eq!(
        compat(&mac, &node, "w1", Purpose::Dispatch(Tool::Claude)),
        Compat::Refuse("Claude Code on Linux isn't proven yet".to_string())
    );
}

#[test]
fn windows_never_runs_claude() {
    let mac = versions("macos");
    let node = versions("windows");
    assert_eq!(
        compat_with(&mac, &node, "w1", Purpose::Dispatch(Tool::Claude), true),
        Compat::Refuse("Claude Code doesn't run on Windows nodes in v1".to_string())
    );
}

#[test]
fn dispatch_needs_the_feature_and_the_schema() {
    let mac = versions("macos");
    let mut node = versions("macos");
    node.features.clear();
    assert!(matches!(
        compat(&mac, &node, "m1", Purpose::Dispatch(Tool::Codex)),
        Compat::Refuse(_)
    ));
    let mut node = versions("macos");
    node.schema = 2;
    assert!(matches!(
        compat(&mac, &node, "m1", Purpose::Dispatch(Tool::Codex)),
        Compat::Refuse(_)
    ));
}

#[test]
fn promote_needs_same_build() {
    let mac = versions("macos");
    assert_eq!(
        compat(&mac, &versions("macos"), "m1", Purpose::Promote),
        Compat::Ok
    );
    let mut node = versions("macos");
    node.notchling = "0.0.9".to_string();
    assert!(matches!(
        compat(&mac, &node, "m1", Purpose::Promote),
        Compat::Refuse(t) if t.contains("Notchling")
    ));
    let mut node = versions("macos");
    node.tools.insert(vs::Tool::Claude, v("2.1.323"));
    assert!(matches!(
        compat(&mac, &node, "m1", Purpose::Promote),
        Compat::Refuse(t) if t.contains("Claude Code 2.1.323")
    ));
    let mut node = versions("macos");
    node.tools.remove(&vs::Tool::Claude);
    assert!(matches!(
        compat(&mac, &node, "m1", Purpose::Promote),
        Compat::Refuse(t) if t.contains("isn't installed")
    ));
}
