use serde_json::json;

use super::core_tests::{answers, dispatch_cmd, pair, run_locals, Hub};
use super::{Effect, Input, LocalCmd, NodeCore};
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::jobs::job_id;
use crate::cluster::join::JoinToken;
use crate::cluster::members::{Member, MemberState};
use crate::cluster::state::{apply, start, Command};
use crate::cluster::testkit::{fp, id, member, versions};
use crate::cluster::text::{unseen, NAME_MAX};
use crate::cluster::wire::{Msg, WireError};

const HIDDEN: [char; 12] = [
    '\u{e0041}',
    '\u{e007f}',
    '\u{fe0f}',
    '\u{e0100}',
    '\u{e01ef}',
    '\u{2028}',
    '\u{2029}',
    '\u{34f}',
    '\u{115f}',
    '\u{1160}',
    '\u{3164}',
    '\u{ffa0}',
];

fn hostile() -> String {
    let mut text = String::from("ok");
    for c in HIDDEN {
        text.push(c);
        text.push('x');
    }
    text.push_str("\u{202e}\u{1b}[2J\u{200b}");
    text.push_str(&"y".repeat(1 << 12));
    text
}

fn is_clean(text: &str, max: usize) -> bool {
    text.chars().count() <= max && !text.chars().any(unseen)
}

fn started(hub: &mut Hub, key: &str) -> (NodeId, NodeId, u64) {
    let m5 = id("m5");
    let w1 = id("w1");
    hub.local(&m5, dispatch_cmd(key, "w1", Tool::Shell));
    let w1_effects: Vec<Effect> = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "w1")
        .map(|(_, e)| e.clone())
        .collect();
    let (call, op, _) = run_locals(&w1_effects).last().cloned().expect("a run");
    assert_eq!(op, "shell.run");
    (m5, w1, call)
}

#[test]
fn a_job_events_detail_is_cleaned_and_capped_before_it_is_journaled_or_published() {
    let (mut hub, m5, w1) = pair();
    let (_, _, call) = started(&mut hub, "k");
    hub.run(
        &w1,
        Input::LocalDone {
            call,
            result: Ok(json!({"session": "shell:j"})),
        },
    );
    let event = Msg::Event {
        topic: "jobs".to_string(),
        name: "job.state".to_string(),
        data: json!({"job": job_id("k"), "state": "failed", "detail": hostile()}),
    };
    hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: event,
        },
    );
    let job = &hub.core(&m5).state().expect("state").jobs[&job_id("k")];
    let detail = job.detail.clone().expect("a detail");
    assert!(is_clean(&detail, 200), "{detail:?}");
    assert!(detail.starts_with("ok"));
    let shown = hub.core(&m5).status();
    let listed = shown["jobs"][0]["detail"].as_str().expect("detail");
    assert_eq!(listed, detail);
}

#[test]
fn a_failed_starts_message_is_cleaned_before_it_becomes_a_jobs_detail() {
    let (mut hub, m5, w1) = pair();
    let (_, _, call) = started(&mut hub, "k");
    hub.run(
        &w1,
        Input::LocalDone {
            call,
            result: Err(WireError::new("failed", hostile())),
        },
    );
    let detail = hub.core(&m5).state().expect("state").jobs[&job_id("k")]
        .detail
        .clone()
        .expect("a detail");
    assert!(is_clean(&detail, 200), "{detail:?}");
}

