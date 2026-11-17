use crate::acting_ops_tests::SESSION;
use crate::ask_find::{Seams, Subscription};
use crate::ask_testkit::{
    asker_at, base, changes, item_of, items, new_row, open, open_with, rig, until, Rig, ASKER,
    SHELL, T0,
};
use crate::event_assert::{of, text};
use crate::op_group::OpGroup;
use notchling_engine::ask::{AskState, Asker, Found, FIND_MS};
use notchling_engine::sessions::iso::parse_iso_ms;
use notchling_platform::watch::Batch;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAIN: &str = include_str!("../../engine/tests/fixtures/ask/main.jsonl");
const A1: &str = include_str!("../../engine/tests/fixtures/ask/subagents/agent-a1.jsonl");
const A1_META: &str = include_str!("../../engine/tests/fixtures/ask/subagents/agent-a1.meta.json");
const A2: &str = include_str!("../../engine/tests/fixtures/ask/subagents/agent-a2.jsonl");
const A2_META: &str = include_str!("../../engine/tests/fixtures/ask/subagents/agent-a2.meta.json");

const EXE: &str = "/Applications/Notchling.app/Contents/MacOS/notchling";
const TASKS: &str =
    "/private/tmp/claude-501/-Users-dev-work/11111111-2222-4333-8444-555555555555/tasks";
const CODEX: u32 = 1010;
const DELEGATE: u32 = 1011;

struct Lab {
    r: Rig,
    senders: Arc<Mutex<Vec<Sender<Batch>>>>,
    held: Arc<AtomicBool>,
    reads: Arc<AtomicUsize>,
}

fn at(stamp: &str) -> i64 {
    parse_iso_ms(stamp).unwrap()
}

fn before_result(text: &str) -> String {
    let mut kept: Vec<&str> = text
        .lines()
        .take_while(|line| !line.contains("tool_result"))
        .collect();
    kept.push("");
    kept.join("\n")
}

fn bash_line(stamp: &str, id: &str, command: &str) -> String {
    let line = json!({
        "type": "assistant",
        "timestamp": stamp,
        "message": {"content": [{
            "type": "tool_use", "id": id, "name": "Bash", "input": {"command": command}
        }]}
    });
    format!("{line}\n")
}

fn projects(r: &Rig) -> PathBuf {
    r.dir.0.join("claude/projects/-Users-dev-work")
}

