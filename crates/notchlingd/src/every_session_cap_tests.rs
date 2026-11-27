use crate::every_session::CODEX_ROWS;
use crate::every_session_tests::{every, now_ms, Fixture};
use notchling_engine::contract::flows::delegate_jobs;
use notchling_engine::delegate::options::{Options, Tool as DelegateTool};
use notchling_engine::delegate::record::{self, Delegation};
use std::collections::HashSet;
use std::time::Duration;

const REAL: usize = 40;
const JOBS: usize = 95;

fn thread(prefix: &str, n: usize) -> String {
    format!("{prefix}-0000-7000-8000-{n:012}")
}

fn rollout_on(fixture: &Fixture, day: u32, n: usize, id: &str) {
    let path = fixture
        .layout
        .codex_dir()
        .join(format!("sessions/2026/11/{day:02}"))
        .join(format!(
            "rollout-2026-11-{day:02}T{:02}-{:02}-00-{id}.jsonl",
            n / 60,
            n % 60
        ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let meta = format!(
        "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"cwd\":\"/work/codex-app\"}}}}\n"
    );
    let user = "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"Do it\"}]}}\n";
    std::fs::write(path, format!("{meta}{user}")).unwrap();
}

fn handed_over(fixture: &Fixture, session: &str) {
    let options = Options {
        tool: DelegateTool::Codex,
        prompt: "A job".into(),
        model: None,
        effort: None,
        folder: "/work".into(),
        out: None,
        write: false,
        session: Some(session.into()),
        timeout: Duration::from_secs(30),
    };
    let mut record = Delegation::new("a-claude-session", "/work", &options, 1.0, 1);
    record.session = Some(session.into());
    record::write(&delegate_jobs::folder(&fixture.layout.data_dir), &record).unwrap();
}

fn world(tag: &str) -> Fixture {
    let fixture = Fixture::new(tag);
    for n in 0..REAL {
        rollout_on(&fixture, 10, n, &thread("019ccccc", n));
    }
    for n in 0..JOBS {
        let id = thread("019bbbbb", n);
        rollout_on(&fixture, 14, n, &id);
        handed_over(&fixture, &id);
    }
    fixture
}

#[test]
fn delegated_rollouts_newer_than_real_ones_do_not_use_up_the_cap() {
    let fixture = world("every-cap");
    let listed: HashSet<String> = every(&fixture)
        .agents(now_ms())
        .into_iter()
        .map(|a| a.id)
        .collect();
    let newest_real: HashSet<String> = (REAL - CODEX_ROWS..REAL)
        .map(|n| format!("codex:{}", thread("019ccccc", n)))
        .collect();
    assert_eq!(listed, newest_real);
}

#[test]
fn the_rows_for_the_phone_are_the_same_capped_real_ones() {
    let fixture = world("every-cap-rows");
    let rows = every(&fixture).rows(now_ms());
    assert_eq!(rows.len(), CODEX_ROWS);
    assert!(rows.iter().all(|r| r.id.starts_with("codex:019ccccc")));
}
