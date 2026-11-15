use super::remote_control::{refused, remote_control, Action};
use super::sidebar_node::Nodes;
use crate::acting::Control;
use crate::cluster::model::ClusterModel;
use crate::cluster::renders::{status, ME, STANDBY, WORKER};

fn nodes() -> Nodes {
    Nodes::from_cluster(&ClusterModel::from_status(&status(), ME))
}

fn remote(node: &str) -> String {
    format!("{node}/job-1")
}

#[test]
fn remote_control_refuses_each_action_when_offline() {
    let nodes = nodes();
    for row in [remote(WORKER), remote("ffffffffffffffff")] {
        for action in Action::ALL {
            let control = remote_control(&row, &nodes, action).expect("a remote row is gated");
            assert!(!control.enabled, "{action:?} on {row}");
        }
    }
    for action in Action::ALL {
        assert_eq!(
            remote_control("local-job", &nodes, action),
            None,
            "a bare id is local: {action:?}"
        );
    }
    let none = Nodes::default();
    assert!(refused(remote_control(
        &remote(STANDBY),
        &none,
        Action::Stop
    )));
}

#[test]
fn an_online_node_takes_a_reply_a_stop_an_answer_and_a_bring_back() {
    let nodes = nodes();
    for action in [
        Action::Reply,
        Action::Stop,
        Action::Answer,
        Action::BringBack,
    ] {
        assert_eq!(
            remote_control(&remote(STANDBY), &nodes, action),
            None,
            "{action:?}"
        );
    }
}

#[test]
fn remote_row_offers_no_terminal_or_move() {
    let nodes = nodes();
    for action in [
        Action::Remove,
        Action::Move,
        Action::Terminal,
        Action::Slash,
        Action::Files,
        Action::Opinion,
    ] {
        let control = remote_control(&remote(STANDBY), &nodes, action);
        assert_eq!(
            control,
            Some(Control {
                enabled: false,
                tooltip: Some("Not available for a session on another machine yet."),
            }),
            "{action:?}"
        );
    }
}