#[test]
fn the_session_a_peer_reports_is_cleaned_and_capped_whichever_field_carries_it() {
    for field in ["session", "id", "jobId"] {
        let (mut hub, m5, w1) = pair();
        let (_, _, call) = started(&mut hub, "k");
        let effects = hub.run(
            &w1,
            Input::LocalDone {
                call,
                result: Ok(json!({field: hostile()})),
            },
        );
        assert!(!effects.is_empty());
        let job = &hub.core(&m5).state().expect("state").jobs[&job_id("k")];
        let session = job.session.clone().expect("a session");
        assert!(is_clean(&session, 200), "{field}: {session:?}");
        assert!(session.starts_with("ok"));
        let body = hub
            .seen
            .iter()
            .filter(|(n, _)| n == "m5")
            .find_map(|(_, e)| match e {
                Effect::Answer { result: Ok(v), .. } if v["job"] == json!(job_id("k")) => {
                    Some(v.clone())
                }
                _ => None,
            })
            .expect("the caller hears the job");
        let shown = body["session"].as_str().expect("session");
        assert_eq!(shown, session);
        let qualified = body["qualified"].as_str().expect("qualified");
        assert_eq!(qualified, format!("{w1}/{session}"));
        let listed = hub.core(&m5).status()["jobs"][0]["session"].clone();
        assert_eq!(listed, json!(session));
    }
}

fn join_msg(name: &str, who: &str, secret: &str) -> Msg {
    Msg::Join {
        secret: secret.to_string(),
        name: name.to_string(),
        fingerprint: fp(who),
        addrs: vec!["100.64.0.9".to_string()],
        port: 47823,
        versions: versions("linux"),
        headless: false,
    }
}

fn issue(hub: &mut Hub, home: &NodeId, secret: &str) {
    hub.local(
        home,
        LocalCmd::IssueToken {
            secret: secret.to_string().into(),
            minter: home.clone(),
        },
    );
}

#[test]
fn a_joiners_name_is_cleaned_before_it_is_journaled_asked_about_or_shown() {
    let (mut hub, m5, _) = pair();
    issue(&mut hub, &m5, "t1");
    let joiner = id("w9");
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: joiner.clone(),
            msg: join_msg(&hostile(), "w9", "t1"),
        },
    );
    let stored = hub.core(&m5).state().expect("state").members[&joiner]
        .name
        .clone();
    assert!(is_clean(&stored, NAME_MAX), "{stored:?}");
    assert!(stored.starts_with("ok"));
    let asked: Vec<&String> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::JoinRequest { name, .. } => Some(name),
            _ => None,
        })
        .collect();
    assert_eq!(asked, [&stored]);
    let shown = hub.core(&m5).status();
    let listed = shown["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|m| m["id"] == json!(joiner))
        .expect("the joiner is listed");
    assert_eq!(listed["name"], json!(stored));
}

#[test]
fn a_joiner_whose_name_is_only_hidden_characters_is_known_by_its_id() {
    let (mut hub, m5, _) = pair();
    issue(&mut hub, &m5, "t1");
    let joiner = id("w9");
    let blank: String = HIDDEN.iter().collect();
    hub.run(
        &m5,
        Input::Msg {
            from: joiner.clone(),
            msg: join_msg(&blank, "w9", "t1"),
        },
    );
    let stored = &hub.core(&m5).state().expect("state").members[&joiner].name;
    assert_eq!(*stored, joiner.to_string());
}

fn rows_with_hostile_names(hub: &Hub, from: &NodeId) -> Vec<Member> {
    hub.core(from)
        .state()
        .expect("state")
        .members
        .values()
        .cloned()
        .map(|mut m| {
            m.name = format!("{}{}", m.name, hostile());
            m
        })
        .collect()
}

#[test]
fn member_names_in_the_primarys_push_are_cleaned_before_they_are_journaled_or_published() {
    let (mut hub, m5, w1) = pair();
    let term = hub.core(&m5).state().expect("state").term.clone();
    let members = rows_with_hostile_names(&hub, &m5);
    let push = Msg::Members {
        home: m5.clone(),
        term,
        vault: None,
        members,
        failover: None,
        armed: false,
        claims: Vec::new(),
        side_copies: Vec::new(),
    };
    hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: push.clone(),
        },
    );
    let state = hub.core(&w1).state().expect("state");
    for m in state.members.values() {
        assert!(is_clean(&m.name, NAME_MAX), "{:?}", m.name);
    }
    assert!(state.members[&m5].name.starts_with("m5ok"));
    let shown = hub.core(&w1).status();
    for m in shown["members"].as_array().expect("members") {
        assert!(is_clean(m["name"].as_str().expect("name"), NAME_MAX));
    }
    let again = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: push,
        },
    );
    assert!(
        again
            .iter()
            .all(|e| !matches!(e, Effect::Persist { changes, .. } if !changes.is_empty())),
        "the same push again changes no member: {again:?}"
    );
}

