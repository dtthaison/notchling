use crate::inbox_ops::InboxOps;
use crate::versions_check_tests::{check, claude_transcripts, native_claude, out, ready, rig, Rig};
use notchling_engine::inbox::{ActionId, Item, Kind, State};
use std::sync::Arc;

fn with_inbox(rig: &Rig) -> Arc<InboxOps> {
    std::fs::create_dir_all(rig.tmp.path("inbox-data")).unwrap();
    let inbox = InboxOps::with_clock(
        rig.tmp.path("inbox-data"),
        rig.room.clone(),
        Arc::new(|| 5_000),
    );
    rig.ops.checks().set_inbox(inbox.clone());
    inbox
}

fn notices(inbox: &InboxOps) -> Vec<Item> {
    inbox
        .items()
        .into_iter()
        .filter(|i| i.kind == Kind::VersionCheckFailed)
        .collect()
}

fn fail(rig: &Rig) {
    rig.launcher.push_output(out("9.9.9 (Claude Code)\n"));
    rig.launcher.push_output(out("Auto-update: enabled\n"));
    check(rig, "claude");
    rig.ops.wait_checks();
}

fn pass(rig: &Rig) {
    rig.launcher.push_output(out("2.1.324 (Claude Code)\n"));
    rig.launcher.push_output(out("Auto-update: enabled\n"));
    check(rig, "claude");
    rig.ops.wait_checks();
}

fn setup(tag: &str) -> (Rig, Arc<InboxOps>) {
    let rig = rig(tag);
    native_claude(&rig, "2.1.324", "claude 2.1.324");
    claude_transcripts(&rig);
    let inbox = with_inbox(&rig);
    ready(&rig);
    (rig, inbox)
}

#[cfg(unix)]
#[test]
fn failed_check_raises_one_item() {
    let (rig, inbox) = setup("vinbox-one");
    fail(&rig);
    let items = notices(&inbox);
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert!(item.id.starts_with("version-check:"));
    assert_eq!(item.title, "Claude Code 2.1.324 Didn't Pass");
    assert!(
        item.detail.contains("doesn't say 2.1.324"),
        "{}",
        item.detail
    );
    assert!(!item.detail.contains("Workers stay"), "no reference yet");
    assert_eq!(item.state, State::Open);
    assert_eq!(item.actions.len(), 1);
    assert_eq!(item.actions[0].id, ActionId::Open);
    assert_eq!(item.actions[0].label, "Open Settings");
    assert!(item.actions[0].primary);
    assert_eq!(
        item.open,
        Some(serde_json::json!({"settings": "agents", "tool": "claude"}))
    );
}

#[cfg(unix)]
#[test]
fn pass_after_check_again_withdraws_it() {
    let (rig, inbox) = setup("vinbox-pass");
    fail(&rig);
    pass(&rig);
    let items = notices(&inbox);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].state, State::Withdrawn);
}

#[cfg(unix)]
#[test]
fn ids_are_fresh_per_failure() {
    let (rig, inbox) = setup("vinbox-fresh");
    fail(&rig);
    fail(&rig);
    let items = notices(&inbox);
    assert_eq!(items.len(), 2);
    assert_ne!(items[0].id, items[1].id);
    let open = items.iter().filter(|i| i.state == State::Open).count();
    let withdrawn = items.iter().filter(|i| i.state == State::Withdrawn).count();
    assert_eq!((open, withdrawn), (1, 1));
}

#[cfg(unix)]
#[test]
fn moving_on_withdraws_it() {
    let (rig, inbox) = setup("vinbox-moved");
    fail(&rig);
    native_claude(&rig, "2.1.325", "claude 2.1.325");
    rig.launcher.push_output(out("2.1.325 (Claude Code)\n"));
    rig.launcher.push_output(out("Auto-update: enabled\n"));
    rig.ops.rescan();
    rig.ops.wait_checks();
    let items = notices(&inbox);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].state, State::Withdrawn);
}

#[cfg(unix)]
#[test]
fn the_notice_says_where_workers_stay() {
    let (rig, inbox) = setup("vinbox-stay");
    pass(&rig);
    native_claude(&rig, "2.1.325", "claude 2.1.325");
    rig.launcher.push_output(out("9.9.9 (Claude Code)\n"));
    rig.launcher.push_output(out("Auto-update: enabled\n"));
    rig.ops.rescan();
    rig.ops.wait_checks();
    let items = notices(&inbox);
    assert_eq!(items.len(), 1);
    assert!(
        items[0].detail.ends_with("Workers stay on 2.1.324."),
        "{}",
        items[0].detail
    );
    assert_eq!(items[0].title, "Claude Code 2.1.325 Didn't Pass");
}