fn put(path: PathBuf, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn subagent_file(r: &Rig, name: &str) -> PathBuf {
    projects(r)
        .join(SESSION)
        .join("subagents")
        .join(format!("agent-{name}.jsonl"))
}

fn lab(a1: &str, a2: &str) -> Lab {
    let r = rig();
    put(projects(&r).join(format!("{SESSION}.jsonl")), MAIN);
    put(subagent_file(&r, "a1"), a1);
    put(subagent_file(&r, "a2"), a2);
    let meta = projects(&r).join(SESSION).join("subagents");
    put(meta.join("agent-a1.meta.json"), A1_META);
    put(meta.join("agent-a2.meta.json"), A2_META);
    r.clock
        .store(at("2026-11-16T10:01:11.500Z"), Ordering::SeqCst);
    r.tree.runs(
        ASKER,
        &[
            EXE,
            "ask",
            "--choice",
            "Keep it",
            "--choice",
            "Drop it",
            "--",
            "Keep the old API?",
        ],
    );
    r.tree
        .writes_to(ASKER, Path::new(&format!("{TASKS}/ba1000001.output")));
    let lab = Lab {
        r,
        senders: Arc::new(Mutex::new(Vec::new())),
        held: Arc::new(AtomicBool::new(false)),
        reads: Arc::new(AtomicUsize::new(0)),
    };
    attach(&lab);
    lab
}

fn attach(l: &Lab) {
    let system = Seams::system();
    let (senders, held, reads) = (l.senders.clone(), l.held.clone(), l.reads.clone());
    let seams = Seams {
        read: Arc::new(move |path| {
            while held.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            reads.fetch_add(1, Ordering::SeqCst);
            (system.read)(path)
        }),
        watch: Arc::new(move |_| {
            let (tx, rx) = channel();
            senders.lock().unwrap().push(tx);
            Some(Subscription::new(rx, Box::new(())))
        }),
        exe: PathBuf::from(EXE),
        main_find_ms: 100,
    };
    l.r.ops.attach_finder_with(seams);
}

impl Lab {
    fn fire(&self) {
        for sender in self.senders.lock().unwrap().iter() {
            let _ = sender.send(Batch::default());
        }
    }

    fn first_read_done(&self) {
        until("the first read", || self.reads.load(Ordering::SeqCst) >= 3);
    }

    fn row(&self, id: &str) -> notchling_engine::ask::Row {
        self.r.store.get(id).unwrap().unwrap()
    }

    fn found_events(&self) -> usize {
        of(&self.r.sink, "session.ask")
            .iter()
            .filter(|e| text(e, "then").as_deref() == Some("found"))
            .count()
    }

    fn later(&self, ms: i64) {
        self.r.clock.fetch_add(ms, Ordering::SeqCst);
    }
}

#[test]
fn found_at_once_when_the_line_is_there() {
    let l = lab(A1, A2);
    let id = open(&l.r);
    until("found", || l.row(&id).asker == Asker::Subagent);
    let row = l.row(&id);
    assert_eq!(row.label.as_deref(), Some("Port the parser"));
    assert_eq!(row.agent_id.as_deref(), Some("a1"));
    assert_eq!(row.tool_use_id.as_deref(), Some("toolu_a1_bg"));
    until("event", || l.found_events() == 1);
    until("item", || items(&l.r).len() == 1);
    assert!(changes(&l.r)
        .iter()
        .any(|c| c["asker"] == json!("subagent")));
    assert_eq!(l.found_events(), 1);
}

#[test]
fn found_on_a_file_event_after_open() {
    let l = lab(&before_result(A1), &before_result(A2));
    let id = open(&l.r);
    l.first_read_done();
    assert_eq!(l.row(&id).asker, Asker::Unknown);
    l.later(3_000);
    put(subagent_file(&l.r, "a1"), A1);
    l.fire();
    until("found", || l.row(&id).asker == Asker::Subagent);
    assert_eq!(l.row(&id).label.as_deref(), Some("Port the parser"));
    until("event", || l.found_events() == 1);
    assert_eq!(l.found_events(), 1);
}

#[test]
fn stops_after_find_ms_and_stays_unknown() {
    let l = lab(&before_result(A1), &before_result(A2));
    let id = open(&l.r);
    l.first_read_done();
    assert!(items(&l.r).is_empty());
    l.later(FIND_MS);
    until("settled", || items(&l.r).len() == 1);
    assert_eq!(
        items(&l.r)[0].title,
        "This session or one of its subagents asks · agent/ask"
    );
    assert_eq!(l.row(&id).asker, Asker::Unknown);
    assert_eq!(l.found_events(), 0);
    let reads = l.reads.load(Ordering::SeqCst);
    l.later(1_000);
    l.fire();
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(l.reads.load(Ordering::SeqCst), reads);
}

#[test]
fn inbox_item_waits_for_the_find() {
    let l = lab(&before_result(A1), &before_result(A2));
    let id = open(&l.r);
    l.first_read_done();
    assert!(items(&l.r).is_empty());
    assert!(l.row(&id).inbox_id.is_none());
    l.later(2_000);
    put(subagent_file(&l.r, "a1"), A1);
    l.fire();
    until("item", || items(&l.r).len() == 1);
    assert_eq!(items(&l.r)[0].title, "Port the parser asks · agent/ask");
    assert_eq!(item_of(&l.r, &id).title, "Port the parser asks · agent/ask");
}

#[test]
fn delegated_codex_job_is_codex_with_its_delegator() {
    let delegate = bash_line(
        "2026-11-16T10:01:20.000Z",
        "toolu_delegate",
        &format!("{EXE} delegate codex --write -- 'Review Task 3'"),
    );
    let l = lab(&format!("{A1}{delegate}"), &before_result(A2));
    l.r.clock
        .store(at("2026-11-16T10:01:30.000Z"), Ordering::SeqCst);
    l.r.tree.child_of(ASKER, CODEX);
    l.r.tree.child_of(CODEX, DELEGATE);
    l.r.tree.child_of(DELEGATE, SHELL);
    l.r.tree.binary(DELEGATE, Path::new(EXE));
    l.r.tree.runs(
        DELEGATE,
        &[EXE, "delegate", "codex", "--write", "--", "Review Task 3"],
    );
    let id = open(&l.r);
    until("found", || l.row(&id).asker == Asker::Codex);
    let row = l.row(&id);
    assert_eq!(
        row.label.as_deref(),
        Some("Codex job · delegated by «Port the parser»")
    );
    assert_eq!(row.tool_use_id, None);
    until("item", || items(&l.r).len() == 1);
    assert_eq!(
        items(&l.r)[0].title,
        "Codex job · delegated by «Port the parser» asks · agent/ask"
    );
}

#[test]
fn another_binary_named_delegate_is_not_a_delegator() {
    let l = lab(&before_result(A1), &before_result(A2));
    l.r.tree.child_of(ASKER, CODEX);
    l.r.tree.child_of(CODEX, SHELL);
    l.r.tree
        .binary(CODEX, Path::new("/tmp/elsewhere/notchling"));
    l.r.tree
        .runs(CODEX, &["notchling", "delegate", "codex", "--", "x"]);
    let id = open(&l.r);
    l.first_read_done();
    l.later(FIND_MS);
    until("settled", || items(&l.r).len() == 1);
    assert_eq!(l.row(&id).asker, Asker::Unknown);
}

fn main_only(claim: bool) -> (Lab, String) {
    let l = lab(&before_result(A1), &before_result(A2));
    l.r.tree.writes_to(ASKER, Path::new("/nonexistent/none"));
    l.r.clock
        .store(at("2026-11-16T10:00:30.000Z"), Ordering::SeqCst);
    if claim {
        let other = l.r.store.insert(&new_row(SESSION, "k0", 77), T0).unwrap();
        let held = Found {
            asker: Asker::Main,
            agent_id: None,
            label: None,
            agent_type: None,
            model: None,
            tool_use_id: Some("toolu_main_bg".into()),
        };
        l.r.store.set_found(&other.id, &held, T0).unwrap();
    }
    let id = open(&l.r);
    (l, id)
}

#[test]
fn claimed_tool_use_ids_are_skipped() {
    let (l, id) = main_only(true);
    l.first_read_done();
    l.later(FIND_MS);
    until("settled", || items(&l.r).len() == 1);
    assert_eq!(l.row(&id).asker, Asker::Unknown);
    assert_eq!(l.row(&id).state, AskState::Open);
}

#[test]
fn unclaimed_main_command_is_found() {
    let (l, id) = main_only(false);
    until("found", || l.row(&id).asker == Asker::Main);
    assert_eq!(l.row(&id).tool_use_id.as_deref(), Some("toolu_main_bg"));
}

#[test]
fn never_blocks_the_op_thread() {
    let l = lab(A1, A2);
    l.held.store(true, Ordering::SeqCst);
    let id = open_with(&l.r, base()).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(l.row(&id).asker, Asker::Unknown);
    assert!(items(&l.r).is_empty());
    assert_eq!(l.reads.load(Ordering::SeqCst), 0);
    let again = l.r.ops.call(
        &asker_at(ASKER),
        notchling_local_api::ops::ASK_OPEN,
        &base(),
    );
    assert_eq!(again.unwrap()["id"], json!(id));
    l.held.store(false, Ordering::SeqCst);
    until("found", || l.row(&id).asker == Asker::Subagent);
    l.r.ops.close();
}