#[test]
fn the_cluster_and_member_names_in_a_join_answer_are_cleaned() {
    let (mut hub, m5, _) = pair();
    let w3 = hub.add("w3", "linux", false);
    let token = JoinToken {
        cluster: "c1".to_string(),
        home: m5.clone(),
        fingerprint: fp("m5"),
        addrs: vec![],
        port: 1,
        secret: "s".to_string(),
    };
    hub.local(
        &w3,
        LocalCmd::JoinWith {
            token,
            me: member("w3", "linux", false),
        },
    );
    let mut members = rows_with_hostile_names(&hub, &m5);
    let mut me = member("w3", "linux", false);
    me.state = MemberState::Active;
    me.name = format!("w3{}", hostile());
    members.push(me);
    let state = hub.core(&m5).state().expect("state");
    let term = state.term.clone();
    hub.run(
        &w3,
        Input::Msg {
            from: m5.clone(),
            msg: Msg::JoinOk {
                cluster: "c1".to_string(),
                name: format!("lab{}", hostile()),
                term,
                home: m5.clone(),
                vault: None,
                members,
                failover: None,
            },
        },
    );
    let joined = hub.core(&w3).state().expect("joined");
    assert!(is_clean(&joined.name, NAME_MAX), "{:?}", joined.name);
    assert!(joined.name.starts_with("labok"));
    for m in joined.members.values() {
        assert!(is_clean(&m.name, NAME_MAX), "{:?}", m.name);
    }
    let shown = hub.core(&w3).status();
    assert!(is_clean(
        shown["cluster"]["name"].as_str().expect("name"),
        NAME_MAX
    ));
}

fn core_holding(raw_name: &str) -> (NodeCore, NodeId) {
    let home = member("m5", "macos", true);
    let (mut state, _, _) = start(
        Command::Create {
            id: "c1".to_string(),
            name: raw_name.to_string(),
            home,
        },
        0,
    )
    .expect("a cluster");
    let mut worker = member("w1", "linux", false);
    worker.name = raw_name.to_string();
    let rows = vec![state.members[&id("m5")].clone(), worker];
    let term = state.term.clone();
    let vault = state.vault.clone();
    apply(
        &mut state,
        Command::Sync {
            id: "c1".to_string(),
            name: raw_name.to_string(),
            term,
            home: id("m5"),
            vault,
            members: rows,
        },
        0,
    )
    .expect("a synced member list");
    (
        NodeCore::new(id("m5"), versions("macos"), Some(state), true),
        id("w1"),
    )
}

#[test]
fn a_name_already_in_the_state_is_cleaned_where_it_is_published() {
    let (core, w1) = core_holding(&hostile());
    let shown = core.status();
    assert!(is_clean(
        shown["cluster"]["name"].as_str().expect("name"),
        NAME_MAX
    ));
    let listed = shown["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|m| m["id"] == json!(w1))
        .expect("the worker is listed");
    assert!(is_clean(listed["name"].as_str().expect("name"), NAME_MAX));
    assert!(listed["name"].as_str().expect("name").starts_with("ok"));
}

#[test]
fn a_name_already_in_the_state_is_cleaned_where_an_error_shows_it() {
    let (mut core, _) = core_holding(&hostile());
    let effects = core.step(Input::Local {
        id: 1,
        cmd: dispatch_cmd("k", "w1", Tool::Shell),
    });
    let shown = answers(&effects);
    let Some(Err(err)) = shown.first() else {
        panic!("the caller hears why: {effects:?}");
    };
    assert!(err.message.ends_with("is offline"), "{}", err.message);
    assert!(is_clean(&err.message, NAME_MAX + " is offline".len()));
    assert!(err.message.starts_with("ok"));
}
