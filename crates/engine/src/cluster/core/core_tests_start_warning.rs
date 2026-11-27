use serde_json::json;

use super::core_tests::{pair, run_locals};
use super::{Input, LocalCmd};
use crate::cluster::id::Tool;
use crate::cluster::jobs::{job_id, JobSpec, JobState};
use crate::cluster::testkit::id;
use crate::cluster::wire::OriginTier;

const WARNING: &str = "The session started but its first message wasn't queued.";

#[test]
fn a_warning_in_a_start_reply_stays_on_the_running_job_as_its_detail() {
    let (mut hub, m5, w1) = pair();
    let spec = JobSpec {
        node: id("w1"),
        tool: Tool::Shell,
        cwd: "/srv/repo".to_string(),
        args: json!({"command": "printf ok"}),
        secrets: Vec::new(),
    };
    hub.local(
        &m5,
        LocalCmd::Dispatch {
            key: "k".to_string(),
            spec,
            tier: OriginTier::App,
        },
    );
    let seen: Vec<_> = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "w1")
        .map(|(_, e)| e.clone())
        .collect();
    let (call, _, _) = run_locals(&seen).last().cloned().expect("a local run");
    hub.run(
        &w1,
        Input::LocalDone {
            call,
            result: Ok(json!({"session": "shell:j", "warning": WARNING})),
        },
    );
    let job = &hub.core(&m5).state().expect("state").jobs[&job_id("k")];
    assert_eq!(job.state, JobState::Running);
    assert_eq!(job.detail.as_deref(), Some(WARNING));
}
