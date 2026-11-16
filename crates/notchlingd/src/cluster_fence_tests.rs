use crate::cluster_fence::{clear_moved, fence_path, mark_moved, read_fence, write_fence};
use notchling_engine::cluster::fence::{FenceFile, Gate, FENCE_FILE, FENCE_VERSION};
use notchling_engine::cluster::id::{fingerprint, NodeId};
use notchling_engine::cluster::members::Term;
use std::collections::BTreeMap;
use std::path::PathBuf;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Dir {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("nl-cf-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("cluster")).unwrap();
    Dir(dir)
}

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

fn lease_file() -> FenceFile {
    FenceFile {
        v: FENCE_VERSION,
        term: term(1),
        node: node("m5"),
        boot_id: "boot".to_string(),
        valid_until: 99,
        sessions: BTreeMap::from([("kept".to_string(), Gate::Lease)]),
    }
}

#[test]
fn the_fence_lives_where_the_hook_reads_it() {
    let dir = scratch("path");
    let cluster = dir.0.join("cluster");
    assert_eq!(fence_path(&cluster), dir.0.join(FENCE_FILE));
}

#[test]
fn r176_marking_a_move_writes_a_moved_gate_the_hook_file_parses() {
    let dir = scratch("mark");
    let cluster = dir.0.join("cluster");
    mark_moved(&cluster, &node("m5"), term(2), &keys(&["a", "b"]), "m1").unwrap();
    let held = read_fence(&cluster).expect("a fence file");
    assert_eq!(held.term, term(2));
    assert_eq!(held.node, node("m5"));
    assert_eq!(held.sessions["a"], Gate::Moved { to: "m1".into() });
    assert_eq!(held.sessions["b"], Gate::Moved { to: "m1".into() });
}

#[test]
fn marking_keeps_the_gates_already_there_and_a_newer_term() {
    let dir = scratch("merge");
    let cluster = dir.0.join("cluster");
    write_fence(&cluster, &lease_file()).unwrap();
    mark_moved(&cluster, &node("m5"), term(2), &keys(&["a"]), "m1").unwrap();
    let held = read_fence(&cluster).unwrap();
    assert_eq!(held.sessions["kept"], Gate::Lease);
    assert_eq!(held.valid_until, 99);
    assert_eq!(held.term, term(2));
    mark_moved(&cluster, &node("m5"), term(1), &keys(&["b"]), "m1").unwrap();
    assert_eq!(read_fence(&cluster).unwrap().term, term(2));
}

#[test]
fn clearing_removes_only_moved_gates() {
    let dir = scratch("clear");
    let cluster = dir.0.join("cluster");
    write_fence(&cluster, &lease_file()).unwrap();
    mark_moved(&cluster, &node("m5"), term(2), &keys(&["a"]), "m1").unwrap();
    clear_moved(&cluster, &keys(&["a", "kept", "never"])).unwrap();
    let held = read_fence(&cluster).unwrap();
    assert!(!held.sessions.contains_key("a"));
    assert_eq!(held.sessions["kept"], Gate::Lease);
}

#[test]
fn clearing_with_no_file_writes_nothing() {
    let dir = scratch("none");
    let cluster = dir.0.join("cluster");
    clear_moved(&cluster, &keys(&["a"])).unwrap();
    assert!(!fence_path(&cluster).exists());
}

#[cfg(unix)]
#[test]
fn the_file_is_private_and_no_temp_file_stays() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("mode");
    let cluster = dir.0.join("cluster");
    write_fence(&cluster, &lease_file()).unwrap();
    let mode = std::fs::metadata(fence_path(&cluster))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let names: Vec<String> = std::fs::read_dir(&cluster)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, vec!["fence.json".to_string()]);
}

#[cfg(unix)]
#[test]
fn a_link_at_the_path_is_refused_and_its_target_untouched() {
    let dir = scratch("link");
    let cluster = dir.0.join("cluster");
    let target = dir.0.join("elsewhere");
    std::fs::write(&target, b"keep").unwrap();
    std::os::unix::fs::symlink(&target, fence_path(&cluster)).unwrap();
    assert!(mark_moved(&cluster, &node("m5"), term(2), &keys(&["a"]), "m1").is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"keep");
}

#[test]
fn a_damaged_file_is_replaced() {
    let dir = scratch("damaged");
    let cluster = dir.0.join("cluster");
    std::fs::write(fence_path(&cluster), b"garbage").unwrap();
    mark_moved(&cluster, &node("m5"), term(2), &keys(&["a"]), "m1").unwrap();
    assert!(read_fence(&cluster).unwrap().sessions.contains_key("a"));
}
