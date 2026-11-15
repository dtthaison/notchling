use super::tests::from_home;
use super::{DispatchCalls, NodeReporter};
use crate::cluster_dispatch_delegate::{DelegateRunner, OUTPUT_CAP};
use crate::cluster_node::LocalCalls;
use crate::cluster_testkit::{Dir, FakeCalls};
use crate::grant_use::tests::{deliver_args, from_vault, rig, Rig as Grants, OTHER_JOB};
use crate::shell_jobs::{ShellGrants, ShellJobs};
use serde_json::{json, Value};
use std::sync::Arc;

const VALUE: &str = "NL-MARKER-OTHER-VALUE";

struct Says {
    out: String,
    err: String,
}

impl DelegateRunner for Says {
    fn run(&self, _args: &[String]) -> (i32, String, String) {
        (0, self.out.clone(), self.err.clone())
    }
}

fn deliver(grants: &Grants, job: &str, name: &str, value: &str) {
    let args = deliver_args(job, name, "V", value);
    grants
        .uses
        .deliver(&from_vault(), &args)
        .expect("accepted before the job starts here");
}

fn reply(out: String, err: String) -> Value {
    let grants = rig();
    deliver(&grants, OTHER_JOB, "other-key", VALUE);
    let dir = Dir::new("delegate-masked");
    let shells = ShellJobs::new(dir.path().to_path_buf(), Arc::new(NodeReporter::default()))
        .with_grants(ShellGrants {
            masks: grants.masks.clone(),
            roots: grants.roots.clone(),
            reaped: Arc::new(|_: &str| {}),
            ended: Arc::new(|_: &str| {}),
        });
    let inner = Arc::new(FakeCalls::default());
    let runner = Arc::new(Says { out, err });
    let calls = DispatchCalls::new(inner, shells, runner, Arc::new(|| true));
    let args = json!({"args": ["agy", "--folder", "/srv", "--", "x"], "job": "j-1"});
    calls.call("delegate.run", &args, &from_home()).unwrap()
}

#[test]
fn delegate_run_masks_every_value_live_on_the_node_in_out_and_err() {
    let out = reply(format!("out {VALUE}\n"), format!("err {VALUE}\n"));
    assert_eq!(
        out,
        json!({"exit": 0, "out": "out [secret other-key]\n", "err": "err [secret other-key]\n"})
    );
}

#[test]
fn a_value_straddling_the_output_cap_is_masked_before_the_cut() {
    let pad = "x".repeat(OUTPUT_CAP - 6);
    let out = reply(
        format!("{pad}{VALUE} and more\n"),
        format!("{pad}{VALUE}\n"),
    );
    for field in ["out", "err"] {
        let text = out[field].as_str().unwrap();
        assert!(text.contains("[output cut at 256 KB]"), "{field} was cut");
        assert!(
            !text.contains("NL-MAR"),
            "{field} leaked the start of a value"
        );
        assert!(!text.contains("MARKER"), "{field} leaked a part of a value");
    }
}

#[test]
fn a_reply_under_the_cap_is_not_cut() {
    let out = reply("short\n".to_string(), String::new());
    assert_eq!(out["out"], "short\n");
    assert_eq!(out["err"], "");
}
