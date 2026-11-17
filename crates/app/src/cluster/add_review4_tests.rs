use notchling_engine::machines::registry::MachineAbilities;
use notchling_engine::machines::step::{CheckResult, ClusterTokens, StepId};
use notchling_engine::versions::knob::sha256_hex;
use notchling_engine::versions::Tool;
use std::sync::Arc;

use super::planned;
use crate::cluster::add::{AddFlow, Phase};
use crate::cluster::add_rig_tests::{sd_machine, Rig};

fn answer(probe: &str) -> (String, i32) {
    if probe.contains("uname -m") {
        return (
            "os=Darwin\narch=arm64\nuid=501\nuser=agent\nhome=/Users/agent\nversion=14.5\ndisk_kb=90000000\ndocker=none"
                .to_string(),
            0,
        );
    }
    if probe.contains("status --json --peers=false") {
        return (
            r#"{"BackendState":"Running","Self":{"TailscaleIPs":["100.64.0.5"]},"CurrentTailnet":{"Name":"t.ts.net"}}"#
                .to_string(),
            0,
        );
    }
    if let Some(at) = probe.find("@@place ") {
        let nonce: String = probe[at + 8..].chars().take(16).collect();
        let mut out = String::new();
        for (i, part) in probe.split("echo '@@place ").skip(1).enumerate() {
            let path = part
                .split("f=")
                .nth(1)
                .and_then(|p| p.split_whitespace().next())
                .unwrap_or("")
                .trim_matches('\'');
            let body = if path.ends_with(".plist") {
                "<plist></plist>"
            } else {
                "{}"
            };
            out.push_str(&format!(
                "@@place {nonce} {i}\npath={path}\nsize={}\nsha256={}\n@@begin {nonce}\n{body}\n@@end {nonce}\n",
                body.len(),
                sha256_hex(body)
            ));
        }
        return (out, 0);
    }
    (String::new(), 1)
}

fn self_update_ids(flow: &AddFlow) -> Vec<StepId> {
    let plan = flow.plan().expect("a plan");
    plan.rows
        .into_iter()
        .chain(plan.after)
        .map(|r| r.id)
        .filter(|id| id.self_update_tool().is_some())
        .collect()
}

#[test]
fn an_sd_machine_joins_on_its_node_steps_only() {
    let mut m = sd_machine("vps-sing-1", "agent");
    m.abilities = Some(MachineAbilities::default());
    let rig = Rig::real(vec![m]);
    rig.answer_batches(answer);
    let sentinel = CheckResult::pass("the sd worker's own result");
    let sd_steps = [
        StepId::self_update(Tool::Claude),
        StepId::self_update(Tool::Codex),
        StepId::TOOL_VERSIONS,
        StepId::CLAUDE_CODE,
        StepId::BASE_TOOLS,
    ];
    for id in &sd_steps {
        rig.engine.show("vps-sing-1", id.clone(), sentinel.clone());
    }
    let mut flow = AddFlow::for_machine(rig.parts(), "vps-sing-1").expect("registered");
    rig.check(&mut flow);
    assert_eq!(flow.phase(), &Phase::Plan, "{:?}", flow.phase());
    assert!(
        self_update_ids(&flow).is_empty(),
        "{:?}",
        self_update_ids(&flow)
    );
    let stored = rig.engine.results("vps-sing-1");
    for id in &sd_steps {
        assert_eq!(
            stored.get(id).map(|r| r.state.text().to_owned()),
            Some("the sd worker's own result".to_owned()),
            "{id:?} was written by the join check"
        );
    }
    assert!(stored.contains_key(&StepId::NODE_SERVICE));
    assert!(stored.contains_key(&StepId::REACHABLE));
}

#[test]
fn a_node_only_machine_keeps_its_self_update_row() {
    let rig = Rig::real(vec![]);
    rig.answer_batches(answer);
    let flow = planned(&rig, "agent@box");
    assert!(
        !self_update_ids(&flow).is_empty(),
        "a node-only machine has its own self-update step"
    );
}

struct Home;

impl ClusterTokens for Home {
    fn join_token(&self) -> Result<String, String> {
        Err("no token in tests".into())
    }
    fn cluster_id(&self) -> Result<Option<String>, String> {
        Ok(Some("home-cluster".into()))
    }
    fn can_be_master(&self, _machine: &str) -> Option<bool> {
        None
    }
}

#[test]
fn a_machine_in_another_cluster_stops_the_add_even_when_its_binary_differs() {
    let rig = Rig::real_in(vec![], Arc::new(Home));
    rig.answer_batches(|probe| {
        if probe.contains("cluster status --json") {
            return (
                r#"{"me":"0123456789abcdef","cluster":{"id":"rival-cluster","name":"rival"}}"#
                    .to_string(),
                0,
            );
        }
        if probe.contains("--version") {
            return ("notchling 0.0.1 (other-build)".to_string(), 0);
        }
        answer(probe)
    });
    let mut flow = rig.flow("agent@box");
    rig.check(&mut flow);
    let Phase::Stopped(stop) = flow.phase() else {
        panic!("{:?}", flow.phase());
    };
    assert!(
        stop.reason.contains("in another cluster, rival"),
        "{stop:?}"
    );
    assert!(!stop.check_again);
    assert!(flow.plan().is_none());
}
