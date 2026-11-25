use crate::snap_testkit::{id_of, Fixture};
use notchling_engine::bridge::net::crypto::base64;
use notchling_engine::cluster::blob::sha256_of;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use serde_json::json;

fn node_origin(from: &notchling_engine::cluster::NodeId) -> Origin {
    Origin {
        node: from.clone(),
        tier: OriginTier::Node,
    }
}

fn rig(tag: &str) -> Fixture {
    let fx = Fixture::new(tag, &[(1, true), (3, false)]);
    fx.make_ready(&[id_of(1)]);
    fx
}

#[test]
fn a_worker_takes_no_set() {
    let fx = rig("recv-worker");
    let (manifest, _) = fx.stage(1, 0, b"x");
    let err = fx
        .net
        .member(&id_of(3))
        .node
        .call(
            "snap.offer",
            &json!({"manifest": manifest}),
            &node_origin(&fx.primary),
        )
        .unwrap()
        .unwrap_err();
    assert!(err.message.contains("Can Be Master"), "{}", err.message);
    assert!(!fx.net.member(&id_of(3)).cluster.join("standby").exists());
}

#[test]
fn the_primary_takes_no_set_and_only_it_may_send_one() {
    let fx = rig("recv-origin");
    let (manifest, _) = fx.stage(1, 0, b"x");
    let args = json!({"manifest": manifest});
    let standby = fx.net.member(&id_of(1));
    let from_other = standby
        .node
        .call("snap.offer", &args, &node_origin(&id_of(3)))
        .unwrap();
    assert!(from_other.is_err());
    let primary = fx.net.member(&fx.primary);
    let to_primary = primary
        .node
        .call("snap.offer", &args, &node_origin(&fx.primary))
        .unwrap();
    assert!(to_primary.is_err());
    assert!(!standby.cluster.join("standby").exists());
}

#[test]
fn a_part_must_name_a_finished_transfer_and_match_its_hash() {
    let fx = rig("recv-part");
    let (manifest, _) = fx.stage(1, 0, b"abc");
    let standby = fx.net.member(&id_of(1));
    let origin = node_origin(&fx.primary);
    standby
        .node
        .call("snap.offer", &json!({"manifest": manifest}), &origin)
        .unwrap()
        .unwrap();
    let part = manifest.parts.last().unwrap().clone();
    let no_transfer = json!({"set": "s-1", "part": part, "transfer": "nope"});
    assert!(standby
        .node
        .call("snap.part", &no_transfer, &origin)
        .unwrap()
        .is_err());
    let hostile = json!({"set": "s-1", "part": part, "transfer": "../../x"});
    assert!(standby
        .node
        .call("snap.part", &hostile, &origin)
        .unwrap()
        .is_err());
    standby
        .blobs
        .put(&json!({"transfer": "t1", "offset": 0, "data": base64(b"zzz")}))
        .unwrap();
    standby
        .blobs
        .end(&json!({"transfer": "t1", "sha256": sha256_of(b"zzz")}))
        .unwrap();
    let wrong = json!({"set": "s-1", "part": part, "transfer": "t1"});
    assert!(standby
        .node
        .call("snap.part", &wrong, &origin)
        .unwrap()
        .is_err());
    let unlisted = json!({"set": "s-2", "part": part, "transfer": "t1"});
    assert!(standby
        .node
        .call("snap.part", &unlisted, &origin)
        .unwrap()
        .is_err());
    let commit = standby
        .node
        .call("snap.commit", &json!({"set": "s-1"}), &origin)
        .unwrap();
    assert!(commit.is_err());
    assert!(fx.store_of(&id_of(1)).current().is_none());
}

#[test]
fn a_folder_part_refuses_paths_that_climb_out() {
    let fx = rig("recv-dir");
    let (manifest, _) = fx.stage(1, 0, b"abc");
    let standby = fx.net.member(&id_of(1));
    let origin = node_origin(&fx.primary);
    standby
        .node
        .call("snap.offer", &json!({"manifest": manifest}), &origin)
        .unwrap()
        .unwrap();
    let dir = manifest
        .parts
        .iter()
        .find(|p| matches!(p.kind, notchling_engine::cluster::snapshot::PartKind::Dir))
        .unwrap()
        .clone();
    standby
        .blobs
        .put(&json!({"transfer": "t1", "offset": 0, "data": base64(b"x")}))
        .unwrap();
    standby
        .blobs
        .end(&json!({"transfer": "t1", "sha256": sha256_of(b"x")}))
        .unwrap();
    for rel in ["../escape", "/abs", "a/../../b", "", "a//b"] {
        let args = json!({"set": "s-1", "part": dir, "files": [{"rel": rel, "transfer": "t1"}]});
        let result = standby.node.call("snap.part", &args, &origin).unwrap();
        assert!(result.is_err(), "{rel:?}");
    }
    assert!(!standby.cluster.join("escape").exists());
    assert!(!standby.cluster.parent().unwrap().join("escape").exists());
}

