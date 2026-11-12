//! The permission card read from a job's state (`approve <tool>: <what>`): what it offers, how its
//! words are split, and how a refusal that the permission is gone is told apart. A card for a file
//! edit offered "Always allow this", which the terminal's dialog for one never has ("Yes, allow all
//! edits during this session" is its only longer answer), so that button could only be refused.

use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};

use super::prompt_flow::{already_answered, is_gone};
use super::prompts::permission_from_needs;

fn waiting(needs: &str) -> Agent {
    Agent {
        id: "job1".into(),
        name: "job".into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group: AgentGroup::NeedsInput,
        label: "Needs input".into(),
        detail: Some(needs.into()),
        since_ms: 1,
        target: AgentTarget::Job("job1".into()),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
    }
}

#[test]
fn a_file_edit_is_not_offered_always_since_its_dialog_has_no_such_option() {
    for tool in ["Write", "Edit", "MultiEdit", "NotebookEdit"] {
        let r = permission_from_needs(&waiting(&format!("approve {tool}: /w/p/memory/notes.md")))
            .expect("a card");
        assert!(!r.can_always_allow, "{tool}");
    }
    // A command's dialog has "Yes, and don't ask again for …"; the daemon still refuses it, with
    // nothing typed, when this one doesn't.
    for needs in [
        "approve Bash: ls -la",
        "approve WebFetch: https://example.com",
        "approve mcp__srv__do: {}",
    ] {
        let r = permission_from_needs(&waiting(needs)).expect("a card");
        assert!(r.can_always_allow, "{needs}");
    }
}

#[test]
fn the_tool_is_the_first_word_with_or_without_a_colon() {
    for needs in [
        "approve Write: /Users/me/a b/notes.md",
        "approve Write /Users/me/a b/notes.md",
    ] {
        let r = permission_from_needs(&waiting(needs)).expect("a card");
        assert_eq!(r.tool_name, "Write", "{needs}");
        assert_eq!(r.summary, "/Users/me/a b/notes.md", "{needs}");
    }
    let r = permission_from_needs(&waiting("approve Bash: echo a: b")).expect("a card");
    assert_eq!(
        (r.tool_name.as_str(), r.summary.as_str()),
        ("Bash", "echo a: b")
    );
    let bare = permission_from_needs(&waiting("approve Edit")).expect("a card");
    assert_eq!(
        (bare.tool_name.as_str(), bare.summary.as_str()),
        ("Edit", "")
    );
}

#[test]
fn a_refusal_that_the_permission_is_gone_means_it_was_answered_elsewhere() {
    use notchling_engine::reply::exec::Failure;
    // What the daemon says when the terminal shows no such dialog: the card goes, with a note.
    assert!(is_gone(&Failure::NotPermission.to_string()));
    assert!(!is_gone(&Failure::PermissionMismatch.to_string()));
    assert!(!is_gone(&Failure::NoAlways.to_string()));
    assert!(!is_gone(&Failure::PermissionStillOpen.to_string()));
    assert!(!is_gone("the daemon didn't answer in time"));
    assert_eq!(
        already_answered("Write", "/w/memory/notes.md"),
        "Already answered in the terminal: Write notes.md"
    );
}
