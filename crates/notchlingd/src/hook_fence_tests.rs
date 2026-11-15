use crate::hook_fence::{answer, deny_output, read_fence, Seen};
use notchling_engine::cluster::fence::{
    after_wait, Fence, FenceFile, Gate, FENCE_FILE, FENCE_VERSION, MAX_FENCE_BYTES, POLL_MS,
    WAIT_MAX_MS,
};
use notchling_engine::cluster::id::{fingerprint, NodeId};
use notchling_engine::cluster::members::Term;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const BOOT: &str = "boot-1";
const HOUR_MS: u64 = 3_600_000;

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
    let dir = std::env::temp_dir().join(format!("nl-hf-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("cluster")).unwrap();
    Dir(dir)
}

fn node(name: &str) -> NodeId {
    NodeId::from_fingerprint(&fingerprint(name.as_bytes())).unwrap()
}

fn fence(gate: Gate, boot: &str, valid_until: u64) -> FenceFile {
    let mut sessions = BTreeMap::new();
    sessions.insert("s1".to_string(), gate);
    FenceFile {
        v: FENCE_VERSION,
        term: Term {
            n: 2,
            node: node("m5"),
        },
        node: node("m5"),
        boot_id: boot.to_string(),
        valid_until,
        sessions,
    }
}

fn put(dir: &Path, f: &FenceFile) {
    std::fs::write(dir.join(FENCE_FILE), serde_json::to_vec(f).unwrap()).unwrap();
}

fn event(session: &str) -> Vec<u8> {
    json!({"session_id": session, "hook_event_name": "PreToolUse", "tool_name": "Bash"})
        .to_string()
        .into_bytes()
}

struct Clock {
    now: Cell<u64>,
    sleeps: Cell<u64>,
}

impl Clock {
    fn at(ms: u64) -> Clock {
        Clock {
            now: Cell::new(ms),
            sleeps: Cell::new(0),
        }
    }
}

fn run(dir: &Path, stdin: &[u8], boot: &str, clock: &Clock) -> Option<String> {
    answer(stdin, dir, boot, &|| clock.now.get(), &|ms| {
        clock.sleeps.set(clock.sleeps.get() + 1);
        clock.now.set(clock.now.get() + ms);
    })
}

fn denied(out: &Option<String>) -> String {
    let v: Value = serde_json::from_str(out.as_deref().expect("a deny")).unwrap();
    let h = &v["hookSpecificOutput"];
    assert_eq!(h["hookEventName"], "PreToolUse");
    assert_eq!(h["permissionDecision"], "deny");
    h["permissionDecisionReason"].as_str().unwrap().to_string()
}

fn after_wait_text() -> String {
    match after_wait() {
        Fence::Deny(t) => t,
        other => panic!("{other:?}"),
    }
}

#[test]
fn pass_prints_nothing() {
    let dir = scratch("pass");
    let clock = Clock::at(1_000);
    assert_eq!(run(&dir.0, &event("s1"), BOOT, &clock), None);
    put(&dir.0, &fence(Gate::Free, "old-boot", 0));
    assert_eq!(run(&dir.0, &event("s1"), BOOT, &clock), None);
    put(&dir.0, &fence(Gate::Lease, BOOT, 5_000));
    assert_eq!(run(&dir.0, &event("s1"), BOOT, &clock), None);
    assert_eq!(run(&dir.0, &event("other"), BOOT, &clock), None);
    assert_eq!(clock.sleeps.get(), 0);
}

#[test]
fn a_missing_cluster_folder_passes() {
    let dir = scratch("nofolder");
    std::fs::remove_dir_all(dir.0.join("cluster")).unwrap();
    let clock = Clock::at(1);
    assert_eq!(run(&dir.0, &event("s1"), BOOT, &clock), None);
    assert_eq!(clock.sleeps.get(), 0);
}

#[test]
fn deny_prints_pretooluse_deny() {
    let dir = scratch("deny");
    put(&dir.0, &fence(Gate::Moved { to: "m1".into() }, BOOT, 0));
    let out = run(&dir.0, &event("s1"), BOOT, &Clock::at(1));
    assert_eq!(denied(&out), "This session continues on m1. Open it there.");
    let exact: Value = serde_json::from_str(&deny_output("why")).unwrap();
    assert_eq!(
        exact,
        json!({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": "why"}})
    );
}

#[test]
fn moved_session_is_denied_at_once() {
    let dir = scratch("moved");
    put(&dir.0, &fence(Gate::Moved { to: "m1".into() }, "any", 0));
    let clock = Clock::at(1);
    let out = run(&dir.0, &event("s1"), BOOT, &clock);
    assert!(denied(&out).contains("m1"));
    assert_eq!(clock.sleeps.get(), 0);
    put(
        &dir.0,
        &fence(
            Gate::Waiting {
                for_node: "m5".into(),
            },
            BOOT,
            0,
        ),
    );
    let out = run(&dir.0, &event("s1"), BOOT, &clock);
    assert!(denied(&out).contains("Continue From This Copy"));
    assert_eq!(clock.sleeps.get(), 0);
}

#[test]
fn wait_then_pass_when_lease_arrives() {
    let dir = scratch("arrive");
    put(&dir.0, &fence(Gate::Lease, BOOT, 0));
    let clock = Clock::at(10_000);
    let renewed = RefCell::new(false);
    let out = answer(&event("s1"), &dir.0, BOOT, &|| clock.now.get(), &|ms| {
        clock.sleeps.set(clock.sleeps.get() + 1);
        clock.now.set(clock.now.get() + ms);
        if clock.sleeps.get() == 3 {
            put(&dir.0, &fence(Gate::Lease, BOOT, clock.now.get() + HOUR_MS));
            *renewed.borrow_mut() = true;
        }
    });
    assert_eq!(out, None);
    assert!(*renewed.borrow());
    assert_eq!(clock.sleeps.get(), 3);
}

#[test]
fn wait_ends_with_the_gate_that_arrives() {
    let dir = scratch("gate");
    put(&dir.0, &fence(Gate::Lease, BOOT, 0));
    let clock = Clock::at(10_000);
    let out = answer(&event("s1"), &dir.0, BOOT, &|| clock.now.get(), &|ms| {
        clock.sleeps.set(clock.sleeps.get() + 1);
        clock.now.set(clock.now.get() + ms);
        if clock.sleeps.get() == 2 {
            put(&dir.0, &fence(Gate::Moved { to: "m1".into() }, BOOT, 0));
        }
    });
    assert!(denied(&out).contains("m1"));
    assert_eq!(clock.sleeps.get(), 2);
}

#[test]
fn a_fence_file_that_goes_away_while_waiting_passes() {
    let dir = scratch("gone");
    put(&dir.0, &fence(Gate::Lease, BOOT, 0));
    let clock = Clock::at(10_000);
    let out = answer(&event("s1"), &dir.0, BOOT, &|| clock.now.get(), &|ms| {
        clock.sleeps.set(clock.sleeps.get() + 1);
        clock.now.set(clock.now.get() + ms);
        let _ = std::fs::remove_file(dir.0.join(FENCE_FILE));
    });
    assert_eq!(out, None);
}

#[test]
fn wait_then_deny_after_twenty_seconds() {
    let dir = scratch("twenty");
    put(&dir.0, &fence(Gate::Lease, BOOT, 5_000));
    let clock = Clock::at(5_000 + 3 * HOUR_MS);
    let begun = clock.now.get();
    let out = run(&dir.0, &event("s1"), BOOT, &clock);
    assert_eq!(denied(&out), after_wait_text());
    let waited = clock.now.get() - begun;
    assert_eq!(waited, WAIT_MAX_MS);
    assert_eq!(clock.sleeps.get(), WAIT_MAX_MS / POLL_MS);
}

#[test]
fn another_boot_id_or_none_never_passes() {
    let dir = scratch("boot");
    put(&dir.0, &fence(Gate::Lease, "yesterday", u64::MAX));
    let out = run(&dir.0, &event("s1"), BOOT, &Clock::at(1));
    assert_eq!(denied(&out), after_wait_text());
    put(&dir.0, &fence(Gate::Lease, BOOT, u64::MAX));
    let out = run(&dir.0, &event("s1"), "", &Clock::at(1));
    assert_eq!(denied(&out), after_wait_text());
}

#[test]
fn a_stuck_clock_still_ends() {
    let dir = scratch("stuck");
    put(&dir.0, &fence(Gate::Lease, BOOT, 0));
    let sleeps = Cell::new(0u64);
    let out = answer(&event("s1"), &dir.0, BOOT, &|| 7, &|_| {
        sleeps.set(sleeps.get() + 1);
    });
    assert_eq!(denied(&out), after_wait_text());
    assert!(
        sleeps.get() <= WAIT_MAX_MS / POLL_MS + 1,
        "{}",
        sleeps.get()
    );
}

#[test]
fn bad_input_passes_without_reading_the_file() {
    let dir = scratch("input");
    std::fs::write(dir.0.join(FENCE_FILE), b"garbage").unwrap();
    for input in [
        &b""[..],
        b"not json",
        b"{}",
        b"[]",
        br#"{"session_id": 7}"#,
        br#"{"session_id": ""}"#,
        br#"{"session_id": null}"#,
        &[0xff, 0xfe, 0x00][..],
    ] {
        let clock = Clock::at(1);
        assert_eq!(run(&dir.0, input, BOOT, &clock), None, "{input:?}");
        assert_eq!(clock.sleeps.get(), 0);
    }
}

fn expect_unreadable_denies(dir: &Dir, what: &str) {
    let clock = Clock::at(10_000);
    let out = run(&dir.0, &event("s1"), BOOT, &clock);
    assert_eq!(denied(&out), after_wait_text(), "{what}");
    assert_eq!(clock.sleeps.get(), WAIT_MAX_MS / POLL_MS, "{what}");
}

#[test]
fn a_garbage_or_newer_file_waits_then_denies() {
    let dir = scratch("garbage");
    for bytes in [
        b"".to_vec(),
        b"{".to_vec(),
        b"null".to_vec(),
        br#"{"v": 99}"#.to_vec(),
    ] {
        std::fs::write(dir.0.join(FENCE_FILE), &bytes).unwrap();
        expect_unreadable_denies(&dir, &String::from_utf8_lossy(&bytes));
    }
}

#[test]
fn an_oversized_file_waits_then_denies_and_is_read_only_that_far() {
    let dir = scratch("big");
    let path = dir.0.join(FENCE_FILE);
    let f = std::fs::File::create(&path).unwrap();
    f.set_len(1 << 30).unwrap();
    drop(f);
    match read_fence(&path) {
        Seen::Bytes(b) => assert_eq!(b.len(), MAX_FENCE_BYTES + 1),
        other => panic!("{other:?}"),
    }
    expect_unreadable_denies(&dir, "oversized");
    let mut ok = serde_json::to_vec(&fence(Gate::Free, BOOT, 0)).unwrap();
    ok.resize(MAX_FENCE_BYTES + 1, b' ');
    std::fs::write(&path, ok).unwrap();
    expect_unreadable_denies(&dir, "one byte over");
}

#[test]
fn a_directory_at_the_path_waits_then_denies() {
    let dir = scratch("isdir");
    std::fs::create_dir(dir.0.join(FENCE_FILE)).unwrap();
    assert_eq!(read_fence(&dir.0.join(FENCE_FILE)), Seen::Unreadable);
    expect_unreadable_denies(&dir, "directory");
}

#[cfg(unix)]
#[test]
fn a_link_at_the_path_is_never_followed() {
    let dir = scratch("link");
    let real = dir.0.join("real.json");
    std::fs::write(
        &real,
        serde_json::to_vec(&fence(Gate::Free, BOOT, 0)).unwrap(),
    )
    .unwrap();
    std::os::unix::fs::symlink(&real, dir.0.join(FENCE_FILE)).unwrap();
    assert_eq!(read_fence(&dir.0.join(FENCE_FILE)), Seen::Unreadable);
    expect_unreadable_denies(&dir, "link to a good file");
    std::fs::remove_file(dir.0.join(FENCE_FILE)).unwrap();
    std::os::unix::fs::symlink(dir.0.join("nowhere"), dir.0.join(FENCE_FILE)).unwrap();
    expect_unreadable_denies(&dir, "dangling link");
}

#[cfg(unix)]
#[test]
fn a_fifo_at_the_path_does_not_block() {
    let dir = scratch("fifo");
    let path = dir.0.join(FENCE_FILE);
    let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let started = std::time::Instant::now();
    assert_eq!(read_fence(&path), Seen::Unreadable);
    assert!(started.elapsed().as_secs() < 5);
    expect_unreadable_denies(&dir, "fifo");
}

#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_opened_waits_then_denies() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("perm");
    let path = dir.0.join(FENCE_FILE);
    put(&dir.0, &fence(Gate::Free, BOOT, 0));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::File::open(&path).is_ok() {
        return;
    }
    assert_eq!(read_fence(&path), Seen::Unreadable);
    expect_unreadable_denies(&dir, "mode 000");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[cfg(unix)]
#[test]
fn only_not_found_means_no_file() {
    let dir = scratch("notdir");
    std::fs::remove_dir_all(dir.0.join("cluster")).unwrap();
    std::fs::write(dir.0.join("cluster"), b"a file where the folder should be").unwrap();
    assert_eq!(read_fence(&dir.0.join(FENCE_FILE)), Seen::Unreadable);
    expect_unreadable_denies(&dir, "a file in place of the folder");
    assert_eq!(read_fence(&dir.0.join("missing/fence.json")), Seen::Absent);
}