#[test]
fn an_ack_counts_only_from_a_can_be_master_standby_on_the_primary() {
    let fx = rig("recv-ack");
    let primary = fx.net.member(&fx.primary);
    let args = json!({"set": "s-1", "lengths": {"/a/b.jsonl": 5}});
    assert!(primary
        .node
        .call("snap.ack", &args, &node_origin(&id_of(3)))
        .unwrap()
        .is_err());
    assert!(primary
        .node
        .call("snap.ack", &args, &node_origin(&id_of(9)))
        .unwrap()
        .is_err());
    assert!(primary
        .node
        .call("snap.ack", &args, &node_origin(&fx.primary))
        .unwrap()
        .is_err());
    for bad in [
        json!({"set": "../x", "lengths": {}}),
        json!({"set": "s-1", "lengths": {"relative": 1}}),
        json!({"set": "s-1", "lengths": {"/a": "x"}}),
        json!({"set": "s-1"}),
    ] {
        assert!(primary
            .node
            .call("snap.ack", &bad, &node_origin(&id_of(1)))
            .unwrap()
            .is_err());
    }
    assert!(fx.book.get(&id_of(1)).is_none());
    primary
        .node
        .call("snap.ack", &args, &node_origin(&id_of(1)))
        .unwrap()
        .unwrap();
    assert_eq!(fx.book.get(&id_of(1)).unwrap().set, "s-1");
    let standby = fx.net.member(&id_of(1));
    assert!(standby
        .node
        .call("snap.ack", &args, &node_origin(&id_of(1)))
        .unwrap()
        .is_err());
}

#[test]
fn snap_ops_are_audited_by_name() {
    let known = crate::cluster_node::known_node_ops();
    for op in crate::snap_recv::SNAP_OPS {
        assert!(known.contains(&op), "{op}");
    }
}

#[test]
fn a_retried_commit_of_the_current_set_is_acked_again() {
    let fx = rig("recv-retry-commit");
    fx.make_ready(&[id_of(1)]);
    let (manifest, dir) = fx.stage(1, 0, b"one\ntwo\n");
    fx.sender.offer(manifest.clone(), dir);
    assert!(fx.sender.wait_idle(std::time::Duration::from_secs(20)));
    let acks = fx.net.calls(&fx.primary, "snap.ack");
    assert_eq!(acks, 1);
    let standby = fx.net.member(&id_of(1));
    let origin = node_origin(&fx.primary);
    let offered = standby
        .node
        .call("snap.offer", &json!({"manifest": manifest}), &origin)
        .unwrap()
        .unwrap();
    assert_eq!(offered["missing"], json!([]));
    let again = standby
        .node
        .call("snap.commit", &json!({"set": "s-1"}), &origin)
        .unwrap()
        .unwrap();
    assert_eq!(again["set"], "s-1");
    assert_eq!(
        again["lengths"][fx.transcript().to_str().unwrap()],
        json!(8)
    );
    assert_eq!(fx.net.calls(&fx.primary, "snap.ack"), 2);
    assert_eq!(fx.book.get(&id_of(1)).unwrap().set, "s-1");
    assert_eq!(fx.store_of(&id_of(1)).current(), Some(manifest));
}

#[test]
fn a_dropped_commit_reply_is_retried_by_the_sender() {
    let fx = rig("recv-dropped-reply");
    fx.make_ready(&[id_of(1)]);
    let (manifest, dir) = fx.stage(1, 0, b"one\n");
    fx.sender.offer(manifest.clone(), dir.clone());
    assert!(fx.sender.wait_idle(std::time::Duration::from_secs(20)));
    fx.book.forget(&id_of(1));
    fx.sender.offer(manifest, dir);
    assert!(fx.sender.wait_idle(std::time::Duration::from_secs(20)));
    assert!(fx.sender.last_error(&id_of(1)).is_none());
    assert_eq!(fx.book.get(&id_of(1)).unwrap().set, "s-1");
}
