use super::*;
use crate::chat::outbox::{bubble, OutboxRow};
use notchling_engine::sessions::AgentTarget;
use serde_json::json;

fn agent(group: AgentGroup, at_prompt: bool) -> Agent {
    Agent {
        id: "6f1c2a9e-3b7d-4e21-9a0c-5d8e7f6a1b2c".into(),
        name: "main".into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group,
        label: "Working".into(),
        detail: None,
        since_ms: 77,
        target: AgentTarget::Job("ab12cd34".into()),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt,
        node: None,
        worktree_path: None,
    }
}

fn row(state: &str, why: Option<&str>) -> Bubble {
    let mut wire = json!({
        "id": "o1", "session": "ab12cd34", "tool": "claude", "kind": "prompt",
        "text": "run the suite", "state": state, "position": 1,
        "queuedMs": 1, "updatedMs": 1, "via": "chat",
    });
    if let Some(why) = why {
        wire["why"] = json!(why);
    }
    bubble(&OutboxRow::from_wire(&wire).expect("a row"))
}

fn status_of(bubbles: &[Bubble]) -> Vec<Status> {
    bubbles.iter().map(|b| b.status.clone()).collect()
}

#[test]
fn the_job_file_says_how_many_background_tasks_run() {
    let flight = in_flight(&json!({
        "state": "working",
        "inFlight": {"tasks": 9, "queued": 0, "kinds": ["local_agent", "local_bash"]}
    }));
    assert_eq!(
        flight,
        InFlight {
            tasks: 9,
            commands: false
        }
    );
}

#[test]
fn only_shell_commands_are_called_commands() {
    let flight = in_flight(&json!({"inFlight": {"tasks": 3, "kinds": ["local_bash"]}}));
    assert_eq!(
        flight,
        InFlight {
            tasks: 3,
            commands: true
        }
    );
    let none = in_flight(&json!({"inFlight": {"tasks": 3}}));
    assert!(!none.commands);
}

#[test]
fn a_file_without_in_flight_or_with_odd_values_has_no_tasks() {
    for job in [
        json!({"state": "working"}),
        json!({"inFlight": 3}),
        json!({"inFlight": {"tasks": "many"}}),
        json!({"inFlight": {"tasks": -1}}),
        json!(null),
    ] {
        assert_eq!(in_flight(&job), InFlight::default(), "{job}");
    }
}

#[test]
fn background_tasks_come_before_the_reply() {
    let facts = BusyFacts {
        working: true,
        at_prompt: false,
        tasks: 9,
        commands: false,
    };
    assert_eq!(
        why_busy(&facts),
        Some(Busy::Tasks {
            count: 9,
            commands: false
        })
    );
}

#[test]
fn a_reply_in_progress_is_a_working_session_not_at_its_prompt() {
    let replying = BusyFacts {
        working: true,
        ..BusyFacts::default()
    };
    assert_eq!(why_busy(&replying), Some(Busy::Replying));
    let at_prompt = BusyFacts {
        working: true,
        at_prompt: true,
        ..BusyFacts::default()
    };
    assert_eq!(why_busy(&at_prompt), None);
}

#[test]
fn an_idle_session_is_not_busy_whatever_the_file_says() {
    let idle = BusyFacts {
        working: false,
        tasks: 4,
        ..BusyFacts::default()
    };
    assert_eq!(why_busy(&idle), None);
    assert_eq!(why_busy(&BusyFacts::default()), None);
}

#[test]
fn the_facts_come_from_the_agent_the_file_and_the_rows() {
    let flight = InFlight {
        tasks: 2,
        commands: true,
    };
    let facts = facts(&agent(AgentGroup::Working, true), flight);
    assert_eq!(
        facts,
        BusyFacts {
            working: true,
            at_prompt: true,
            tasks: 2,
            commands: true
        }
    );
    let done = super::facts(&agent(AgentGroup::Completed, false), flight);
    assert!(!done.working);
}

#[test]
fn queued_messages_say_what_the_session_is_busy_with() {
    let facts = BusyFacts {
        working: true,
        at_prompt: true,
        tasks: 9,
        ..BusyFacts::default()
    };
    let mut shown = vec![row("queued", None), row("waiting", Some("turn"))];
    explain(&mut shown, &facts);
    let busy = Status::Busy(Busy::Tasks {
        count: 9,
        commands: false,
    });
    assert_eq!(status_of(&shown), [busy.clone(), busy]);
}

