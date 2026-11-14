use std::fs;

use serde_json::json;

use super::{load, save, wipe, CLUSTER_DB};
use crate::cluster::id::Tool;
use crate::cluster::jobs::{job_id, JobSpec, JobState};
use crate::cluster::members::Term;
use crate::cluster::state::{apply, start, Applied, ClusterState, Command};
use crate::cluster::testkit::{id, member, versions};
use crate::sessions::testutil::TempDir;

fn built() -> (ClusterState, Vec<Applied>) {
    let (mut state, entry, changes) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "home".to_string(),
            home: member("m5", "macos", true),
        },
        1,
    )
    .expect("create");
    let mut steps = vec![Applied {
        entry: Some(entry),
        changes,
    }];
    let w1 = member("w1", "linux", false);
    for command in [
        Command::ProposeJoin {
            member: w1.clone(),
            item: "i".to_string(),
        },
        Command::Approve {
            node: w1.id.clone(),
            client: false,
            can_be_master: false,
        },
        Command::Versions {
            node: w1.id.clone(),
            versions: versions("linux"),
        },
        Command::Dispatch {
            key: "k".to_string(),
            spec: JobSpec {
                node: w1.id.clone(),
                tool: Tool::Codex,
                cwd: "/srv".to_string(),
                args: json!({"a": 1}),
                secrets: Vec::new(),
            },
        },
        Command::JobState {
            id: job_id("k"),
            state: JobState::Running,
            session: Some("s".to_string()),
            detail: None,
        },
        Command::Term {
            term: Term {
                n: 2,
                node: id("m5"),
            },
        },
    ] {
        steps.push(apply(&mut state, command, 2).expect("apply"));
    }
    (state, steps)
}

fn save_all(dir: &std::path::Path, steps: &[Applied]) {
    for step in steps {
        let entry = step.entry.as_ref().expect("entry");
        save(dir, entry, &step.changes).expect("save");
    }
}

#[test]
fn cluster_db_round_trips_state() {
    let tmp = TempDir::new("cluster-persist");
    let dir = tmp.path("cluster");
    let (state, steps) = built();
    save_all(&dir, &steps);
    let loaded = load(&dir).expect("load").expect("state");
    assert_eq!(loaded, state);
}

#[test]
fn missing_cluster_db_loads_none_and_creates_nothing() {
    let tmp = TempDir::new("cluster-none");
    let dir = tmp.path("cluster");
    assert_eq!(load(&dir).expect("load"), None);
    assert!(!dir.exists());
    fs::create_dir_all(&dir).expect("dir");
    assert_eq!(load(&dir).expect("load"), None);
    assert!(!dir.join(CLUSTER_DB).exists());
}

#[test]
fn newer_schema_is_refused_untouched() {
    let tmp = TempDir::new("cluster-newer");
    let dir = tmp.path("cluster");
    fs::create_dir_all(&dir).expect("dir");
    let path = dir.join(CLUSTER_DB);
    {
        let db = rusqlite::Connection::open(&path).expect("db");
        db.execute_batch("CREATE TABLE odd (x); PRAGMA user_version = 99;")
            .expect("batch");
    }
    let before = fs::read(&path).expect("bytes");
    assert_eq!(
        load(&dir).err().as_deref(),
        Some("cluster.db is from a newer Notchling")
    );
    let (_, steps) = built();
    let first = steps[0].entry.as_ref().expect("entry");
    assert_eq!(
        save(&dir, first, &steps[0].changes).err().as_deref(),
        Some("cluster.db is from a newer Notchling")
    );
    assert_eq!(fs::read(&path).expect("bytes"), before);
    let leftovers: Vec<_> = fs::read_dir(&dir)
        .expect("dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(leftovers.len(), 1, "{leftovers:?}");
}

#[test]
fn journal_seq_only_grows() {
    let tmp = TempDir::new("cluster-seq");
    let dir = tmp.path("cluster");
    let (_, steps) = built();
    save_all(&dir, &steps[..3]);
    let old = steps[1].entry.as_ref().expect("entry");
    assert!(save(&dir, old, &steps[1].changes).is_err());
    let same = steps[2].entry.as_ref().expect("entry");
    assert!(save(&dir, same, &steps[2].changes).is_err());
    let next = steps[3].entry.as_ref().expect("entry");
    assert!(save(&dir, next, &steps[3].changes).is_ok());
    let loaded = load(&dir).expect("load").expect("state");
    assert_eq!(loaded.seq, 4);
}

#[test]
fn a_failed_save_changes_nothing() {
    let tmp = TempDir::new("cluster-atomic");
    let dir = tmp.path("cluster");
    let (_, steps) = built();
    save_all(&dir, &steps[..2]);
    let before = load(&dir).expect("load");
    let stale = steps[0].entry.as_ref().expect("entry");
    assert!(save(&dir, stale, &steps[3].changes).is_err());
    assert_eq!(load(&dir).expect("load"), before);
}

#[test]
fn wipe_removes_the_database_only() {
    let tmp = TempDir::new("cluster-wipe");
    let dir = tmp.path("cluster");
    let (_, steps) = built();
    save_all(&dir, &steps);
    fs::write(dir.join("node.pem"), b"keep me").expect("pem");
    wipe(&dir).expect("wipe");
    assert!(!dir.join(CLUSTER_DB).exists());
    assert!(!dir.join(format!("{CLUSTER_DB}-wal")).exists());
    assert!(!dir.join(format!("{CLUSTER_DB}-shm")).exists());
    assert_eq!(fs::read(dir.join("node.pem")).expect("pem"), b"keep me");
    assert_eq!(load(&dir).expect("load"), None);
    wipe(&dir).expect("wipe again");
    wipe(&tmp.path("never-made")).expect("wipe nothing");
}

#[cfg(unix)]
#[test]
fn the_database_is_private_and_a_link_in_its_place_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new("cluster-private");
    let dir = tmp.path("cluster");
    let (_, steps) = built();
    save_all(&dir, &steps[..1]);
    let mode = |p: &std::path::Path| fs::metadata(p).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&dir.join(CLUSTER_DB)), 0o600);
    wipe(&dir).expect("wipe");
    let target = tmp.path("elsewhere.db");
    fs::write(&target, b"").expect("target");
    std::os::unix::fs::symlink(&target, dir.join(CLUSTER_DB)).expect("link");
    assert!(load(&dir).is_err());
    assert!(save(
        &dir,
        steps[0].entry.as_ref().expect("entry"),
        &steps[0].changes
    )
    .is_err());
    assert_eq!(fs::read(&target).expect("bytes"), b"");
}
