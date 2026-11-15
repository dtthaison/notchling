use std::path::Path;

use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::transcript::model::{Conversation, ItemKind};
use serde_json::json;

use super::data::{DataRoot, Session, Tool};
use super::remote::{
    bring_back_job, brought_back, job_of_worktree, shell_conversation, source, waiting_for_mirror,
    Source, LOG_TAIL_BYTES,
};
use crate::acting::Request;
use crate::cluster::renders::WORKER;
use crate::testutil::Scratch;
use crate::transcripts::Locator;

fn agent(id: &str, path: Option<&Path>) -> Agent {
    Agent {
        id: id.into(),
        name: "job".into(),
        project: "api".into(),
        cwd: "/srv/api".into(),
        group: AgentGroup::Working,
        label: "Working".into(),
        detail: None,
        since_ms: 1,
        target: AgentTarget::Job("j1".into()),
        suggested_reply: None,
        transcript_path: path.map(|p| p.to_string_lossy().into_owned()),
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
    }
}

fn session(agent: Agent, transcript: Option<std::path::PathBuf>) -> Session {
    Session {
        agent,
        tool: Tool::Claude,
        transcript,
    }
}

fn remote(id: &str) -> String {
    format!("{WORKER}/{id}")
}

#[test]
fn remote_row_uses_mirror_transcript() {
    let s = Scratch::new("remote-mirror");
    let mirror = s.write(
        &format!("data/cluster/mirror/{WORKER}/0123456789abcdef.jsonl"),
        "{}\n",
    );
    let owner = s.write("claude/projects/-srv-api/s1.jsonl", "{}\n");
    let projects = s.path().join("claude/projects");
    let mut locator = Locator::new(None);

    let mirrored = agent(&remote("j1"), Some(&mirror));
    assert_eq!(source(&mirrored), Source::Mirror(mirror.clone()));
    assert_eq!(locator.find(&mirrored, &projects, 0), Some(mirror.clone()));

    let owners = agent(&remote("j2"), Some(&owner));
    assert_eq!(source(&owners), Source::Pending, "never the owner's path");
    assert_eq!(locator.find(&owners, &projects, 0), None);

    let not_yet = agent(&remote("s1"), None);
    assert_eq!(source(&not_yet), Source::Pending);
    assert_eq!(
        locator.find(&not_yet, &projects, 0),
        None,
        "not even a local file with the same name"
    );
    assert!(waiting_for_mirror(&session(not_yet, None)));
    assert!(waiting_for_mirror(&session(owners, None)));
    assert!(waiting_for_mirror(&session(mirrored.clone(), None)));
    assert!(!waiting_for_mirror(&session(mirrored, Some(mirror))));

    let local = agent("j9", Some(&owner));
    assert_eq!(source(&local), Source::Local);
    assert_eq!(locator.find(&local, &projects, 0), Some(owner));
    assert!(!waiting_for_mirror(&session(local, None)));
}

#[test]
fn a_path_with_dots_or_a_relative_one_is_not_the_mirror() {
    let odd = "/data/cluster/mirror/../../../etc/passwd";
    assert_eq!(
        source(&agent(&remote("j1"), Some(Path::new(odd)))),
        Source::Pending
    );
    let relative = Path::new("data/cluster/mirror/x.jsonl");
    assert_eq!(
        source(&agent(&remote("j1"), Some(relative))),
        Source::Pending,
        "absolute only"
    );
}

fn shell_agent(id: &str, log: &Path) -> Agent {
    let mut row = agent(id, Some(log));
    row.command = Some("make test".into());
    row.target = AgentTarget::Process(42);
    row.host = "Shell".into();
    row
}

