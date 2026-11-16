use crate::cluster_fence::{
    drop_stale, drop_stale_at_start, fence_path, mark_moved, mark_moved_in, read_fence, wipe,
};
use crate::cluster_testkit::{eventually, pair, Dir, Opts};
use crate::grant_use::tests::member;
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::fence::{parse, Gate};
use notchling_engine::cluster::id::{fingerprint, NodeId};
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::persist::save;
use notchling_engine::cluster::state::{start, Command};
use notchling_engine::cluster::LocalCmd;

fn node(name: &str) -> NodeId {
    NodeId::from_fingerprint(&fingerprint(name.as_bytes())).unwrap()
}

fn term(n: u64) -> Term {
    Term {
        n,
        node: node("m1"),
    }
}

fn keys(list: &[&str]) -> Vec<String> {
    list.iter().map(|k| k.to_string()).collect()
}

fn cluster_of(dir: &Dir) -> std::path::PathBuf {
    let cluster = dir.path().join("cluster");
    std::fs::create_dir_all(&cluster).unwrap();
    cluster
}

fn persisted(cluster: &std::path::Path, id: &str) {
    let (_, entry, changes) = start(
        Command::Create {
            id: id.to_string(),
            name: "home".to_string(),
            home: member("m5"),
        },
        1,
    )
    .expect("create");
    save(cluster, &entry, &changes).expect("save");
}

#[test]
fn a_gate_names_the_cluster_that_wrote_it_and_the_hook_still_reads_the_file() {
    let dir = Dir::new("stale-stamp");
    let cluster = cluster_of(&dir);
    mark_moved_in(
        &cluster,
        &node("m5"),
        term(2),
        &keys(&["a"]),
        "m1",
        Some("c1"),
    )
    .unwrap();
    let bytes = std::fs::read(fence_path(&cluster)).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("\"cluster\":\"c1\""), "{text}");
    let parsed = parse(&bytes).expect("the hook's reader parses a stamped file");
    assert_eq!(parsed.sessions["a"], Gate::Moved { to: "m1".into() });
}

#[test]
fn marking_in_another_cluster_drops_the_old_clusters_gates() {
    let dir = Dir::new("stale-other");
    let cluster = cluster_of(&dir);
    let me = node("m5");
    mark_moved_in(&cluster, &me, term(2), &keys(&["old"]), "m1", Some("c1")).unwrap();
    mark_moved_in(&cluster, &me, term(1), &keys(&["new"]), "m2", Some("c2")).unwrap();
    let held = read_fence(&cluster).unwrap();
    assert!(!held.sessions.contains_key("old"), "{:?}", held.sessions);
    assert_eq!(held.sessions["new"], Gate::Moved { to: "m2".into() });
    mark_moved_in(&cluster, &me, term(3), &keys(&["more"]), "m2", Some("c2")).unwrap();
    let held = read_fence(&cluster).unwrap();
    assert!(held.sessions.contains_key("new"));
    assert!(held.sessions.contains_key("more"));
}

#[test]
fn old_gates_that_would_not_fit_are_pruned_before_the_write_is_refused() {
    let dir = Dir::new("stale-cap");
    let cluster = cluster_of(&dir);
    let me = node("m5");
    let old: Vec<String> = (0..800).map(|i| format!("{i:036}")).collect();
    mark_moved_in(&cluster, &me, term(1), &old, "m1", Some("c1")).unwrap();
    let new: Vec<String> = (0..250).map(|i| format!("n{i:035}")).collect();
    mark_moved_in(&cluster, &me, term(2), &new, "m2", Some("c2")).unwrap();
    let held = read_fence(&cluster).unwrap();
    assert_eq!(held.sessions.len(), 250);
}

