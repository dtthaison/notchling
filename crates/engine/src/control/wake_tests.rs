use super::*;
use crate::sessions::agent::sample;
use crate::sessions::testutil::TempDir;

fn job(id: &str, group: AgentGroup) -> Agent {
    let mut a = sample(AgentTarget::Job(format!("job-{id}")), None);
    a.id = id.into();
    a.group = group;
    a
}

fn always(id: &'static str) -> impl Fn(&Agent) -> Option<String> {
    move |_| Some(id.to_string())
}

#[test]
fn nothing_happens_before_a_wake_or_when_switched_off() {
    let mut resumer = WakeResumer::new();
    let agents = [job("a", AgentGroup::Completed)];
    assert!(resumer
        .check(&agents, 100_000, true, &always("cut-1"))
        .is_empty());
    resumer.did_wake(0);
    assert!(resumer
        .check(&agents, 100_000, false, &always("cut-1"))
        .is_empty());
}

#[test]
fn waits_for_the_settle_time_then_nudges_once_per_cut() {
    let mut resumer = WakeResumer::new();
    let agents = [job("a", AgentGroup::Completed)];
    resumer.did_wake(0);
    assert!(resumer
        .check(&agents, SETTLE_MS - 1, true, &always("cut-1"))
        .is_empty());
    let nudges = resumer.check(&agents, SETTLE_MS, true, &always("cut-1"));
    assert_eq!(
        nudges,
        [Nudge {
            agent_id: "a".into(),
            job_id: "job-a".into(),
            text: PROMPT.into()
        }]
    );
    resumer.finished("a");
    assert!(resumer
        .check(&agents, SETTLE_MS + 1000, true, &always("cut-1"))
        .is_empty());
    assert_eq!(
        resumer
            .check(&agents, SETTLE_MS + 2000, true, &always("cut-2"))
            .len(),
        1
    );
}

#[test]
fn skips_working_agents_terminals_and_ones_still_sending() {
    let mut resumer = WakeResumer::new();
    let mut terminal = job("t", AgentGroup::Completed);
    terminal.target = AgentTarget::Process(7);
    let agents = [
        job("w", AgentGroup::Working),
        terminal,
        job("a", AgentGroup::NeedsInput),
    ];
    resumer.did_wake(0);
    let first = resumer.check(&agents, SETTLE_MS, true, &|a| Some(format!("cut-{}", a.id)));
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].agent_id, "a");
    assert!(resumer
        .check(&agents, SETTLE_MS + 1, true, &always("other"))
        .is_empty());
    resumer.finished("a");
    assert_eq!(
        resumer
            .check(&agents, SETTLE_MS + 2, true, &always("other"))
            .len(),
        1
    );
}

#[test]
fn a_cut_off_long_after_the_wake_was_not_this_sleeps() {
    let mut resumer = WakeResumer::new();
    let agents = [job("a", AgentGroup::Completed)];
    resumer.did_wake(0);
    assert!(resumer
        .check(&agents, WINDOW_MS + 1, true, &always("late"))
        .is_empty());
    assert!(resumer
        .check(&agents, SETTLE_MS, true, &always("late"))
        .is_empty());
    resumer.did_wake(WINDOW_MS + 5);
    assert_eq!(
        resumer
            .check(&agents, WINDOW_MS + 5 + SETTLE_MS, true, &always("late"))
            .len(),
        1
    );
}

#[test]
fn an_agent_without_a_cut_off_is_left_alone() {
    let mut resumer = WakeResumer::new();
    resumer.did_wake(0);
    let agents = [job("a", AgentGroup::Completed)];
    assert!(resumer
        .check(&agents, SETTLE_MS, true, &|_| None)
        .is_empty());
}

#[test]
fn the_default_check_reads_the_transcripts_last_message() {
    let tmp = TempDir::new("wake-cut");
    let cut = r#"{"type":"assistant","uuid":"u-cut","isApiErrorMessage":true,"streamCutCause":"stream_suspended","message":{"content":[{"type":"text","text":"API Error: Your computer went to sleep mid-response"}]}}"#;
    let fine = r#"{"type":"assistant","uuid":"u-ok","message":{"content":[{"type":"text","text":"done"}]}}"#;
    let cut_file = tmp.write("cut.jsonl", &format!("{fine}\n{cut}\n"));
    let ok_file = tmp.write("ok.jsonl", &format!("{cut}\n{fine}\n"));
    let mut a = job("a", AgentGroup::Completed);
    a.transcript_path = Some(cut_file.to_string_lossy().into_owned());
    assert_eq!(sleep_cut(&a).as_deref(), Some("u-cut"));
    a.transcript_path = Some(ok_file.to_string_lossy().into_owned());
    assert_eq!(sleep_cut(&a), None, "something happened after the cut-off");
    a.transcript_path = None;
    assert_eq!(sleep_cut(&a), None);
}
