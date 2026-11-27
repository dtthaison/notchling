use std::fs;

use super::*;
use crate::cluster::grant_files::EXCLUDED_DIRS;
use crate::sessions::testutil::TempDir;

const MARKER: &str = "NL-MARKER-710";
const MINUTE: i64 = 60_000;

fn now() -> i64 {
    clock_ms()
}

fn login(expires_ms: i64, value: &str) -> LoginFile {
    LoginFile {
        tool: CLAUDE.to_string(),
        expires_ms,
        value: GrantValue::new(value.to_string()),
    }
}

fn store(tmp: &TempDir) -> LoginStore {
    LoginStore::new(&tmp.path("cluster"))
}

#[cfg(unix)]
#[test]
fn put_then_get_then_remove() {
    let tmp = TempDir::new("login-put-get");
    let store = store(&tmp);
    let n = now();
    store.put(&login(n + MINUTE, MARKER)).expect("put");
    let got = store.get(CLAUDE, n).expect("get").expect("present");
    assert_eq!(got.expires_ms, n + MINUTE);
    assert!(got.value.expose() == MARKER);
    assert!(store.standing(CLAUDE, n));
    store.remove(CLAUDE).expect("remove");
    assert!(store.get(CLAUDE, n).expect("get").is_none());
    assert!(!store.standing(CLAUDE, n));
}

#[cfg(unix)]
#[test]
fn the_file_is_cluster_logins_claude_json_and_excluded_from_snapshots() {
    let tmp = TempDir::new("login-path");
    let store = store(&tmp);
    store.put(&login(now() + MINUTE, MARKER)).expect("put");
    assert_eq!(
        store.path_of(CLAUDE),
        tmp.path("cluster/logins/claude.json")
    );
    assert!(store.path_of(CLAUDE).is_file());
    assert!(EXCLUDED_DIRS.contains(&"cluster/logins"));
}

#[cfg(unix)]
#[test]
fn the_file_is_0600_in_a_0700_folder() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new("login-modes");
    let store = store(&tmp);
    store.put(&login(now() + MINUTE, MARKER)).expect("put");
    let mode = |p: std::path::PathBuf| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(tmp.path("cluster/logins")), 0o700);
    assert_eq!(mode(tmp.path("cluster/logins/claude.json")), 0o600);
}

#[cfg(unix)]
#[test]
fn an_expired_login_is_deleted_on_get_and_is_not_standing() {
    let tmp = TempDir::new("login-expired");
    let store = store(&tmp);
    let n = now();
    store.put(&login(n + MINUTE, MARKER)).expect("put");
    assert!(store.get(CLAUDE, n + 2 * MINUTE).expect("get").is_none());
    assert!(!store.path_of(CLAUDE).exists());
    assert!(!store.standing(CLAUDE, n + 2 * MINUTE));
}

#[test]
fn an_already_expired_login_is_refused() {
    let tmp = TempDir::new("login-put-expired");
    let store = store(&tmp);
    assert!(store.put(&login(now() - MINUTE, MARKER)).is_err());
    assert!(!store.path_of(CLAUDE).exists());
}

#[cfg(unix)]
#[test]
fn a_second_put_replaces_the_first() {
    let tmp = TempDir::new("login-replace");
    let store = store(&tmp);
    let n = now();
    store.put(&login(n + MINUTE, MARKER)).expect("put");
    store
        .put(&login(n + 5 * MINUTE, "NL-MARKER-711"))
        .expect("put");
    let got = store.get(CLAUDE, n).expect("get").expect("present");
    assert!(got.value.expose() == "NL-MARKER-711");
    assert_eq!(got.expires_ms, n + 5 * MINUTE);
}

#[test]
fn only_claude_has_a_standing_login_and_names_build_no_paths() {
    let tmp = TempDir::new("login-tools");
    let store = store(&tmp);
    let mut other = login(now() + MINUTE, MARKER);
    other.tool = "../x".to_string();
    assert!(store.put(&other).is_err());
    assert!(store.get("codex", now()).is_err());
    assert!(store.remove("../claude").is_err());
}

#[test]
fn a_value_with_a_space_a_newline_or_no_text_is_refused() {
    let tmp = TempDir::new("login-values");
    let store = store(&tmp);
    for bad in ["", "two words", "line\nbreak", "tab\there", "nul\0byte"] {
        assert!(
            store.put(&login(now() + MINUTE, bad)).is_err(),
            "a bad value was accepted"
        );
    }
    assert!(store
        .put(&login(now() + MINUTE, &"a".repeat(33 * 1024)))
        .is_err());
    assert!(!store.path_of(CLAUDE).exists());
}

#[cfg(unix)]
#[test]
fn a_planted_link_at_the_file_is_refused_and_its_target_is_untouched() {
    let tmp = TempDir::new("login-link-file");
    let store = store(&tmp);
    store.put(&login(now() + MINUTE, MARKER)).expect("put");
    store.remove(CLAUDE).expect("remove");
    let target = tmp.path("victim");
    fs::write(&target, "precious").unwrap();
    std::os::unix::fs::symlink(&target, store.path_of(CLAUDE)).unwrap();
    assert!(store.put(&login(now() + MINUTE, MARKER)).is_err());
    assert!(store.get(CLAUDE, now()).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "precious");
}

#[cfg(unix)]
#[test]
fn a_planted_link_at_the_folder_is_refused() {
    let tmp = TempDir::new("login-link-dir");
    let real = tmp.path("elsewhere");
    fs::create_dir_all(&real).unwrap();
    fs::create_dir_all(tmp.path("cluster")).unwrap();
    std::os::unix::fs::symlink(&real, tmp.path("cluster/logins")).unwrap();
    let store = store(&tmp);
    assert!(store.put(&login(now() + MINUTE, MARKER)).is_err());
    assert!(fs::read_dir(&real).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn a_file_open_to_others_is_not_served() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new("login-loose");
    let store = store(&tmp);
    store.put(&login(now() + MINUTE, MARKER)).expect("put");
    fs::set_permissions(store.path_of(CLAUDE), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.get(CLAUDE, now()).is_err());
    assert!(!store.standing(CLAUDE, now()));
}

#[test]
fn a_login_prints_hidden() {
    let file = login(now() + MINUTE, MARKER);
    let shown = format!("{file:?}{}", file.value);
    assert!(!shown.contains("NL-MARKER"), "a login printed its value");
}
