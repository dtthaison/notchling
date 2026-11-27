use super::data::DataRoot;
use crate::notch::source::{now_ms, DataRoot as NotchRoot, Source};
use crate::testutil::Scratch;
use notchling_engine::delegate::options::{Options, Tool};
use notchling_engine::delegate::record::{self, Delegation};
use std::collections::HashSet;
use std::time::Duration;

const REAL: usize = 40;
const JOBS: usize = 95;
const CAP: usize = 30;

fn thread(prefix: &str, n: usize) -> String {
    format!("{prefix}-0000-7000-8000-{n:012}")
}

fn rollout_on(scratch: &Scratch, day: u32, n: usize, id: &str) {
    scratch.write(
        &format!(
            "codex/sessions/2026/11/{day:02}/rollout-2026-11-{day:02}T{:02}-{:02}-00-{id}.jsonl",
            n / 60,
            n % 60
        ),
        &format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"cwd\":\"/work/app\"}}}}\n"
        ),
    );
}

fn handed_over(scratch: &Scratch, session: &str) {
    let options = Options {
        tool: Tool::Codex,
        prompt: "A job".into(),
        model: None,
        effort: None,
        folder: "/work".into(),
        out: None,
        write: false,
        session: None,
        timeout: Duration::from_secs(30),
    };
    let mut job = Delegation::new("a-claude-session", "/work", &options, 1.0, 1);
    job.session = Some(session.into());
    record::write(&scratch.path().join("notchling").join(record::FOLDER), &job)
        .expect("write the record");
}

fn world(tag: &str) -> Scratch {
    let scratch = Scratch::new(tag);
    for n in 0..REAL {
        rollout_on(&scratch, 10, n, &thread("019ccccc", n));
    }
    for n in 0..JOBS {
        let id = thread("019bbbbb", n);
        rollout_on(&scratch, 14, n, &id);
        handed_over(&scratch, &id);
    }
    scratch
}

fn newest_real() -> HashSet<String> {
    (REAL - CAP..REAL)
        .map(|n| format!("codex:{}", thread("019ccccc", n)))
        .collect()
}

#[test]
fn delegated_rollouts_do_not_push_real_sessions_out_of_chat() {
    let scratch = world("chat-cap");
    let listed: HashSet<String> = DataRoot::fixture(scratch.path())
        .sessions(now_ms())
        .into_iter()
        .map(|s| s.agent.id)
        .collect();
    let missing: Vec<_> = newest_real().difference(&listed).cloned().collect();
    assert!(missing.is_empty(), "missing {} real rows", missing.len());
}

#[test]
fn delegated_rollouts_do_not_push_real_sessions_out_of_the_notch() {
    let scratch = world("notch-cap");
    let mut source = Source::keeping_everything(NotchRoot::fixture(scratch.path()));
    let loaded = source.load(now_ms());
    let ids: Vec<&str> = loaded.agents.iter().map(|a| a.id.as_str()).collect();
    assert!(!ids.is_empty(), "no Codex row at all");
    assert!(
        ids.iter().all(|id| id.starts_with("codex:019ccccc")),
        "{ids:?}"
    );
}
