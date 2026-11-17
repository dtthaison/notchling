mod common;

use common::*;
use notchling_engine::transcript::conversation;
use notchling_engine::transcript::shells;

const SHELLS: &str = include_str!("fixtures/background-job/agent-shells.jsonl");
const MOVED: &str = "Command was moved to the background (ID: bshell0001) after running for 180s. Output is being written to: /private/tmp/claude-501/-work-film/5e5510aa/tasks/bshell0001.output. You will be notified when it completes. It is running now: do not start a second copy while it runs.";

#[test]
fn launch_reads_a_command_the_harness_moved_to_the_background() {
    let launch = shells::launch(MOVED).expect("launch");
    assert_eq!(launch.id, "bshell0001");
    assert_eq!(
        launch.path,
        "/private/tmp/claude-501/-work-film/5e5510aa/tasks/bshell0001.output"
    );
}

#[test]
fn launch_ignores_a_moved_notice_with_no_output_file() {
    assert!(
        shells::launch("Command was moved to the background (ID: bshell0001) after 180s.")
            .is_none()
    );
    assert!(shells::launch("Command was moved to the background (ID: ) x.output").is_none());
}

#[test]
fn a_command_moved_to_the_background_is_a_shell_of_its_transcript() {
    let scratch = Scratch::new("shell-moved");
    let path = s(&scratch.write("agent-shells.jsonl", SHELLS));
    let read = conversation::read_subagent(&path);
    let ids: Vec<&str> = read.shells.iter().map(|sh| sh.id.as_str()).collect();
    assert_eq!(ids, ["bshell0001", "bshell0002"], "{:?}", read.shells);
    assert!(read.shells[0].title.starts_with("Ran until ! pgrep"));
    assert_eq!(read.shells[1].title, "Wait for the render");
    assert!(read.shells[1]
        .output_path
        .ends_with("/tasks/bshell0002.output"));
    assert_eq!(
        read.finished_agents.get("bshell0002").map(String::as_str),
        Some("completed")
    );
    assert!(!read.finished_agents.contains_key("bshell0001"));
}
