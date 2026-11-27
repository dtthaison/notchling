use super::*;
use crate::chat::inspector::{inspector_with_fan, no_writers, Mark};
use crate::chat::paging::Counts;
use crate::chat::shell_list::ShellPage;
use notchling_engine::sessions::job_fan::{FanEntry, FanKind};
use notchling_engine::transcript::model::{BackgroundShell, Conversation};
use notchling_engine::transcript::subagents::{Origin, Status, Subagent};

fn entry(id: &str, kind: FanKind, label: &str, started: f64) -> FanEntry {
    FanEntry {
        id: id.into(),
        kind,
        label: label.into(),
        started: Some(started),
    }
}

fn shell(id: &str, started: Option<f64>) -> BackgroundShell {
    BackgroundShell {
        id: id.into(),
        command: format!("run {id}"),
        title: String::new(),
        output_path: format!("/tmp/{id}.output"),
        description: String::new(),
        started,
    }
}

fn agent(id: &str, status: Status) -> Subagent {
    Subagent {
        id: id.into(),
        path: String::new(),
        description: format!("job {id}"),
        agent_type: "general-purpose".into(),
        model: None,
        effort: None,
        background: true,
        tool_use_id: None,
        started: 1.0,
        updated: 1.0,
        status,
        origin: Origin::default(),
        session: String::new(),
        parent: String::new(),
        depth: 1,
    }
}

#[test]
fn a_fan_shell_the_transcript_lists_is_not_added_again() {
    let mut c = Conversation::default();
    c.shells.push(shell("b1", Some(10.0)));
    let fan = [
        entry("b1", FanKind::Shell, "run b1", 10.0),
        entry("b2", FanKind::Shell, "sleep 600", 20.0),
        entry("a1", FanKind::Agent, "Look", 30.0),
    ];
    let extra = extra_shells(&fan, &c);
    assert_eq!(extra.len(), 1);
    assert_eq!(extra[0].id, "b2");
    assert_eq!(extra[0].command, "sleep 600");
    assert_eq!(extra[0].started, Some(20.0));
    assert!(extra[0].output_path.is_empty());
}

#[test]
fn merged_shells_follow_their_start_times() {
    let mut c = Conversation::default();
    c.shells.push(shell("b1", Some(10.0)));
    c.shells.push(shell("b3", Some(30.0)));
    c.shells.push(shell("b4", None));
    let extra = [shell("b2", Some(20.0)), shell("b5", Some(40.0))];
    let order: Vec<&str> = merged(&c, &extra)
        .iter()
        .map(|(s, fan)| {
            assert_eq!(*fan, s.id == "b2" || s.id == "b5");
            s.id.as_str()
        })
        .collect();
    assert_eq!(order, ["b1", "b2", "b3", "b4", "b5"]);
}

#[test]
fn an_agent_the_fan_says_is_in_flight_is_running_whatever_the_transcript_said() {
    let mut list = vec![
        agent("a1", Status::Completed),
        agent("a2", Status::Quiet),
        agent("a3", Status::Stopped),
        agent("a4", Status::Completed),
        agent("a5", Status::Running),
    ];
    let fan = [
        entry("a1", FanKind::Agent, "x", 1.0),
        entry("a2", FanKind::Agent, "x", 1.0),
        entry("a3", FanKind::Agent, "x", 1.0),
        entry("a5", FanKind::Agent, "x", 1.0),
        entry("a4", FanKind::Shell, "a shell with that id", 1.0),
        entry("a9", FanKind::Agent, "no such file yet", 1.0),
    ];
    mark_running(&mut list, &fan);
    let status: Vec<Status> = list.iter().map(|a| a.status).collect();
    assert_eq!(
        status,
        [
            Status::Running,
            Status::Running,
            Status::Running,
            Status::Completed,
            Status::Running
        ]
    );
}

#[test]
fn a_subagent_of_another_tool_is_never_matched_by_id() {
    let mut other = agent("a1", Status::Completed);
    other.origin = Origin::native(notchling_engine::contract::Tool::Codex);
    let mut list = vec![other];
    mark_running(&mut list, &[entry("a1", FanKind::Agent, "x", 1.0)]);
    assert_eq!(list[0].status, Status::Completed);
}

#[test]
fn a_fan_shell_shows_as_a_running_row_with_no_output_to_open() {
    let mut c = Conversation::default();
    c.shells.push(shell("b1", Some(10.0)));
    c.finished_agents.insert("b1".into(), "completed".into());
    let extra = [BackgroundShell {
        id: "b2".into(),
        command: "sleep 600".into(),
        title: String::new(),
        output_path: String::new(),
        description: String::new(),
        started: Some(20.0),
    }];
    let mut page = ShellPage::default();
    let listing = page.show_with_fan(Some("s"), &c, &extra, 0, &|_: &str| None);
    assert!(!listing.waiting);
    assert_eq!(listing.ids, ["b2"]);
    assert_eq!((listing.counts.total, listing.counts.running), (2, 1));
    let i = inspector_with_fan(
        &c,
        &extra,
        &[],
        Counts::default(),
        Some(&listing),
        &no_writers,
    );
    assert_eq!(i.shells.len(), 1);
    let row = &i.shells[0];
    assert_eq!(row.title, "sleep 600");
    assert!(row.running && row.mark == Mark::Spinner);
    assert_eq!(row.status_word, "Running");
    assert!(row.output_path.is_empty());
    assert_eq!(i.shells_title(), "Shells · 1 running");
}

#[test]
fn without_a_listing_the_rows_and_total_count_the_fan_shells_too() {
    let mut c = Conversation::default();
    c.shells.push(shell("b1", Some(10.0)));
    c.finished_agents.insert("b1".into(), "completed".into());
    let extra = [shell("b2", Some(20.0))];
    let i = inspector_with_fan(&c, &extra, &[], Counts::default(), None, &no_writers);
    assert_eq!(i.shell_counts.total, 2);
    assert_eq!(i.shells.len(), 2);
    assert!(i.shells[0].running, "the fan shell, running first");
}

#[test]
fn a_fan_shell_takes_the_description_found_for_it() {
    let c = Conversation::default();
    let fan = [
        entry("b2", FanKind::Shell, "sleep 600", 20.0),
        entry("b3", FanKind::Shell, "cargo test", 30.0),
    ];
    let found: HashMap<String, String> =
        HashMap::from([("b2".to_string(), "Wait for the render".to_string())]);
    let extra = extra_shells_described(&fan, &c, &found, &mut |_| Output::Unknown);
    assert_eq!(extra[0].description, "Wait for the render");
    assert_eq!(extra[0].title, "Wait for the render");
    assert_eq!(extra[0].command, "sleep 600");
    assert_eq!(extra[1].description, "");
    assert_eq!(extra[1].title, "");
}

#[test]
fn a_fan_shell_with_no_start_time_leaves_no_file_out_of_reach() {
    let dated = entry("b1", FanKind::Shell, "sleep 1", 500.0);
    let mut undated = entry("b2", FanKind::Shell, "sleep 2", 0.0);
    undated.started = None;
    assert_eq!(earliest_start(&[&dated]), 500.0);
    assert_eq!(earliest_start(&[&dated, &undated]), 0.0);
    assert_eq!(earliest_start(&[&undated]), 0.0);
}
