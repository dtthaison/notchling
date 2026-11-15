use crate::snap_files_tests::{count, make_db, put};
use crate::snap_restore::DbHooks;
use crate::snap_restore_testkit::World;
use notchling_engine::bridge::net::devices::{decode, encode, read_devices, PairedDevice};
use notchling_engine::cluster::snapshot::SessionEntry;
use notchling_engine::cluster::Tool;
use rusqlite::Connection;
use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn device(id: &str, name: &str) -> PairedDevice {
    PairedDevice {
        id: id.to_owned(),
        name: name.to_owned(),
        platform: "ios".to_owned(),
        token_hash: format!("hash-{id}"),
        paired_at: 1_700_000_000_000,
        last_seen: None,
    }
}

fn staged(w: &World, bytes: &[u8]) -> std::path::PathBuf {
    let transcript = w.transcript("s1", bytes);
    w.stage(vec![w.entry("s1", &w.repo, &[&transcript], &[])], &[], &[]);
    transcript
}

#[test]
fn a_missing_transcript_is_put_private_in_private_folders() {
    let w = World::new();
    let transcript = staged(&w, b"one\ntwo\n");
    fs::remove_dir_all(w.at(".claude")).unwrap();

    w.run(&["s1"], false, false).unwrap();

    assert_eq!(fs::read(&transcript).unwrap(), b"one\ntwo\n");
    assert_eq!(mode(&transcript), 0o600);
    assert_eq!(mode(&w.at(".claude")), 0o700);
    assert_eq!(mode(&w.at(".claude/projects/p")), 0o700);
}

#[test]
fn a_shorter_local_copy_gets_only_the_rest_appended() {
    let w = World::new();
    let transcript = staged(&w, b"one\ntwo\nthree\n");
    fs::write(&transcript, b"one\ntwo\n").unwrap();

    let report = w.run(&["s1"], false, false).unwrap();

    assert_eq!(report.steps, 1);
    assert_eq!(fs::read(&transcript).unwrap(), b"one\ntwo\nthree\n");
}

#[test]
fn a_transcript_already_the_same_needs_nothing() {
    let w = World::new();
    let transcript = staged(&w, b"one\n");
    let report = w.run(&["s1"], false, false).unwrap();
    assert_eq!(report.steps, 0);
    assert_eq!(fs::read(&transcript).unwrap(), b"one\n");
}

#[test]
fn longer_local_transcript_refuses() {
    let w = World::new();
    let transcript = staged(&w, b"one\n");
    fs::write(&transcript, b"one\nmore work here\n").unwrap();

    let why = w.run(&["s1"], false, false).unwrap_err();

    assert_eq!(why, "s1 ran on m1 after this set");
    assert_eq!(fs::read(&transcript).unwrap(), b"one\nmore work here\n");
    assert!(!w.cluster.join("copies").exists());
}

#[test]
fn aside_keeps_the_local_copy() {
    let w = World::new();
    let transcript = staged(&w, b"one\n");
    let local = b"one\nthe work done here after the set\n";
    fs::write(&transcript, local).unwrap();

    w.run(&["s1"], true, false).unwrap();

    let kept = w
        .cluster
        .join("copies/s1/s-1")
        .join(transcript.strip_prefix("/").unwrap());
    assert_eq!(fs::read(&kept).unwrap(), local);
    assert_eq!(fs::read(&transcript).unwrap(), b"one\n");
    assert_eq!(mode(kept.parent().unwrap()), 0o700);

    fs::write(&transcript, local).unwrap();
    w.run(&["s1"], true, false).unwrap();
    assert_eq!(fs::read(format!("{}.1", kept.display())).unwrap(), local);
    assert_eq!(fs::read(&kept).unwrap(), local);
}

