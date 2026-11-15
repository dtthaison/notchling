use serde_json::json;

use super::grants::{
    attachable, decide, expires_ms, GrantAsk, GrantDecision, SecretAllow, GRANT_TTL_MS,
};
use super::id::Tool;
use super::jobs::{Job, JobSpec, JobState};
use super::members::{Member, MemberState, Term};
use super::state::{apply, start, Change, Command, CommandError};
use super::testkit::{id, member};

fn allow(names: &[&str], any: bool) -> SecretAllow {
    SecretAllow::new(names.iter().map(|n| n.to_string()), any)
}

fn worker(names: &[&str], any: bool) -> Member {
    let mut m = member("w1", "linux", false);
    m.secrets = allow(names, any);
    m
}

fn job(state: JobState) -> Job {
    Job {
        id: "j-0123456789ab".to_string(),
        key: "k".to_string(),
        spec: JobSpec {
            node: id("w1"),
            tool: Tool::Codex,
            cwd: "/srv/repo".to_string(),
            args: json!({"prompt": "go"}),
            secrets: Vec::new(),
        },
        state,
        term: Term {
            n: 1,
            node: id("m1"),
        },
        created_ms: 0,
        session: None,
        detail: None,
    }
}

fn ask<'a>(node: &'a Member, job: &'a Job, name: &'a str) -> GrantAsk<'a> {
    GrantAsk {
        node,
        job,
        name,
        exists: true,
        asks: false,
        vault_is_primary: true,
        vault_name: "m1",
    }
}

#[test]
fn allowlist_is_empty_and_any_is_off_by_default() {
    let allow = SecretAllow::default();
    assert!(allow.names.is_empty());
    assert!(!allow.any);
    assert!(!allow.allows("github"));
    assert_eq!(member("w1", "linux", false).secrets, SecretAllow::default());
}

#[test]
fn old_member_json_without_secrets_still_loads() {
    let mut value = serde_json::to_value(member("w1", "linux", false)).expect("to json");
    let removed = value.as_object_mut().expect("object").remove("secrets");
    assert!(removed.is_some());
    let loaded: Member = serde_json::from_value(value).expect("old member");
    assert_eq!(loaded.secrets, SecretAllow::default());
    assert_eq!(loaded.name, "w1");
}

#[test]
fn name_off_the_allowlist_is_refused() {
    let node = worker(&["github"], false);
    let job = job(JobState::Running);
    assert_eq!(
        decide(&ask(&node, &job, "npm")),
        GrantDecision::Refuse("npm isn't allowed on w1".to_string())
    );
    assert_eq!(decide(&ask(&node, &job, "github")), GrantDecision::Release);
}

#[test]
fn any_allows_every_name() {
    let node = worker(&[], true);
    let job = job(JobState::Running);
    assert_eq!(
        decide(&ask(&node, &job, "anything")),
        GrantDecision::Release
    );
    assert_eq!(decide(&ask(&node, &job, "else")), GrantDecision::Release);
}

#[test]
fn missing_entry_is_refused_with_its_name() {
    let node = worker(&["github"], false);
    let job = job(JobState::Running);
    let mut a = ask(&node, &job, "github");
    a.exists = false;
    assert_eq!(
        decide(&a),
        GrantDecision::Refuse("No secret named github in the vault on m1".to_string())
    );
}

#[test]
fn ask_entry_needs_ask() {
    let node = worker(&["github"], false);
    let job = job(JobState::Dispatching);
    let mut a = ask(&node, &job, "github");
    a.asks = true;
    assert_eq!(decide(&a), GrantDecision::Ask);
}

#[test]
fn grant_waits_while_vault_node_isnt_primary() {
    let node = worker(&["github"], false);
    let job = job(JobState::Running);
    let mut a = ask(&node, &job, "github");
    a.vault_is_primary = false;
    a.exists = false;
    assert_eq!(
        decide(&a),
        GrantDecision::Wait("github waits for m1: the vault is there".to_string())
    );
}

#[test]
fn off_list_name_is_refused_even_while_waiting() {
    let node = worker(&["github"], false);
    let job = job(JobState::Running);
    let mut a = ask(&node, &job, "npm");
    a.vault_is_primary = false;
    assert_eq!(
        decide(&a),
        GrantDecision::Refuse("npm isn't allowed on w1".to_string())
    );
}