#[test]
fn an_idle_session_leaves_the_status_alone() {
    let mut shown = vec![row("queued", None)];
    explain(&mut shown, &BusyFacts::default());
    assert_eq!(status_of(&shown), [Status::Queued]);
}

#[test]
fn a_message_behind_an_unsure_one_says_it_is_unconfirmed() {
    let mut shown = vec![row("unsure", Some("not_found")), row("queued", None)];
    shown[1].status = Status::WaitsForPrevious;
    explain(&mut shown, &BusyFacts::default());
    assert_eq!(shown[0].status, Status::MayBeIn);
    assert_eq!(shown[1].status, Status::Busy(Busy::Unconfirmed));
}

#[test]
fn a_message_behind_something_else_keeps_its_words() {
    for blocker in [
        row("box", None),
        row("stopped", None),
        row("gone", None),
        row("in_queue", None),
    ] {
        let mut shown = vec![blocker, row("queued", None)];
        shown[1].status = Status::WaitsForPrevious;
        explain(&mut shown, &BusyFacts::default());
        assert_eq!(shown[1].status, Status::WaitsForPrevious);
    }
}

#[test]
fn only_the_first_message_holding_the_line_names_the_cause() {
    let mut shown = vec![
        row("box", None),
        row("unsure", Some("not_found")),
        row("queued", None),
    ];
    shown[2].status = Status::WaitsForPrevious;
    explain(&mut shown, &BusyFacts::default());
    assert_eq!(shown[2].status, Status::WaitsForPrevious);
}

#[test]
fn other_states_are_never_explained_as_busy() {
    let facts = BusyFacts {
        working: true,
        tasks: 5,
        ..BusyFacts::default()
    };
    let mut shown = vec![
        row("waiting", Some("dialog")),
        row("waiting", Some("answer")),
        row("waiting", Some("retry")),
        row("sending", None),
        row("stopped", None),
        row("gone", None),
    ];
    let before = status_of(&shown);
    explain(&mut shown, &facts);
    assert_eq!(status_of(&shown), before);
}

#[test]
fn only_a_local_working_job_with_waiting_messages_is_read() {
    let waiting = vec![row("queued", None)];
    let working = agent(AgentGroup::Working, true);
    assert!(wants_read(&working, &waiting));
    assert!(!wants_read(&working, &[]));
    assert!(!wants_read(&working, &[row("sending", None)]));
    assert!(!wants_read(&agent(AgentGroup::Completed, false), &waiting));
    let mut remote = working.clone();
    remote.node = Some(notchling_engine::sessions::NodeRef {
        id: "n1".into(),
        name: "m1".into(),
        online: true,
    });
    assert!(!wants_read(&remote, &waiting));
    let mut process = working;
    process.target = AgentTarget::Process(4242);
    assert!(!wants_read(&process, &waiting));
}

#[test]
fn a_read_is_made_once_per_state_of_the_job() {
    let mut reading = Reading::default();
    let working = agent(AgentGroup::Working, true);
    let key = Reading::key_of(&working).expect("a key");
    assert_eq!(key, "ab12cd34|77");
    assert!(reading.due(&key));
    reading.begin(&key);
    assert!(!reading.due(&key));
    assert_eq!(reading.of(&working), InFlight::default(), "not read yet");
    let flight = InFlight {
        tasks: 9,
        commands: false,
    };
    reading.finish(&key, flight);
    assert_eq!(reading.of(&working), flight);
    let mut later = working.clone();
    later.since_ms = 78;
    let next = Reading::key_of(&later).expect("a key");
    assert!(reading.due(&next), "the job's file changed");
    assert_eq!(reading.of(&later), flight, "kept until the new read lands");
    let mut other = working;
    other.target = AgentTarget::Job("ffff0000".into());
    assert_eq!(reading.of(&other), InFlight::default());
}

#[test]
fn a_late_answer_for_an_old_read_is_dropped() {
    let mut reading = Reading::default();
    reading.begin("ab12cd34|1");
    reading.begin("ab12cd34|2");
    reading.finish(
        "ab12cd34|1",
        InFlight {
            tasks: 5,
            commands: false,
        },
    );
    assert_eq!(
        reading.of(&agent(AgentGroup::Working, true)),
        InFlight::default()
    );
}
