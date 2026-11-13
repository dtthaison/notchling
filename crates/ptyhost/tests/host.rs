mod common;

use common::*;
use notchling_ptyhost::state::{decide, HostRecord, HostSettings, StateStore, UpdateAction};
use notchling_ptyhost::wire::{self, Frame, FrameReader, ReadFailure, SessionId};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::sync::Arc;

fn object(value: Value) -> Map<String, Value> {
    value.as_object().expect("an object").clone()
}

#[test]
fn frames_round_trip_and_survive_being_split_anywhere() {
    let id = SessionId::random();
    let json = wire::encode(&Frame::Json(object(json!({
        "id": 7, "op": "list", "ok": true, "nested": {"a": [1, "x"]}
    }))));
    let binary = wire::encode(&Frame::Data {
        session: id,
        bytes: vec![0, 1, 2, 255],
    });
    let stream: Vec<u8> = json.into_iter().chain(binary).collect();
    let mut reader = FrameReader::new();
    let mut frames = Vec::new();
    for byte in stream {
        frames.extend(reader.feed(&[byte]));
    }
    assert_eq!(frames.len(), 2);
    let Frame::Json(object) = &frames[0] else {
        panic!("json first")
    };
    assert_eq!(object["id"], json!(7));
    assert_eq!(object["ok"], json!(true));
    assert_eq!(object["nested"]["a"], json!([1, "x"]));
    assert_eq!(
        frames[1],
        Frame::Data {
            session: id,
            bytes: vec![0, 1, 2, 255]
        }
    );
    assert_eq!(reader.failure(), None);
}

#[test]
fn oversized_and_malformed_frames_end_the_stream() {
    let mut big = FrameReader::new();
    assert!(big.feed(&[0x7F, 0xFF, 0xFF, 0xFF, 1]).is_empty());
    assert_eq!(big.failure(), Some(ReadFailure::TooLarge));
    let mut bad = FrameReader::new();
    bad.feed(&[0, 0, 0, 3, 9, 1, 2]);
    assert_eq!(
        bad.failure(),
        Some(ReadFailure::Malformed),
        "unknown frame type"
    );
    let mut not_json = FrameReader::new();
    not_json.feed(&[0, 0, 0, 3, 1, 0x7B, 0x7B]);
    assert_eq!(not_json.failure(), Some(ReadFailure::Malformed));
    assert!(
        bad.feed(&wire::json(object(json!({"id": 1})))).is_empty(),
        "nothing is read after a failure"
    );
}