#[test]
fn finished_job_gets_nothing() {
    let node = worker(&[], true);
    for state in [
        JobState::Done,
        JobState::Failed,
        JobState::Interrupted,
        JobState::Lost,
    ] {
        let job = job(state);
        assert_eq!(
            decide(&ask(&node, &job, "github")),
            GrantDecision::Refuse("job j-0123456789ab isn't running".to_string()),
            "{state:?}"
        );
    }
}

#[test]
fn inactive_node_gets_nothing() {
    let mut node = worker(&[], true);
    let job = job(JobState::Running);
    for state in [MemberState::Pending, MemberState::Removed] {
        node.state = state;
        assert!(matches!(
            decide(&ask(&node, &job, "github")),
            GrantDecision::Refuse(_)
        ));
    }
}

#[test]
fn expiry_is_twelve_hours() {
    assert_eq!(GRANT_TTL_MS, 43_200_000);
    assert_eq!(expires_ms(1_000), 1_000 + 12 * 3_600_000);
    assert_eq!(expires_ms(i64::MAX), i64::MAX);
}

#[test]
fn dispatch_with_a_refused_attachment_fails_before_start() {
    let mut spec = job(JobState::Dispatching).spec;
    spec.secrets = vec!["github".to_string(), "npm".to_string(), "pypi".to_string()];
    assert_eq!(
        attachable(&spec, &allow(&["github"], false)),
        Err("npm isn't allowed on that machine".to_string())
    );
    assert_eq!(attachable(&spec, &allow(&[], true)), Ok(()));
    assert_eq!(
        attachable(&spec, &allow(&["github", "npm", "pypi"], false)),
        Ok(())
    );
    spec.secrets.clear();
    assert_eq!(attachable(&spec, &SecretAllow::default()), Ok(()));
}

#[test]
fn names_are_case_insensitive() {
    let stored = allow(&["GitHub", "NPM"], false);
    assert!(stored.names.contains("github"));
    assert!(stored.names.contains("npm"));
    assert!(stored.allows("GITHUB"));
    assert!(stored.allows("Npm"));
    assert!(!stored.allows("pypi"));
    let node = worker(&["GitHub"], false);
    let job = job(JobState::Running);
    assert_eq!(decide(&ask(&node, &job, "GITHUB")), GrantDecision::Release);
}

fn cluster_with_worker() -> super::state::ClusterState {
    let (mut state, _, _) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "home".to_string(),
            home: member("m1", "macos", true),
        },
        10,
    )
    .expect("create");
    let w = member("w1", "linux", false);
    apply(
        &mut state,
        Command::ProposeJoin {
            member: w.clone(),
            item: format!("node-join:{}", w.id),
        },
        20,
    )
    .expect("propose");
    apply(
        &mut state,
        Command::Approve {
            node: w.id,
            client: false,
            can_be_master: false,
        },
        30,
    )
    .expect("approve");
    state
}

#[test]
fn set_allow_is_journaled() {
    let mut state = cluster_with_worker();
    let before = state.seq;
    let applied = apply(
        &mut state,
        Command::SetAllow {
            node: id("w1"),
            allow: allow(&["GitHub"], false),
        },
        40,
    )
    .expect("set allow");
    let entry = applied.entry.expect("journaled");
    assert_eq!(entry.seq, before + 1);
    assert!(matches!(entry.command, Command::SetAllow { .. }));
    assert_eq!(state.members[&id("w1")].secrets, allow(&["github"], false));
    assert_eq!(applied.changes.len(), 1);
    match &applied.changes[0] {
        Change::Member(m) => assert_eq!(m.secrets, allow(&["github"], false)),
        other => panic!("{other:?}"),
    }
    apply(
        &mut state,
        Command::SetAllow {
            node: id("w1"),
            allow: allow(&[], true),
        },
        50,
    )
    .expect("any");
    assert!(state.members[&id("w1")].secrets.any);
}

#[test]
fn set_allow_refuses_unknown_and_removed_nodes() {
    let mut state = cluster_with_worker();
    let unknown = apply(
        &mut state,
        Command::SetAllow {
            node: id("nobody"),
            allow: allow(&[], true),
        },
        40,
    );
    assert_eq!(unknown, Err(CommandError::Unknown(id("nobody"))));
    apply(&mut state, Command::Remove { node: id("w1") }, 50).expect("remove");
    let removed = apply(
        &mut state,
        Command::SetAllow {
            node: id("w1"),
            allow: allow(&[], true),
        },
        60,
    );
    assert_eq!(removed, Err(CommandError::Removed(id("w1"))));
}