fn text_of(conversation: &Conversation) -> String {
    match &conversation.items[0].kind {
        ItemKind::Assistant { text } => text.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn shell_job_row_shows_log_tail() {
    let s = Scratch::new("remote-shell");
    let lines: Vec<String> = (0..6_000)
        .map(|n| format!("line {n:05} of the build"))
        .collect();
    let log = s.write("data/shell/j-1.log", &(lines.join("\n") + "\n"));
    let row = shell_agent("shell:j-1", &log);
    let mut locator = Locator::new(None);
    let found = locator.find(&row, &s.path().join("p"), 0);
    assert_eq!(found.as_deref(), Some(log.as_path()));
    let root = DataRoot::fixture(s.path());
    let conversation = root.conversation_shared(&session(row, found));
    assert_eq!(conversation.items.len(), 1);
    let text = text_of(&conversation);
    assert!(text.starts_with("```\nline "), "{}", &text[..20]);
    assert!(
        text.ends_with("line 05999 of the build\n```"),
        "{}",
        &text[text.len() - 40..]
    );
    assert!(text.len() as u64 <= LOG_TAIL_BYTES + 16);
    assert!(!text.contains("line 00000 "), "only the tail");
    let first = text.lines().nth(1).expect("a line");
    assert!(first.ends_with(" of the build"), "no cut line: {first}");

    let mirror = s.write(
        &format!("data/cluster/mirror/{WORKER}/aaaaaaaaaaaaaaaa.log"),
        "one\ntwo\n",
    );
    let there = shell_agent(&remote("shell:j-2"), &mirror);
    let found = locator.find(&there, &s.path().join("p"), 0);
    assert_eq!(found.as_deref(), Some(mirror.as_path()));
    let conversation = root.conversation_shared(&session(there, found));
    assert_eq!(text_of(&conversation), "```\none\ntwo\n```");

    let waiting = shell_agent(&remote("shell:j-3"), Path::new("/owner/log"));
    assert_eq!(locator.find(&waiting, &s.path().join("p"), 0), None);
}

#[test]
fn a_log_with_backticks_gets_a_longer_fence_and_an_empty_one_shows_nothing() {
    let conversation = shell_conversation("a\n```\nb\n");
    assert_eq!(text_of(&conversation), "````\na\n```\nb\n````");
    assert!(shell_conversation("").items.is_empty());
    assert!(shell_conversation("\n\n").items.is_empty());
}

#[test]
fn bring_back_parses_job_from_worktree_cwd() {
    for (cwd, job) in [
        (
            "/home/u/.local/share/notchling/cluster/work/jobs/j-0123456789ab",
            "j-0123456789ab",
        ),
        (
            "/Users/me/Library/Application Support/Notchling/cluster/work/jobs/j-1/src/lib",
            "j-1",
        ),
        ("/d/cluster/work/jobs/j-2/", "j-2"),
        ("/cluster/work/jobs/job_3", "job_3"),
    ] {
        assert_eq!(job_of_worktree(cwd).as_deref(), Some(job), "{cwd}");
    }
}

#[test]
fn bring_back_refuses_odd_paths() {
    for cwd in [
        "",
        "relative/cluster/work/jobs/j-1",
        "/d/cluster/work/jobs",
        "/d/cluster/work/jobs/",
        "/d/cluster/work/jobs//j-1",
        "/d/cluster/work/jobs/../j-1",
        "/d/cluster/work/jobs/j-1/../../x",
        "/d/cluster/work/jobs/./j-1",
        "/d/cluster/work/jobs/-j1",
        "/d/cluster/work/jobs/j 1",
        "/d/cluster/work/jobs/j.1",
        "/d/cluster/work/jobs/j\0",
        "/d/cluster/work/other/j-1",
        "/d/work/jobs/j-1",
        "/d/cluster/jobs/j-1",
        "/srv/api",
    ] {
        assert_eq!(job_of_worktree(cwd), None, "{cwd:?}");
    }
    let mut local = agent("j1", None);
    local.cwd = "/d/cluster/work/jobs/j-1".into();
    assert_eq!(
        bring_back_job(&local),
        None,
        "only a session on another node"
    );
    let mut there = agent(&remote("j1"), None);
    there.cwd = "/d/cluster/work/jobs/j-1".into();
    assert_eq!(bring_back_job(&there).as_deref(), Some("j-1"));
}

#[test]
fn bring_back_sends_cluster_return() {
    let request = Request::cluster_return("j-1");
    assert_eq!(request.op, "cluster.return");
    assert_eq!(request.args, json!({"job": "j-1"}));
    let reply = json!({"job": "j-1", "branch": "notchling/mac-mini/j-1", "node": "mac-mini"});
    assert_eq!(
        brought_back(&reply).as_deref(),
        Some("notchling/mac-mini/j-1")
    );
    assert_eq!(brought_back(&json!({"job": "j-1"})), None);
    assert_eq!(brought_back(&json!({"branch": ""})), None);
}
