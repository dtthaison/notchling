use crate::every_session_tests::{every, now_ms, Fixture};
use crate::git_ops::{listed_folders, live_folders};
use crate::pages_ops::Titles;
use notchling_engine::contract::flows::delegate_jobs;
use notchling_engine::delegate::options::{Options, Tool as DelegateTool};
use notchling_engine::delegate::record::{self, Delegation};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const REAL: usize = 40;
const JOBS: usize = 95;

fn thread(prefix: &str, n: usize) -> String {
    format!("{prefix}-0000-7000-8000-{n:012}")
}

fn rollout_on(fixture: &Fixture, day: u32, n: usize, id: &str, cwd: &str) {
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
        "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"cwd\":\"{cwd}\"}}}}\n"
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
        let cwd = format!("/work/real-{n}");
        rollout_on(&fixture, 10, n, &thread("019ccccc", n), &cwd);
    }
    for n in 0..JOBS {
        let id = thread("019bbbbb", n);
        rollout_on(&fixture, 14, n, &id, "/work/job");
        handed_over(&fixture, &id);
    }
    fixture
}

fn no_titles() -> Titles {
    Arc::new(Vec::new)
}

#[test]
fn every_listed_codex_row_has_its_folder_allowed_for_git() {
    let fixture = world("git-listed");
    let every = every(&fixture);
    let listed = every.agents(now_ms());
    assert!(!listed.is_empty());
    let allowed: Vec<PathBuf> = listed_folders(
        live_folders(
            Arc::new(Mutex::new(json!([]))),
            no_titles(),
            fixture.layout.codex_dir().join("sessions"),
            false,
        ),
        every.clone(),
    )();
    for agent in listed {
        assert!(
            allowed.contains(&PathBuf::from(&agent.cwd)),
            "{} ({}) is listed but its folder isn't allowed",
            agent.id,
            agent.cwd
        );
    }
}