#[test]
fn proposed_member_starts_with_an_empty_allowlist() {
    let (mut state, _, _) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "home".to_string(),
            home: member("m1", "macos", true),
        },
        10,
    )
    .expect("create");
    let mut w = member("w1", "linux", false);
    w.secrets = allow(&["github", "npm"], true);
    apply(
        &mut state,
        Command::ProposeJoin {
            member: w.clone(),
            item: format!("node-join:{}", w.id),
        },
        20,
    )
    .expect("propose");
    assert_eq!(state.members[&id("w1")].secrets, SecretAllow::default());
    apply(
        &mut state,
        Command::Approve {
            node: w.id,
            client: false,
            can_be_master: false,
        },
        30,
    )
    .expect("approve");
    assert_eq!(state.members[&id("w1")].secrets, SecretAllow::default());
}

#[test]
fn set_allow_refuses_invalid_names_and_too_many() {
    let mut state = cluster_with_worker();
    let long = "a".repeat(65);
    for bad in ["", "1abc", "has space", "semi;colon", "ünï", long.as_str()] {
        let result = apply(
            &mut state,
            Command::SetAllow {
                node: id("w1"),
                allow: SecretAllow {
                    names: [bad.to_string()].into_iter().collect(),
                    any: false,
                },
            },
            40,
        );
        assert_eq!(result, Err(CommandError::BadTransition), "{bad:?}");
    }
    let many: Vec<String> = (0..257).map(|n| format!("n{n}")).collect();
    let result = apply(
        &mut state,
        Command::SetAllow {
            node: id("w1"),
            allow: SecretAllow::new(many.clone(), false),
        },
        40,
    );
    assert_eq!(result, Err(CommandError::BadTransition));
    assert_eq!(state.members[&id("w1")].secrets, SecretAllow::default());
    let ok = apply(
        &mut state,
        Command::SetAllow {
            node: id("w1"),
            allow: SecretAllow::new(many.into_iter().take(256), false),
        },
        50,
    );
    assert!(ok.is_ok());
    assert_eq!(state.members[&id("w1")].secrets.names.len(), 256);
}

#[test]
fn loaded_allowlist_is_lowercased() {
    let loaded: SecretAllow =
        serde_json::from_value(json!({"names": ["GitHub", "NPM"], "any": false})).expect("load");
    assert_eq!(loaded, allow(&["github", "npm"], false));
    assert!(loaded.names.contains("github"));
    let bare: SecretAllow = serde_json::from_value(json!({})).expect("empty");
    assert_eq!(bare, SecretAllow::default());
    let text = serde_json::to_string(&allow(&["GitHub"], true)).expect("json");
    let back: SecretAllow = serde_json::from_str(&text).expect("back");
    assert_eq!(back, allow(&["github"], true));
}

#[test]
fn invalid_requested_name_is_refused_first() {
    let node = worker(&[], true);
    let job = job(JobState::Running);
    for bad in ["", "a b", "../x", "1x"] {
        assert_eq!(
            decide(&ask(&node, &job, bad)),
            GrantDecision::Refuse(format!("{bad} isn't a valid secret name")),
            "{bad:?}"
        );
    }
    let long = "x".repeat(200);
    let mut a = ask(&node, &job, &long);
    a.vault_is_primary = false;
    match decide(&a) {
        GrantDecision::Refuse(text) => {
            assert_eq!(
                text,
                format!("{} isn't a valid secret name", "x".repeat(64))
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn missing_entry_is_refused_before_asking() {
    let node = worker(&["github"], false);
    let job = job(JobState::Running);
    let mut a = ask(&node, &job, "github");
    a.exists = false;
    a.asks = true;
    assert_eq!(
        decide(&a),
        GrantDecision::Refuse("No secret named github in the vault on m1".to_string())
    );
}

#[test]
fn journaled_mixed_case_allow_replays_as_lowercase() {
    let mut state = cluster_with_worker();
    let applied = apply(
        &mut state,
        Command::SetAllow {
            node: id("w1"),
            allow: SecretAllow {
                names: ["GitHub".to_string(), "NPM".to_string()]
                    .into_iter()
                    .collect(),
                any: false,
            },
        },
        40,
    )
    .expect("set");
    let entry = applied.entry.expect("entry");
    let mut replayed = cluster_with_worker();
    apply(&mut replayed, entry.command, 40).expect("replay");
    assert_eq!(
        replayed.members[&id("w1")].secrets,
        allow(&["github", "npm"], false)
    );
    assert!(replayed.members[&id("w1")].secrets.names.contains("github"));
}