#[test]
fn the_bytes_are_the_swift_layout() {
    let id = SessionId::parse("00112233-4455-6677-8899-AABBCCDDEEFF").expect("parses");
    let frame = wire::encode(&Frame::Data {
        session: id,
        bytes: b"hi".to_vec(),
    });
    let mut expected = vec![0, 0, 0, 19, 2];
    expected.extend_from_slice(&[
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE,
        0xFF,
    ]);
    expected.extend_from_slice(b"hi");
    assert_eq!(frame, expected);
    let json = wire::json(object(json!({"op": "hello", "id": 1})));
    assert_eq!(&json[..5], &[0, 0, 0, 22, 1]);
    assert_eq!(&json[5..], br#"{"id":1,"op":"hello"}"#, "sorted keys");
}

#[test]
fn session_ids_print_as_swift_uuid_strings() {
    let id = SessionId::parse("e621e1f8-c36c-495a-93fc-0c247a3e6e5f").expect("lower case parses");
    assert_eq!(id.to_string(), "E621E1F8-C36C-495A-93FC-0C247A3E6E5F");
    assert_eq!(SessionId::parse(&id.to_string()), Some(id));
    assert_eq!(SessionId::parse("not-a-uuid"), None);
    assert_eq!(
        SessionId::parse("E621E1F8C36C495A93FC0C247A3E6E5F"),
        None,
        "dashes are required"
    );
    let random = SessionId::random();
    assert_ne!(random, SessionId::random());
    assert_eq!(random.bytes()[6] >> 4, 4, "version 4");
}

#[test]
fn spawn_and_resume_use_the_same_arguments_as_the_app() {
    let scratch = Scratch::new();
    let spawner = FakeSpawner::new();
    let (host, state) = make_host(&scratch, spawner.clone(), never_alive, nothing_found);
    let new = host
        .spawn_claude(Path::new("/tmp"), Some("haiku"))
        .expect("spawned");
    assert_eq!(spawner.spawns()[0].arguments, ["--model", "haiku"]);
    assert_eq!(spawner.spawns()[0].executable, "/fake/claude");
    let resumed = host.resume("abc-123", Path::new("/tmp")).expect("resumed");
    assert_eq!(spawner.spawns()[1].arguments, ["--resume", "abc-123"]);
    let mut listed: Vec<String> = host.list().iter().map(|i| i.id.to_string()).collect();
    listed.sort();
    let mut expected = vec![new.id.to_string(), resumed.id.to_string()];
    expected.sort();
    assert_eq!(listed, expected);
    assert_eq!(state.records().len(), 2);
    host.spawn_command(
        Path::new("/bin/zsh"),
        &["-f".to_string()],
        Path::new("/tmp"),
    )
    .expect("spawned");
    assert_eq!(
        state.records().len(),
        2,
        "a plain command is not recorded: it cannot be resumed"
    );
}

#[test]
fn the_claude_session_behind_a_new_session_is_found_and_recorded() {
    let scratch = Scratch::new();
    let (host, state) = make_host(&scratch, FakeSpawner::new(), never_alive, |_| {
        Some("found-id".to_string())
    });
    let info = host.spawn_claude(Path::new("/tmp"), None).expect("spawned");
    wait_until(|| {
        state
            .records()
            .first()
            .and_then(|r| r.claude_session_id.clone())
            .is_some()
    });
    assert_eq!(
        state.records()[0].claude_session_id.as_deref(),
        Some("found-id")
    );
    let listed = host.list();
    let found = listed.iter().find(|i| i.id == info.id).expect("listed");
    assert_eq!(found.claude_session_id.as_deref(), Some("found-id"));
}

#[test]
fn a_session_that_ends_is_forgotten() {
    let scratch = Scratch::new();
    let spawner = FakeSpawner::new();
    let (host, state) = make_host(&scratch, spawner.clone(), never_alive, nothing_found);
    host.spawn_claude(Path::new("/tmp"), None).expect("spawned");
    assert_eq!(state.records().len(), 1);
    spawner.handle(0).finish();
    assert!(
        state.records().is_empty(),
        "it ended by itself: not brought back later"
    );
    assert!(host.list().is_empty());
}

fn record(id: &str, cwd: &Path, claude: Option<&str>, pid: i32) -> HostRecord {
    HostRecord {
        id: id.to_string(),
        cwd: cwd.to_string_lossy().into_owned(),
        model: None,
        claude_session_id: claude.map(String::from),
        pid,
        started_at_ms: now_ms() / 1000 * 1000,
    }
}

#[test]
fn recorded_sessions_with_dead_processes_are_resumed_at_start() {
    let scratch = Scratch::new();
    let work = scratch.path("work");
    std::fs::create_dir_all(&work).expect("work");
    let state = Arc::new(StateStore::new(scratch.path("state.json")));
    state.upsert(record("a", &work, Some("session-a"), 111));
    state.upsert(record("b", &work, Some("session-b"), 222));
    state.upsert(record("c", &work, None, 333));
    state.upsert(record(
        "d",
        Path::new("/definitely/not/here"),
        Some("session-d"),
        444,
    ));
    state.upsert(record("e", &work, Some("session-e"), 0));
    let spawner = FakeSpawner::new();
    let options = notchling_ptyhost::host::HostOptions {
        is_alive: Arc::new(|pid| pid == 222),
        discover_session_id: Arc::new(|_| None),
        ..Default::default()
    };
    let host = notchling_ptyhost::host::SessionHost::new(spawner.clone(), state.clone(), options);

    assert_eq!(host.resume_recorded(), 2);
    let mut args: Vec<Vec<String>> = spawner.spawns().into_iter().map(|s| s.arguments).collect();
    args.sort();
    assert_eq!(args, [["--resume", "session-a"], ["--resume", "session-e"]]);
    let work_text = work.to_string_lossy().into_owned();
    assert!(spawner.spawns().iter().all(|s| s.cwd == work_text));
    let kept: Vec<Option<String>> = state
        .records()
        .into_iter()
        .map(|r| r.claude_session_id)
        .collect();
    assert!(
        kept.contains(&Some("session-b".to_string())),
        "a record whose process is alive is left alone"
    );
    assert!(
        !kept.contains(&None),
        "a record with no Claude id is dropped"
    );
}

#[test]
fn resume_on_start_can_be_switched_off() {
    let scratch = Scratch::new();
    let work = scratch.path("w");
    std::fs::create_dir_all(&work).expect("work");
    let state = Arc::new(StateStore::new(scratch.path("state.json")));
    state.upsert(record("a", &work, Some("s"), 0));
    state.set_settings(HostSettings {
        resume_on_start: false,
    });
    let spawner = FakeSpawner::new();
    let host =
        notchling_ptyhost::host::SessionHost::new(spawner.clone(), state, Default::default());
    assert_eq!(host.resume_recorded(), 0);
    assert!(spawner.spawns().is_empty());
    assert!(
        !StateStore::new(scratch.path("state.json"))
            .settings()
            .resume_on_start,
        "the setting is persisted"
    );
    assert!(HostSettings::default().resume_on_start, "on by default");
}

#[test]
fn migration_records_are_resumed_by_the_next_host() {
    let scratch = Scratch::new();
    let work = scratch.path("w");
    std::fs::create_dir_all(&work).expect("work");
    let (first, state) = make_host(&scratch, FakeSpawner::new(), never_alive, nothing_found);
    first.record_for_resume("migrated", &work.to_string_lossy(), None);
    let spawner = FakeSpawner::new();
    let second = notchling_ptyhost::host::SessionHost::new(
        spawner.clone(),
        Arc::new(StateStore::new(scratch.path("state.json"))),
        notchling_ptyhost::host::HostOptions {
            is_alive: Arc::new(|_| false),
            ..Default::default()
        },
    );
    assert_eq!(second.resume_recorded(), 1);
    assert_eq!(spawner.spawns()[0].arguments, ["--resume", "migrated"]);
    assert_eq!(state.records().len(), 1);
}

#[cfg(unix)]
#[test]
fn state_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let path = scratch.path("sub/state.json");
    StateStore::new(&path).upsert(record("a", Path::new("/tmp"), Some("s"), 1));
    let mode = |p: &Path| std::fs::metadata(p).expect("exists").permissions().mode() & 0o777;
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&scratch.path("sub")), 0o700);
}

