use super::manifest::{dir_digest, staged_path, Manifest, Part, PartKind};
use super::{excluded, Movable, SessionEntry, MAX_FILE};
use crate::cluster::id::Tool;
use crate::cluster::members::Term;
use crate::cluster::testkit::id;
use std::path::{Path, PathBuf};

fn entry(name: &str, len: u64, sha: &str) -> (String, u64, String) {
    (name.to_owned(), len, sha.to_owned())
}

#[test]
fn manifest_round_trips_through_json() {
    let manifest = Manifest {
        set: "s-4".to_owned(),
        seq: 4,
        term: Term {
            n: 2,
            node: id("a"),
        },
        home: id("a"),
        created_ms: 1_700_000_000_000,
        sessions: vec![SessionEntry {
            id: "abc".to_owned(),
            tool: Tool::Claude,
            cwd: "/work/x".to_owned(),
            repo: None,
            movable: Movable::No("/work/x isn't a git repository".to_owned()),
        }],
        parts: vec![
            Part {
                kind: PartKind::Db,
                path: PathBuf::from("/d/cluster.db"),
                sha256: "aa".to_owned(),
                len: 1,
            },
            Part {
                kind: PartKind::File,
                path: PathBuf::from("/d/devices.json"),
                sha256: "bb".to_owned(),
                len: 2,
            },
            Part {
                kind: PartKind::Dir,
                path: PathBuf::from("/d/pages"),
                sha256: "cc".to_owned(),
                len: 3,
            },
            Part {
                kind: PartKind::Tail { from: 40 },
                path: PathBuf::from("/d/t.jsonl"),
                sha256: "dd".to_owned(),
                len: 4,
            },
        ],
    };
    let text = serde_json::to_string(&manifest).expect("serialises");
    let back: Manifest = serde_json::from_str(&text).expect("parses");
    assert_eq!(back, manifest);
}

#[test]
fn dir_digest_ignores_the_order_of_entries() {
    let a = entry("a/x", 1, "11");
    let b = entry("b", 2, "22");
    let forward = dir_digest(&[a.clone(), b.clone()]);
    let backward = dir_digest(&[b, a]);
    assert_eq!(forward, backward);
    assert_eq!(forward.len(), 64);
}

#[test]
fn dir_digest_changes_with_any_field() {
    let base = dir_digest(&[entry("a", 1, "11")]);
    assert_ne!(base, dir_digest(&[entry("b", 1, "11")]));
    assert_ne!(base, dir_digest(&[entry("a", 2, "11")]));
    assert_ne!(base, dir_digest(&[entry("a", 1, "12")]));
    assert_ne!(base, dir_digest(&[]));
}

#[test]
fn dir_digest_is_the_hash_of_sorted_lines() {
    let digest = dir_digest(&[entry("b", 2, "22"), entry("a", 1, "11")]);
    use sha2::{Digest, Sha256};
    let want: String = Sha256::digest(b"11 1 a\n22 2 b\n")
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(digest, want);
}

#[test]
fn excluded_names_the_cluster_secret_folders() {
    assert!(excluded(Path::new("/data/cluster/grants/a.json")));
    assert!(excluded(Path::new("/data/cluster/logins")));
    assert!(excluded(Path::new("/data/cluster/incoming/x/y")));
    assert!(!excluded(Path::new("/data/cluster/outgoing/x")));
    assert!(!excluded(Path::new("/data/grants/a.json")));
    assert!(!excluded(Path::new("/data/cluster/grantsx/a.json")));
}

#[test]
fn excluded_names_a_regular_file_over_the_limit() {
    let dir = std::env::temp_dir().join(format!("nl-excl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let big = dir.join("big");
    let file = std::fs::File::create(&big).expect("file");
    file.set_len(MAX_FILE + 1).expect("sparse");
    let small = dir.join("small");
    std::fs::write(&small, b"x").expect("small");
    assert!(excluded(&big));
    assert!(!excluded(&small));
    assert!(!excluded(&dir.join("missing")));
    assert!(excluded(Path::new("/data/pages/../cluster/grants")));
    assert!(excluded(Path::new("/data/../pages")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn staged_path_lays_a_source_under_parts() {
    assert_eq!(
        staged_path(
            Path::new("/stage/s-1"),
            Path::new("/home/u/.claude/p/s.jsonl")
        ),
        Some(PathBuf::from("/stage/s-1/parts/home/u/.claude/p/s.jsonl"))
    );
}

#[test]
fn staged_path_refuses_relative_and_dotted_paths() {
    let set = Path::new("/stage/s-1");
    assert_eq!(staged_path(set, Path::new("home/u/a")), None);
    assert_eq!(staged_path(set, Path::new("/home/../etc/a")), None);
    assert_eq!(staged_path(set, Path::new("/home/./u/a")), None);
    assert_eq!(staged_path(set, Path::new("/home/u/..")), None);
}

#[test]
fn part_kinds_and_verdicts_serialise_in_snake_case() {
    let tail = serde_json::to_string(&PartKind::Tail { from: 3 }).expect("tail");
    assert_eq!(tail, r#"{"tail":{"from":3}}"#);
    assert_eq!(serde_json::to_string(&PartKind::Db).expect("db"), r#""db""#);
    assert_eq!(
        serde_json::to_string(&Movable::Yes).expect("yes"),
        r#""yes""#
    );
    let no = serde_json::to_string(&Movable::No("why".to_owned())).expect("no");
    assert_eq!(no, r#"{"no":"why"}"#);
}
