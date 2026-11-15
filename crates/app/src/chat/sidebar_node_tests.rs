use notchling_engine::sessions::AgentGroup;

use super::data::Tool;
use super::sidebar::tests::session;
use super::sidebar::{sidebar_model, SidebarModel};
use super::sidebar_node::{node_label, reply_control, reply_note, Nodes};
use crate::accounts::store::Snapshot;
use crate::acting::Control;
use crate::cluster::model::ClusterModel;
use crate::cluster::renders::{status, ME, STANDBY, WORKER};

const NOW: i64 = 1_000_000_000_000;

fn remote(node: &str, id: &str) -> String {
    format!("{node}/{id}")
}

fn model(ids: &[String], nodes: &Nodes) -> SidebarModel {
    let sessions: Vec<_> = ids
        .iter()
        .map(|id| session(id, AgentGroup::Working, None, Tool::Claude))
        .collect();
    sidebar_model(&sessions, NOW, None, &Snapshot::default(), None).with_nodes(nodes)
}

fn nodes() -> Nodes {
    Nodes::from_cluster(&ClusterModel::from_status(&status(), ME))
}

fn rows(model: &SidebarModel) -> Vec<&super::sidebar::SidebarRow> {
    model.sections.iter().flat_map(|s| &s.rows).collect()
}

#[test]
fn remote_row_label_is_node_name() {
    let ids = [remote(STANDBY, "job-1"), "local-job".to_owned()];
    let model = model(&ids, &nodes());
    let rows = rows(&model);
    let label = rows[0].node.as_ref().expect("a remote row names its node");
    assert_eq!(label.name, "mac-mini");
    assert_eq!(label.id, STANDBY);
    assert!(label.online);
    assert_eq!(rows[1].node, None, "a bare id is local");
}

#[test]
fn offline_remote_row_disables_reply() {
    let ids = [remote(WORKER, "job-2"), remote(STANDBY, "job-3")];
    let nodes = nodes();
    assert_eq!(
        reply_note(&ids[0], &nodes).as_deref(),
        Some("build-box is offline")
    );
    assert_eq!(reply_note(&ids[1], &nodes), None);
    assert_eq!(reply_note("local-job", &nodes), None);
    assert_eq!(
        reply_control(&ids[0], &nodes),
        Some(Control {
            enabled: false,
            tooltip: None
        })
    );
    assert_eq!(reply_control(&ids[1], &nodes), None);
    assert_eq!(reply_control("local-job", &nodes), None);
    assert_eq!(
        reply_control(&remote("ffffffffffffffff", "x"), &nodes).map(|c| c.enabled),
        Some(false)
    );
    let model = model(&ids, &nodes);
    let rows = rows(&model);
    assert!(!rows[0].node.as_ref().expect("label").online);
    assert!(rows[1].node.as_ref().expect("label").online);
}

#[test]
fn a_node_the_cluster_does_not_list_is_named_by_its_id_and_is_offline() {
    let unknown = "ffffffffffffffff";
    let id = remote(unknown, "job-4");
    let label = node_label(&id, &nodes()).expect("a qualified id");
    assert_eq!(label.name, unknown);
    assert!(!label.online);
    assert_eq!(
        reply_note(&id, &nodes()),
        Some(format!("{unknown} is offline"))
    );
    let none = node_label(&id, &Nodes::default()).expect("no cluster at all");
    assert_eq!(none.name, unknown);
    assert!(!none.online);
}

#[test]
fn only_a_sixteen_hex_prefix_makes_an_id_remote() {
    for id in [
        "codex:abc",
        "agy:abc/def",
        "pty:3",
        "ABCDEF0123456789/x",
        "short/x",
    ] {
        assert_eq!(node_label(id, &nodes()), None, "{id}");
    }
    assert!(node_label(&remote(ME, "pty:3"), &nodes()).is_some());
}

#[test]
fn a_removed_member_is_not_a_known_node() {
    let mut status = status();
    status["members"][0]["state"] = serde_json::json!("removed");
    let nodes = Nodes::from_cluster(&ClusterModel::from_status(&status, ME));
    let label = node_label(&remote(WORKER, "job-5"), &nodes).expect("label");
    assert_eq!(label.name, WORKER);
    assert!(!label.online);
}

#[test]
fn the_model_changes_when_a_node_goes_offline() {
    let ids = [remote(STANDBY, "job-1")];
    let online = model(&ids, &nodes());
    let offline = model(&ids, &nodes().with(STANDBY, "mac-mini", false));
    assert_ne!(online, offline);
    assert_eq!(online, model(&ids, &nodes()));
}