#[test]
fn the_state_file_reads_what_swift_wrote() {
    let scratch = Scratch::new();
    let path = scratch.path("state.json");
    std::fs::write(
        &path,
        r#"{
  "records" : [
    {
      "claudeSessionId" : "s-1",
      "cwd" : "/tmp/w",
      "id" : "A1",
      "pid" : 42,
      "startedAt" : "2026-11-08T10:00:00Z"
    }
  ],
  "settings" : {
    "resumeOnStart" : false
  }
}"#,
    )
    .expect("write");
    let store = StateStore::new(&path);
    let records = store.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].claude_session_id.as_deref(), Some("s-1"));
    assert_eq!(records[0].model, None);
    assert_eq!(records[0].started_at_ms, 1_794_132_000_000);
    assert!(!store.settings().resume_on_start);

    store.upsert(record("B2", Path::new("/tmp/x"), None, 7));
    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
    assert_eq!(
        written["records"][0]["startedAt"],
        json!("2026-11-08T10:00:00Z")
    );
    assert!(
        written["records"][1].get("claudeSessionId").is_none(),
        "a missing id is left out, as Swift's encoder does"
    );

    std::fs::write(&path, "not json").expect("write");
    assert!(
        StateStore::new(&path).records().is_empty(),
        "unreadable: starting empty"
    );
}

#[test]
fn an_updated_helper_restarts_only_when_nothing_is_running() {
    assert_eq!(decide(false, 0), UpdateAction::Nothing);
    assert_eq!(decide(false, 3), UpdateAction::Nothing);
    assert_eq!(decide(true, 0), UpdateAction::RestartNow);
    assert_eq!(decide(true, 2), UpdateAction::DeferUntilIdle);
}