#[test]
fn a_link_that_leads_out_of_home_is_never_followed() {
    let w = World::new();
    staged(&w, b"one\n");
    let elsewhere = crate::limits_ops_tests::TempDir::new("restore-out");
    fs::remove_dir_all(w.at(".claude")).unwrap();
    fs::create_dir_all(w.at(".claude")).unwrap();
    std::os::unix::fs::symlink(elsewhere.root(), w.at(".claude/projects")).unwrap();

    let why = w.run(&["s1"], false, false).unwrap_err();

    assert!(why.contains("leads out of"), "{why}");
    assert!(fs::read_dir(elsewhere.root()).unwrap().next().is_none());
}

#[test]
fn a_link_that_stays_inside_home_is_allowed() {
    let w = World::new();
    let transcript = staged(&w, b"one\n");
    let inside = w.at("dotfiles/projects");
    fs::create_dir_all(&inside).unwrap();
    fs::remove_dir_all(w.at(".claude")).unwrap();
    fs::create_dir_all(w.at(".claude")).unwrap();
    std::os::unix::fs::symlink(&inside, w.at(".claude/projects")).unwrap();

    w.run(&["s1"], false, false).unwrap();

    assert_eq!(fs::read(&transcript).unwrap(), b"one\n");
    assert_eq!(fs::read(inside.join("p/s1.jsonl")).unwrap(), b"one\n");
}

#[test]
fn a_link_at_the_file_itself_is_refused() {
    let w = World::new();
    let transcript = staged(&w, b"one\n");
    fs::remove_file(&transcript).unwrap();
    let target = w.at("elsewhere.txt");
    fs::write(&target, "keep me").unwrap();
    std::os::unix::fs::symlink(&target, &transcript).unwrap();

    let why = w.run(&["s1"], false, false).unwrap_err();

    assert!(why.contains("plain file"), "{why}");
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep me");
}

#[test]
fn a_sidecar_folder_is_merged_and_nothing_in_it_is_deleted() {
    let w = World::new();
    let transcript = w.transcript("s1", b"one\n");
    let history = w.at(".claude/file-history/s1");
    put(&history.join("v1/a.txt"), b"first");
    put(&history.join("b.txt"), b"second");
    w.stage(
        vec![w.entry("s1", &w.repo, &[&transcript], &[&history])],
        &[],
        &[],
    );
    fs::remove_dir_all(&history).unwrap();
    put(&history.join("mine.txt"), b"local only");
    put(&history.join("b.txt"), b"older");

    w.run(&["s1"], false, false).unwrap();

    assert_eq!(fs::read(history.join("v1/a.txt")).unwrap(), b"first");
    assert_eq!(fs::read(history.join("b.txt")).unwrap(), b"second");
    assert_eq!(fs::read(history.join("mine.txt")).unwrap(), b"local only");
    assert_eq!(mode(&history.join("v1")), 0o700);
    assert_eq!(mode(&history.join("v1/a.txt")), 0o600);
}

#[test]
fn a_sidecar_folder_never_reads_through_a_link_that_leaves_home() {
    let w = World::new();
    let transcript = w.transcript("s1", b"one\n");
    let history = w.at(".claude/file-history/s1");
    put(&history.join("v1/a.txt"), b"first");
    w.stage(
        vec![w.entry("s1", &w.repo, &[&transcript], &[&history])],
        &[],
        &[],
    );
    fs::remove_dir_all(&history).unwrap();
    let elsewhere = crate::limits_ops_tests::TempDir::new("restore-dir-out");
    elsewhere.write("a.txt", "first");
    fs::create_dir_all(&history).unwrap();
    std::os::unix::fs::symlink(elsewhere.root(), history.join("v1")).unwrap();

    let why = w.run(&["s1"], false, false).unwrap_err();

    assert!(why.contains("leads out of"), "{why}");
    assert_eq!(fs::read_dir(elsewhere.root()).unwrap().count(), 1);
    assert_eq!(
        fs::read_to_string(elsewhere.path("a.txt")).unwrap(),
        "first"
    );
}