#[test]
fn a_gate_of_the_same_cluster_stays_and_one_with_no_stamp_stays_in_a_cluster() {
    let dir = Dir::new("stale-keep");
    let cluster = cluster_of(&dir);
    mark_moved_in(
        &cluster,
        &node("m5"),
        term(2),
        &keys(&["a"]),
        "m1",
        Some("c1"),
    )
    .unwrap();
    drop_stale(&cluster, Some("c1")).unwrap();
    assert!(read_fence(&cluster).is_some());
    mark_moved(&cluster, &node("m5"), term(2), &keys(&["b"]), "m1").unwrap();
    drop_stale(&cluster, Some("c9")).unwrap();
    assert!(
        read_fence(&cluster).is_none(),
        "the stamp from c1 is not c9's"
    );
    mark_moved(&cluster, &node("m5"), term(2), &keys(&["b"]), "m1").unwrap();
    drop_stale(&cluster, Some("c1")).unwrap();
    assert!(
        read_fence(&cluster).is_some(),
        "an unstamped file can't be told apart"
    );
}

#[test]
fn no_cluster_means_no_gates() {
    let dir = Dir::new("stale-none");
    let cluster = cluster_of(&dir);
    mark_moved(&cluster, &node("m5"), term(2), &keys(&["a"]), "m1").unwrap();
    drop_stale(&cluster, None).unwrap();
    assert!(!fence_path(&cluster).exists());
    drop_stale(&cluster, None).unwrap();
}

#[cfg(unix)]
#[test]
fn wiping_removes_a_link_and_never_its_target() {
    let dir = Dir::new("stale-link");
    let cluster = cluster_of(&dir);
    let target = dir.path().join("elsewhere");
    std::fs::write(&target, b"keep").unwrap();
    std::os::unix::fs::symlink(&target, fence_path(&cluster)).unwrap();
    wipe(&cluster).unwrap();
    assert!(std::fs::symlink_metadata(fence_path(&cluster)).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"keep");
}

#[test]
fn wiping_removes_a_damaged_file() {
    let dir = Dir::new("stale-damaged");
    let cluster = cluster_of(&dir);
    std::fs::write(fence_path(&cluster), b"garbage").unwrap();
    wipe(&cluster).unwrap();
    assert!(!fence_path(&cluster).exists());
}

#[test]
fn at_start_gates_of_a_cluster_that_is_gone_are_dropped() {
    let dir = Dir::new("stale-start");
    let cluster = cluster_of(&dir);
    mark_moved_in(
        &cluster,
        &node("m5"),
        term(2),
        &keys(&["a"]),
        "m1",
        Some("c1"),
    )
    .unwrap();
    drop_stale_at_start(&cluster).unwrap();
    assert!(!fence_path(&cluster).exists(), "no cluster is left on disk");

    persisted(&cluster, "c2");
    mark_moved_in(
        &cluster,
        &node("m5"),
        term(2),
        &keys(&["a"]),
        "m1",
        Some("c1"),
    )
    .unwrap();
    drop_stale_at_start(&cluster).unwrap();
    assert!(!fence_path(&cluster).exists(), "the cluster on disk is c2");

    mark_moved_in(
        &cluster,
        &node("m5"),
        term(2),
        &keys(&["a"]),
        "m1",
        Some("c2"),
    )
    .unwrap();
    drop_stale_at_start(&cluster).unwrap();
    assert!(
        read_fence(&cluster).is_some(),
        "the gates of this cluster stay"
    );
}

#[test]
fn a_removed_node_loses_its_fence_file_with_the_rest_of_its_cluster_state() {
    let net = MemNet::new();
    let mut home = Opts::named("home");
    home.client = true;
    let (a, b) = pair(&net, home, Opts::named("worker"));
    let cluster = b.cluster_dir();
    mark_moved(&cluster, &node("m5"), term(2), &keys(&["a"]), "m1").unwrap();
    assert!(fence_path(&cluster).exists());
    a.node.local(LocalCmd::Remove { node: b.id() }).unwrap();
    eventually("the fence file is gone", || !fence_path(&cluster).exists());
}
