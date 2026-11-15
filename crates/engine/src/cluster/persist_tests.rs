use std::collections::BTreeSet;
use std::fs;

use serde_json::json;

use super::{load, save, wipe, CLUSTER_DB};
use crate::cluster::id::Tool;
use crate::cluster::jobs::{job_id, JobSpec, JobState};
use crate::cluster::main_sessions::MainSession;
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
        Command::Main {
            main: [(
                "job-1".to_string(),
                MainSession {
                    id: "job-1".to_string(),
                    tool: Tool::Claude,
                    cwd: "/work".to_string(),
                    config_dir: None,
                    transcript: Some("/t.jsonl".to_string()),
                    respawn: Vec::new(),
                    since_ms: 2,
                },
            )]
            .into(),
            ended: [("job-1".to_string(), 3)].into(),
            kept: BTreeSet::new(),
        },
        Command::SetMain {
            session: "job-2".to_string(),
            main: false,
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

#[test]
fn the_inbox_table_is_added_to_an_existing_cluster_db() {
    let tmp = TempDir::new("cluster-inbox-table");
    let dir = tmp.path("cluster");
    assert!(super::open_inbox(&dir, false).expect("none").is_none());
    assert!(!dir.exists());
    let (_, steps) = built();
    save_all(&dir, &steps[..1]);
    let db = super::open_inbox(&dir, false).expect("open").expect("db");
    db.execute(
        "INSERT INTO inbox (id, value, updated_ms) VALUES ('a', '{}', 1)",
        [],
    )
    .expect("insert");
    drop(db);
    save_all(&dir, &steps[1..2]);
    assert!(load(&dir).expect("load").is_some());
    let db = super::open_inbox(&dir, false).expect("open").expect("db");
    let n: i64 = db
        .query_row("SELECT COUNT(*) FROM inbox", [], |r| r.get(0))
        .expect("count");
    assert_eq!(n, 1);
}

#[test]
fn keep_aside_renames_the_database_and_its_side_files_and_deletes_nothing() {
    let tmp = TempDir::new("cluster-aside");
    let dir = tmp.path("cluster");
    let (_, steps) = built();
    save_all(&dir, &steps[..1]);
    fs::write(dir.join("cluster.db-wal"), b"wal").expect("wal");
    fs::write(dir.join("cluster.db-shm"), b"shm").expect("shm");
    let before = fs::read(dir.join(CLUSTER_DB)).expect("bytes");
    super::keep_aside(&dir, 77).expect("aside");
    assert!(!dir.join(CLUSTER_DB).exists());
    assert_eq!(
        fs::read(dir.join("cluster.db.inbox-left-77")).expect("kept"),
        before
    );
    assert_eq!(
        fs::read(dir.join("cluster.db.inbox-left-77-wal")).expect("wal"),
        b"wal"
    );
    assert_eq!(
        fs::read(dir.join("cluster.db.inbox-left-77-shm")).expect("shm"),
        b"shm"
    );
    wipe(&dir).expect("wipe");
    assert!(dir.join("cluster.db.inbox-left-77").exists());
    super::keep_aside(&tmp.path("never-made"), 1).expect("nothing to move");
}

#[test]
fn a_newer_cluster_db_is_not_opened_for_the_inbox() {
    let tmp = TempDir::new("cluster-inbox-newer");
    let dir = tmp.path("cluster");
    fs::create_dir_all(&dir).expect("dir");
    {
        let db = rusqlite::Connection::open(dir.join(CLUSTER_DB)).expect("db");
        db.execute_batch("PRAGMA user_version = 99;").expect("v");
    }
    assert_eq!(
        super::open_inbox(&dir, true).err().as_deref(),
        Some("cluster.db is from a newer Notchling")
    );
}

#[test]
fn main_sessions_survive_a_reload_and_an_older_db_has_none() {
    let tmp = TempDir::new("cluster-main");
    let dir = tmp.path("cluster");
    let (state, steps) = built();
    save_all(&dir, &steps[..2]);
    let older = load(&dir).expect("load").expect("state");
    assert!(older.main.is_empty() && older.kept.is_empty() && older.ended.is_empty());
    save_all(&tmp.path("again"), &steps);
    let loaded = load(&tmp.path("again")).expect("load").expect("state");
    assert!(loaded.main.contains_key("job-1"));
    assert!(loaded.kept.contains("job-2"));
    assert_eq!(loaded.ended["job-1"], 3);
    assert_eq!(loaded, state);
}

fn queued(id: &str, request: &str, at_ms: i64) -> crate::cluster::held::Held {
    crate::cluster::held::Held {
        id: id.to_string(),
        op: "session.reply".to_string(),
        args: json!({"text": id, "n": 1}),
        target: if request == "r2" {
            crate::cluster::held::HeldTarget::Primary
        } else {
            crate::cluster::held::HeldTarget::Session("s1".to_string())
        },
        device: "d1".to_string(),
        request: request.to_string(),
        at_ms,
    }
}

#[test]
fn held_survives_restart() {
    use crate::cluster::held::{HeldQueue, HELD_STALE_MS};
    let tmp = TempDir::new("cluster-held");
    let dir = tmp.path("cluster");
    assert_eq!(super::load_held(&dir).expect("none"), HeldQueue::default());
    assert!(!dir.exists());
    let mut queue = HeldQueue::default();
    queue.hold(queued("done", "r0", 1));
    queue.take_all();
    queue.hold(queued("a", "r1", 10));
    queue.hold(queued("b", "r2", 20));
    queue.hold(queued("c", "r3", 30));
    assert!(queue.confirm("b"));
    super::save_held(&dir, &queue).expect("save");
    let loaded = super::load_held(&dir).expect("load");
    assert_eq!(loaded, queue);
    assert_eq!(
        loaded
            .list()
            .iter()
            .map(|h| h.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    assert!(loaded.was_delivered("done"));
    let mut loaded = loaded;
    assert_eq!(loaded.hold(queued("x", "r0", 99)), "done");
    let now = 20 + HELD_STALE_MS + 1;
    assert_eq!(loaded.waiting(now).len(), 1);
    let due: Vec<String> = loaded.take_due(now).into_iter().map(|h| h.id).collect();
    assert_eq!(due, ["c", "b"]);
    super::save_held(&dir, &loaded).expect("save again");
    let again = super::load_held(&dir).expect("load again");
    assert_eq!(again, loaded);
    assert!(again.was_delivered("b") && again.was_delivered("c"));
    assert_eq!(again.list().len(), 1);
}

#[test]
fn an_older_cluster_db_without_the_held_table_still_opens() {
    let tmp = TempDir::new("cluster-held-older");
    let dir = tmp.path("cluster");
    let (state, steps) = built();
    save_all(&dir, &steps);
    {
        let db = rusqlite::Connection::open(dir.join(CLUSTER_DB)).expect("db");
        let tables: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'held'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(tables, 0);
    }
    assert!(super::load_held(&dir).expect("load").list().is_empty());
    assert_eq!(load(&dir).expect("load").expect("state"), state);
    let mut queue = crate::cluster::held::HeldQueue::default();
    queue.hold(queued("a", "r1", 1));
    super::save_held(&dir, &queue).expect("save");
    assert_eq!(load(&dir).expect("load").expect("state"), state);
    assert_eq!(super::load_held(&dir).expect("load"), queue);
}

#[test]
fn a_newer_cluster_db_is_not_opened_for_the_held_queue() {
    let tmp = TempDir::new("cluster-held-newer");
    let dir = tmp.path("cluster");
    fs::create_dir_all(&dir).expect("dir");
    {
        let db = rusqlite::Connection::open(dir.join(CLUSTER_DB)).expect("db");
        db.execute_batch("PRAGMA user_version = 99;").expect("v");
    }
    assert_eq!(
        super::load_held(&dir).err().as_deref(),
        Some("cluster.db is from a newer Notchling")
    );
    assert!(super::save_held(&dir, &Default::default()).is_err());
}
