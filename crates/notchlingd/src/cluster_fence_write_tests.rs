use crate::cluster_fence::{clear_moved, fence_path, mark_moved, read_fence};
use notchling_engine::cluster::fence::{FenceFile, Gate, FENCE_VERSION};
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
    let dir = std::env::temp_dir().join(format!("nl-cfw-{tag}-{}-{n}", std::process::id()));
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

fn newer_than_this_build() -> Vec<u8> {
    let file = FenceFile {
        v: FENCE_VERSION,
        term: term(1),
        node: node("m5"),
        boot_id: "boot".to_string(),
        valid_until: 99,
        sessions: BTreeMap::from([("theirs".to_string(), Gate::Free)]),
    };
    let text = serde_json::to_string(&file).unwrap();
    text.replace(
        &format!("\"v\":{FENCE_VERSION}"),
        &format!("\"v\":{}", FENCE_VERSION + 1),
    )
    .into_bytes()
}

#[test]
fn writers_at_the_same_time_all_land() {
    let dir = scratch("race");
    let cluster = dir.0.join("cluster");
    let writers = 12;
    let handles: Vec<_> = (0..writers)
        .map(|i| {
            let cluster = cluster.clone();
            std::thread::spawn(move || {
                let key = format!("session-{i}");
                mark_moved(&cluster, &node("m5"), term(2), &[key], "m1")
            })
        })
        .collect();
    for handle in handles {
        handle
            .join()
            .unwrap()
            .expect("a write that was not refused");
    }
    let held = read_fence(&cluster).expect("a fence file");
    for i in 0..writers {
        assert!(
            held.sessions.contains_key(&format!("session-{i}")),
            "session-{i} was lost: {:?}",
            held.sessions.keys().collect::<Vec<_>>()
        );
    }
}

#[test]
fn a_damaged_file_is_left_as_it_is_and_reported() {
    let dir = scratch("damaged");
    let cluster = dir.0.join("cluster");
    std::fs::write(fence_path(&cluster), b"garbage").unwrap();
    let why = mark_moved(&cluster, &node("m5"), term(2), &keys(&["a"]), "m1").unwrap_err();
    assert!(why.contains("fence.json"), "{why}");
    assert_eq!(std::fs::read(fence_path(&cluster)).unwrap(), b"garbage");
}

#[test]
fn a_file_from_a_newer_build_is_left_as_it_is_and_reported() {
    let dir = scratch("newer");
    let cluster = dir.0.join("cluster");
    let theirs = newer_than_this_build();
    std::fs::write(fence_path(&cluster), &theirs).unwrap();
    let why = mark_moved(&cluster, &node("m5"), term(2), &keys(&["a"]), "m1").unwrap_err();
    assert!(why.contains("fence.json"), "{why}");
    assert_eq!(std::fs::read(fence_path(&cluster)).unwrap(), theirs);
}

#[test]
fn lifting_a_gate_in_a_damaged_file_is_reported() {
    let dir = scratch("clear-damaged");
    let cluster = dir.0.join("cluster");
    std::fs::write(fence_path(&cluster), b"garbage").unwrap();
    let why = clear_moved(&cluster, &keys(&["a"])).unwrap_err();
    assert!(why.contains("fence.json"), "{why}");
    assert_eq!(std::fs::read(fence_path(&cluster)).unwrap(), b"garbage");
}
