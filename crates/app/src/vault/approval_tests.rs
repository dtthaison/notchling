use super::*;
use serde_json::json;

fn approval(id: &str) -> Approval {
    Approval::from_event(
        &json!({"id": id, "names": ["github", "npm"], "command": "gh api /user",
        "folder": "/Users/me/work"}),
    )
    .unwrap()
}

#[test]
fn the_prompt_says_the_swift_panels_words() {
    let p = Prompt::new(&approval("ap-1"));
    assert_eq!(p.id, "ap-1");
    assert_eq!(p.title, "Allow an agent to use github, npm?");
    assert_eq!(
        (p.folder.as_str(), p.command.as_str()),
        ("/Users/me/work", "gh api /user")
    );
    assert_eq!(p.explanation, EXPLANATION);
    assert_eq!(
        p.always_tip,
        "Stop asking for github, npm; you can turn it back on in the vault"
    );
    assert_eq!(
        (p.requested_by, p.warning, p.claimed_folder),
        (None, None, None)
    );
    assert_eq!(COMMAND, "Command it says it will run");
    let bare = Approval::from_event(&json!({"id": "ap-2", "names": ["aws"]})).unwrap();
    assert_eq!(Prompt::new(&bare).folder, "unknown folder");
    assert_eq!(
        pending(&[json!({"names": ["no id"]}), json!({"id": "a"})]).len(),
        1
    );
}

#[test]
fn the_daemons_verified_caller_is_shown_and_a_wrong_folder_warns() {
    let a = Approval::from_event(&json!({"id": "ap-1", "names": ["github"],
        "command": "gh api /user", "folder": "/Users/me/work",
        "verified": {"exe": "/bin/zsh", "pid": 42, "cwd": "/tmp/evil",
                     "parents": ["zsh", "claude", "Terminal"], "folder_matches": false}}))
    .unwrap();
    let p = Prompt::new(&a);
    assert_eq!(p.folder, "/tmp/evil", "where it really runs");
    assert_eq!(p.claimed_folder.as_deref(), Some("It names /Users/me/work"));
    assert_eq!(
        p.requested_by.as_deref(),
        Some("Requested by zsh ← claude ← Terminal")
    );
    assert_eq!(p.warning, Some(FOLDER_WARNING));
    let ok = Approval::from_event(&json!({"id": "ap-2", "names": ["github"],
        "folder": "/w", "verified": {"cwd": "/w", "parents": [], "folder_matches": true}}))
    .unwrap();
    let p = Prompt::new(&ok);
    assert_eq!(
        (p.warning, p.claimed_folder.clone(), p.requested_by.clone()),
        (None, None, None)
    );
    let unknown = Approval::from_event(&json!({"id": "ap-4", "names": ["github"],
        "folder": "/w", "verified": {"cwd": null, "parents": ["zsh"], "folder_matches": null}}))
    .unwrap();
    let p = Prompt::new(&unknown);
    assert_eq!(p.warning, None);
    assert_eq!(p.unchecked, Some(FOLDER_UNCHECKED));
    assert_eq!(p.folder, "/w");
    let old =
        Approval::from_event(&json!({"id": "ap-5", "names": ["github"], "folder": "/w"})).unwrap();
    assert_eq!(Prompt::new(&old).unchecked, Some(FOLDER_UNCHECKED));
    assert_eq!(Prompt::new(&ok).unchecked, None);
    let odd = Approval::from_event(&json!({"id": "ap-3", "verified": "yes"})).unwrap();
    assert_eq!(odd.verified, None);
}

#[test]
fn the_command_shows_its_newlines_and_hidden_characters_and_says_when_it_is_clipped() {
    assert_eq!(visible("echo a\necho b"), "echo a⏎\necho b");
    assert_eq!(visible("ls\u{202E}txt.exe"), "ls⟨U+202E⟩txt.exe");
    assert_eq!(visible("a\rb\u{200B}"), "a⟨U+000D⟩b⟨U+200B⟩");
    assert_eq!(visible("tab\there"), "tab\there");
    assert_eq!(command_size("gh api /user"), None);
    let long = (1..=12)
        .map(|i| format!("echo {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        command_size(&long).as_deref(),
        Some(
            format!(
                "12 lines, {} characters; scroll to read it all",
                long.chars().count()
            )
            .as_str()
        )
    );
    let wide = "x".repeat(500);
    assert_eq!(
        command_size(&wide).as_deref(),
        Some("1 line, 500 characters; scroll to read it all")
    );
    let p = Prompt::new(
        &Approval::from_event(&json!({"id": "a", "names": ["x"], "command": long})).unwrap(),
    );
    assert!(p.command.contains("⏎") && p.command_size.is_some());
}

#[test]
fn allow_waits_for_arming_after_the_window_shows_and_deny_never_does() {
    let mut m = ApprovalModel::new(&approval("ap-1"));
    assert!(!m.live(Decision::Once, 10_000), "not shown yet");
    m.shown(1_000);
    assert!(!m.live(Decision::Once, 1_000 + ARMING_MS - 1));
    assert!(!m.live(Decision::Always, 1_100));
    assert_eq!(m.answer(Decision::Once, 1_100), None, "a click too soon");
    assert!(m.live(Decision::Deny, 1_000));
    assert!(m.live(Decision::Once, 1_000 + ARMING_MS));
    assert_eq!(m.arms_at(), Some(1_000 + ARMING_MS));
    m.hidden();
    assert!(!m.live(Decision::Once, 5_000));
    m.shown(5_000);
    assert!(!m.live(Decision::Once, 5_100));
    assert!(m.live(Decision::Once, 5_000 + ARMING_MS));
}

#[test]
fn the_model_is_bound_to_its_approval_and_resolves_only_when_the_daemon_takes_it() {
    let mut m = ApprovalModel::new(&approval("ap-1"));
    assert_eq!(m.prompt.element("vault-once"), "vault-once-ap-1");
    m.shown(0);
    assert_eq!(
        m.answer(Decision::Once, ARMING_MS),
        Some(("ap-1".to_owned(), Decision::Once))
    );
    assert!(m.busy() && !m.resolved);
    assert_eq!(
        m.answer(Decision::Deny, ARMING_MS),
        None,
        "one answer at a time"
    );
    m.finished(Err("the vault is off".into()));
    assert!(!m.resolved && !m.busy());
    assert_eq!(m.note.as_deref(), Some("the vault is off"));
    assert_eq!(
        m.answer(Decision::Deny, ARMING_MS),
        Some(("ap-1".to_owned(), Decision::Deny))
    );
    m.finished(Ok(()));
    assert!(m.resolved);
    assert_eq!(m.answer(Decision::Once, ARMING_MS * 2), None, "resolved");
    assert_eq!(m.prompt.id, "ap-1", "never another approval");
}
