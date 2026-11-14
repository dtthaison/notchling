use std::sync::Arc;
use std::time::Duration;

use super::registry::MAX_DEPTH;
use super::subagents::{Roots, SessionRef};
use super::{Registry, Tool};
use crate::delegate::options::{Options, Tool as DelegateTool};
use crate::delegate::record::{self, Delegation};
use crate::sessions::testutil::TempDir;
use crate::tools::codex::children::DayFolders;
use crate::transcript::codex::testutil::{tree, FORKED, PARENT};
use crate::transcript::subagents::Via;
use crate::transcript::util::now_secs;

fn registry() -> Registry {
    Registry::with_sources(Arc::new(DayFolders))
}

fn top(dir: &TempDir) -> SessionRef {
    let id = format!("codex:{}", PARENT.thread);
    SessionRef {
        id: id.clone(),
        tool: Tool::Codex,
        transcript: Some(std::path::PathBuf::from(PARENT.path_in(dir, "sessions"))),
        name: String::new(),
        finished: Arc::default(),
        session: id,
    }
}

fn roots(dir: &TempDir) -> Roots {
    Roots {
        codex_sessions: dir.path("sessions"),
        antigravity: dir.path("agy"),
        antigravity_copy: dir.path("agy-copy"),
        delegations: dir.path("delegations"),
    }
}

fn thread(n: usize) -> String {
    format!("01a1d5da-0000-7000-8000-000000000{n:03}")
}

fn rollout(dir: &TempDir, thread: &str) {
    dir.write(
        &format!("sessions/2026/11/14/rollout-2026-11-14T09-00-00-{thread}.jsonl"),
        &format!(
            "{{\"timestamp\":\"2026-11-14T09:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{thread}\",\"cwd\":\"/w\"}}}}\n"
        ),
    );
}

fn job(dir: &TempDir, parent: &str, session: &str, n: usize) {
    let options = Options {
        tool: DelegateTool::Codex,
        prompt: format!("job {n}"),
        model: None,
        effort: None,
        folder: "/w".into(),
        out: None,
        write: false,
        session: Some(session.into()),
        timeout: Duration::from_secs(30),
    };
    let mut record = Delegation::new(
        parent,
        "/w",
        &options,
        now_secs() - 100.0 + n as f64,
        std::process::id(),
    );
    record.session = Some(session.into());
    record::write(&dir.path("delegations"), &record).unwrap();
}

#[test]
fn a_codex_child_with_a_job_of_its_own_is_a_tree() {
    let dir = tree("reg-tree");
    let job_thread = thread(1);
    rollout(&dir, &job_thread);
    job(&dir, FORKED.thread, &job_thread, 1);
    let rows = registry().subagents(&top(&dir), &roots(&dir), now_secs());
    let child = rows
        .iter()
        .find(|r| r.id == format!("codex:{}", FORKED.thread))
        .expect("the child");
    assert_eq!(
        (child.depth, child.parent.as_str()),
        (1, top(&dir).id.as_str())
    );
    let nested = rows
        .iter()
        .find(|r| r.id.starts_with("job:"))
        .expect("the job");
    assert_eq!(nested.depth, 2);
    assert_eq!(nested.parent, child.id);
    assert_eq!(nested.origin.via, Via::Delegated);
    assert!(rows.iter().all(|r| r.session == top(&dir).id));
    let at = |id: &str| rows.iter().position(|r| r.id == id).unwrap();
    assert!(
        at(&child.id) < at(&nested.id),
        "a parent comes before its rows"
    );
}

#[test]
fn a_loop_is_listed_once() {
    let dir = tree("reg-loop");
    let other = thread(2);
    rollout(&dir, &other);
    job(&dir, PARENT.thread, &other, 1);
    job(&dir, &other, PARENT.thread, 2);
    let rows = registry().subagents(&top(&dir), &roots(&dir), now_secs());
    let mut ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    let all = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), all, "{ids:?}");
    assert_eq!(rows.iter().filter(|r| r.id.starts_with("job:")).count(), 2);
}

#[test]
fn depth_stops_at_ten() {
    let dir = tree("reg-depth");
    let mut parent = PARENT.thread.to_owned();
    for n in 1..=12 {
        let session = thread(10 + n);
        rollout(&dir, &session);
        job(&dir, &parent, &session, n);
        parent = session;
    }
    let rows = registry().subagents(&top(&dir), &roots(&dir), now_secs());
    assert_eq!(rows.iter().map(|r| r.depth).max(), Some(MAX_DEPTH));
    assert_eq!(rows.iter().filter(|r| r.id.starts_with("job:")).count(), 10);
}

#[test]
fn a_gap_without_a_flow_lists_only_jobs() {
    let dir = tree("reg-gap");
    let mut parent = top(&dir);
    parent.id = "antigravity:conv-1".into();
    parent.session = parent.id.clone();
    parent.tool = Tool::Antigravity;
    parent.transcript = None;
    job(&dir, "conv-1", &thread(3), 1);
    let rows = registry().subagents(&parent, &roots(&dir), now_secs());
    assert_eq!(rows.len(), 1);
    assert!(rows[0].id.starts_with("job:"));
}

#[test]
fn glance_routes_by_the_rows_tool() {
    let dir = tree("reg-glance");
    let job_thread = thread(4);
    dir.write(
        &format!("sessions/2026/11/14/rollout-2026-11-14T09-00-00-{job_thread}.jsonl"),
        &FORKED.text.replace(FORKED.thread, &job_thread),
    );
    job(&dir, PARENT.thread, &job_thread, 1);
    let registry = registry();
    let rows = registry.subagents(&top(&dir), &roots(&dir), now_secs());
    for row in &rows {
        let glance = registry.glance(row);
        assert!(glance.is_some(), "{}", row.id);
        assert!(registry.open(row).is_some(), "{}", row.id);
    }
    let missing = rows
        .iter()
        .find(|r| r.id.starts_with("job:"))
        .map(|r| {
            let mut gone = r.clone();
            gone.path.clear();
            gone
        })
        .unwrap();
    assert!(registry.glance(&missing).is_none());
}