#[test]
fn a_devices_file_that_is_a_link_is_refused_and_its_target_left_alone() {
    let w = World::new();
    let devices = devices_at(&w);
    put(&devices, &encode(&[device("a", "Phone A")]));
    w.stage(vec![], &[&devices], &[]);
    fs::remove_file(&devices).unwrap();
    let target = w.rig.t.path("real-devices.json");
    let same = encode(&[device("a", "Phone A")]);
    fs::write(&target, &same).unwrap();
    std::os::unix::fs::symlink(&target, &devices).unwrap();

    let why = w.run(&[], false, true).unwrap_err();

    assert!(why.contains("plain file"), "{why}");
    assert_eq!(fs::read(&target).unwrap(), same);
    assert!(fs::symlink_metadata(&devices)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn a_codex_index_gets_only_the_moving_sessions_line() {
    let w = World::new();
    let rollout = w.at(".codex/sessions/2026/11/15/rollout-x-abc.jsonl");
    put(&rollout, b"{}\n");
    let index = w.at(".codex/session_index.jsonl");
    put(
        &index,
        b"{\"id\":\"abc\",\"thread_name\":\"mine\"}\n{\"id\":\"zzz\",\"thread_name\":\"theirs\"}\n",
    );
    let mut entry: SessionEntry = w.entry("codex:abc", &w.repo, &[&rollout], &[&index]);
    entry.tool = Tool::Codex;
    w.stage(vec![entry], &[], &[]);
    fs::write(
        &index,
        "{\"id\":\"zzz\",\"thread_name\":\"local theirs\"}\n{\"id\":\"other\",\"thread_name\":\"o\"}",
    )
    .unwrap();
    fs::remove_file(&rollout).unwrap();

    w.run(&["codex:abc"], false, false).unwrap();

    assert_eq!(
        fs::read_to_string(&index).unwrap(),
        "{\"id\":\"zzz\",\"thread_name\":\"local theirs\"}\n{\"id\":\"other\",\"thread_name\":\"o\"}\n{\"id\":\"abc\",\"thread_name\":\"mine\"}\n"
    );
    w.run(&["codex:abc"], false, false).unwrap();
    assert_eq!(fs::read_to_string(&index).unwrap().lines().count(), 3);
    assert!(rollout.exists());
}

fn devices_at(w: &World) -> std::path::PathBuf {
    w.at("AgentConsole/bridge/devices.json")
}

#[test]
fn devices_are_merged_never_removed() {
    let w = World::new();
    let devices = devices_at(&w);
    put(
        &devices,
        &encode(&[device("a", "Phone A"), device("b", "Phone B")]),
    );
    let transcript = w.transcript("s1", b"one\n");
    w.stage(
        vec![w.entry("s1", &w.repo, &[&transcript], &[])],
        &[&devices],
        &[],
    );
    fs::write(
        &devices,
        encode(&[device("b", "Renamed here"), device("c", "Mac's own")]),
    )
    .unwrap();

    w.run(&[], false, true).unwrap();

    let merged = read_devices(&devices).unwrap();
    let ids: Vec<&str> = merged.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, vec!["b", "c", "a"]);
    assert_eq!(merged[0].name, "Renamed here");
    assert_eq!(mode(&devices), 0o600);
    assert!(decode(&fs::read(&devices).unwrap()).is_ok());
}

#[test]
fn an_unreadable_local_devices_file_is_not_written_over() {
    let w = World::new();
    let devices = devices_at(&w);
    put(&devices, &encode(&[device("a", "Phone A")]));
    w.stage(vec![], &[&devices], &[]);
    fs::write(&devices, "not json at all").unwrap();

    let why = w.run(&[], false, true).unwrap_err();

    assert!(why.contains("can't be read"), "{why}");
    assert_eq!(fs::read_to_string(&devices).unwrap(), "not json at all");
}

#[test]
fn devices_are_created_where_there_are_none() {
    let w = World::new();
    let devices = devices_at(&w);
    put(&devices, &encode(&[device("a", "Phone A")]));
    w.stage(vec![], &[&devices], &[]);
    fs::remove_dir_all(w.at("AgentConsole")).unwrap();

    w.run(&[], false, true).unwrap();

    assert_eq!(read_devices(&devices).unwrap().len(), 1);
    assert_eq!(mode(&devices), 0o600);
}

fn cluster_db(w: &World) -> std::path::PathBuf {
    w.at("data/cluster/cluster.db")
}

#[test]
fn db_restore_is_atomic() {
    let w = World::new();
    let db = cluster_db(&w);
    let live = make_db(&db, 50);
    w.stage(vec![], &[], &[&db]);
    live.execute("INSERT INTO t (v) VALUES ('after the set')", [])
        .unwrap();
    assert_eq!(crate::snap_files_tests::count(&live), 51);
    let live = RefCell::new(Some(live));
    let log = RefCell::new(Vec::new());
    let hooks = DbHooks {
        close: &|| {
            log.borrow_mut().push("close");
            live.borrow_mut().take();
            Ok(())
        },
        reopen: &|| {
            log.borrow_mut().push("reopen");
            Ok(())
        },
    };

    w.run_with(&[], false, true, &hooks).unwrap();

    assert_eq!(*log.borrow(), vec!["close", "reopen"]);
    let restored = Connection::open(&db).unwrap();
    assert_eq!(count(&restored), 50);
    let check: String = restored
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(check, "ok");
    assert_eq!(mode(&db), 0o600);
    let names: Vec<String> = fs::read_dir(db.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names
            .iter()
            .all(|n| !n.contains("nlrestore") && !n.ends_with("-wal")),
        "{names:?}"
    );
}

#[test]
fn a_damaged_stored_database_leaves_the_live_one_alone() {
    let w = World::new();
    let db = cluster_db(&w);
    let live = make_db(&db, 20);
    let manifest = w.stage(vec![], &[], &[&db]);
    let stored = notchling_engine::cluster::standby_store::StandbyStore::new(
        &w.cluster,
        &crate::snap_files_tests::node(),
    )
    .stored(&manifest.set, &db)
    .unwrap();
    let mut bytes = fs::read(&stored).unwrap();
    bytes[2000] ^= 0xff;
    fs::write(&stored, bytes).unwrap();
    let closed = RefCell::new(false);
    let hooks = DbHooks {
        close: &|| {
            *closed.borrow_mut() = true;
            Ok(())
        },
        reopen: &|| Ok(()),
    };

    let why = w.run_with(&[], false, true, &hooks).unwrap_err();

    assert!(why.contains("doesn't match the set"), "{why}");
    assert!(!*closed.borrow());
    assert_eq!(count(&live), 20);
}

#[test]
fn a_database_that_will_not_close_is_not_replaced() {
    let w = World::new();
    let db = cluster_db(&w);
    let live = make_db(&db, 20);
    w.stage(vec![], &[], &[&db]);
    live.execute("INSERT INTO t (v) VALUES ('newer')", [])
        .unwrap();
    let hooks = DbHooks {
        close: &|| Err("a writer is busy".to_owned()),
        reopen: &|| panic!("never opened"),
    };

    let why = w.run_with(&[], false, true, &hooks).unwrap_err();

    assert!(why.contains("a writer is busy"), "{why}");
    assert_eq!(count(&live), 21);
    assert!(!fs::read_dir(db.parent().unwrap()).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("nlrestore")));
}

#[test]
fn restore_steps_never_touch_excluded_dirs() {
    let w = World::new();
    let transcript = w.transcript("s1", b"one\n");
    let secret = w.at("data/cluster/grants/s1-grant.json");
    put(&secret, b"{\"secret\":1}");
    w.stage(
        vec![w.entry("s1", &w.repo, &[&transcript], &[&secret])],
        &[&secret],
        &[],
    );
    fs::remove_file(&secret).unwrap();

    let steps = w.plan(&["s1"], false, true);

    match steps {
        Err(why) => assert!(why.contains("never carries"), "{why}"),
        Ok(steps) => assert!(steps.iter().all(|s| !format!("{s:?}").contains("grants"))),
    }
    assert!(!secret.exists());
}
