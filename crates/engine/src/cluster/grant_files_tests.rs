use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::grant_files::{GrantFile, GrantStore, GrantValue, EXCLUDED_DIRS};
use crate::sessions::testutil::TempDir;

pub(super) const JOB: &str = "j-0123456789ab";
pub(super) const JOB2: &str = "j-ba9876543210";
pub(super) const MINUTE: i64 = 60_000;

pub(super) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(super) fn grant(job: &str, name: &str, expires_ms: i64, value: &str) -> GrantFile {
    GrantFile {
        job: job.to_string(),
        name: name.to_string(),
        expires_ms,
        value: GrantValue::new(value.to_string()),
    }
}

pub(super) fn store(tmp: &TempDir) -> GrantStore {
    GrantStore::new(&tmp.path("cluster"))
}

pub(super) fn root(tmp: &TempDir) -> PathBuf {
    tmp.path("cluster/grants")
}

pub(super) fn live(jobs: &[&str]) -> BTreeSet<String> {
    jobs.iter().map(|j| j.to_string()).collect()
}

pub(super) fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|rd| {
            rd.map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[cfg(unix)]
pub(super) fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("chmod");
}

#[cfg(unix)]
pub(super) fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).expect("meta").permissions().mode() & 0o777
}

#[test]
fn put_then_get() {
    let tmp = TempDir::new("grant-put-get");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "gh_token", n + MINUTE, "NL-MARKER-1"))
        .expect("put");
    let got = store
        .get(JOB, "gh_token", n)
        .expect("get")
        .expect("present");
    assert_eq!(got.job, JOB);
    assert_eq!(got.name, "gh_token");
    assert_eq!(got.expires_ms, n + MINUTE);
    assert!(got.value.expose() == "NL-MARKER-1");
    assert!(store.get(JOB, "other", n).expect("get").is_none());
    assert!(store.get(JOB2, "gh_token", n).expect("get").is_none());
}

#[test]
fn stored_format_is_versioned_json() {
    let tmp = TempDir::new("grant-format");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "a", n + MINUTE, "NL-MARKER-2"))
        .expect("put");
    let text = fs::read_to_string(root(&tmp).join(JOB).join("a.json")).expect("read");
    let json: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(json["v"], 1);
    assert_eq!(json["job"], JOB);
    assert_eq!(json["name"], "a");
    assert_eq!(json["expiresMs"], n + MINUTE);
    assert!(json["value"] == "NL-MARKER-2");
}

#[test]
fn names_are_case_insensitive() {
    let tmp = TempDir::new("grant-case");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "GH_TOKEN", n + MINUTE, "NL-MARKER-35"))
        .expect("put");
    assert_eq!(
        listing(&root(&tmp).join(JOB)),
        vec!["gh_token.json".to_string()]
    );
    for ask in ["gh_token", "GH_TOKEN", "Gh_Token"] {
        let got = store.get(JOB, ask, n).expect("get").expect("present");
        assert_eq!(got.name, "gh_token");
        assert!(got.value.expose() == "NL-MARKER-35");
    }
    store
        .put(&grant(JOB, "gh_token", n + 2 * MINUTE, "NL-MARKER-36"))
        .expect("replace");
    assert_eq!(listing(&root(&tmp).join(JOB)).len(), 1);
    store.remove(JOB, "GH_Token").expect("remove");
    assert!(store.get(JOB, "gh_token", n).expect("get").is_none());
}

#[test]
fn an_expired_or_oversize_grant_is_not_written() {
    let tmp = TempDir::new("grant-put-refused");
    let store = store(&tmp);
    let n = now();
    assert!(store.put(&grant(JOB, "a", n - 1, "NL-MARKER-37")).is_err());
    assert!(store.put(&grant(JOB, "a", 0, "NL-MARKER-38")).is_err());
    let big = "x".repeat(40 * 1024);
    assert!(store.put(&grant(JOB, "a", n + MINUTE, &big)).is_err());
    assert!(!tmp.path("cluster").exists());
}

#[cfg(unix)]
#[test]
fn files_are_0600_in_0700_folders() {
    let tmp = TempDir::new("grant-modes");
    let store = store(&tmp);
    store
        .put(&grant(JOB, "a", now() + MINUTE, "NL-MARKER-3"))
        .expect("put");
    assert_eq!(mode_of(&root(&tmp)), 0o700);
    assert_eq!(mode_of(&root(&tmp).join(JOB)), 0o700);
    assert_eq!(mode_of(&root(&tmp).join(JOB).join("a.json")), 0o600);
}

#[test]
fn sweep_lists_every_entry_of_a_big_folder() {
    let tmp = TempDir::new("grant-sweep-big");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "keep", n + 10 * MINUTE, "NL-MARKER-54"))
        .expect("put");
    let folder = root(&tmp).join(JOB);
    for i in 0..700 {
        fs::write(folder.join(format!("stray{i}.txt")), "x").expect("write");
    }
    assert_eq!(store.sweep(n, &live(&[JOB])).expect("sweep"), 700);
    assert_eq!(listing(&folder), vec!["keep.json".to_string()]);
}

#[test]
fn expired_grant_is_deleted_on_get() {
    let tmp = TempDir::new("grant-expired");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "a", n + MINUTE, "NL-MARKER-10"))
        .expect("put");
    assert!(store.get(JOB, "a", n + MINUTE).expect("get").is_none());
    assert!(!root(&tmp).join(JOB).join("a.json").exists());
    store
        .put(&grant(JOB, "b", n + MINUTE, "NL-MARKER-11"))
        .expect("put");
    assert!(store.get(JOB, "b", n + MINUTE - 1).expect("get").is_some());
    assert!(store.get(JOB, "b", n + MINUTE).expect("get").is_none());
}

#[test]
fn sweep_removes_expired_and_dead_jobs() {
    let tmp = TempDir::new("grant-sweep");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "old", n + MINUTE, "NL-MARKER-12"))
        .expect("put");
    store
        .put(&grant(JOB, "fresh", n + 10 * MINUTE, "NL-MARKER-13"))
        .expect("put");
    store
        .put(&grant(JOB2, "fresh", n + 10 * MINUTE, "NL-MARKER-14"))
        .expect("put");
    let later = n + 2 * MINUTE;
    let removed = store.sweep(later, &live(&[JOB])).expect("sweep");
    assert_eq!(removed, 2);
    assert!(store.get(JOB, "fresh", later).expect("get").is_some());
    assert!(!root(&tmp).join(JOB).join("old.json").exists());
    assert!(!root(&tmp).join(JOB2).exists());
    assert_eq!(store.sweep(later, &live(&[JOB])).expect("sweep"), 0);
}

#[test]
fn sweep_without_a_root_is_nothing() {
    let tmp = TempDir::new("grant-sweep-empty");
    assert_eq!(store(&tmp).sweep(now(), &live(&[])).expect("sweep"), 0);
    assert!(!root(&tmp).exists());
}

#[test]
fn sweep_deletes_malformed_and_stray_files_in_live_jobs() {
    let tmp = TempDir::new("grant-sweep-bad");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "ok", n + 10 * MINUTE, "NL-MARKER-15"))
        .expect("put");
    let folder = root(&tmp).join(JOB);
    fs::write(folder.join("broken.json"), "{ NL-MARKER-16").expect("write");
    fs::write(folder.join("stray.txt"), "x").expect("write");
    assert_eq!(store.sweep(n, &live(&[JOB])).expect("sweep"), 2);
    assert_eq!(listing(&folder), vec!["ok.json".to_string()]);
}

#[test]
fn bad_job_id_builds_no_path() {
    let tmp = TempDir::new("grant-bad-ids");
    let store = store(&tmp);
    let n = now();
    let bad_jobs = [
        "../x",
        "j-ZZZZZZZZZZZZ",
        "j-0123456789a",
        "j-0123456789abc",
        "j-0123456789AB",
        "x-0123456789ab",
        "",
        "j-0123456789ab/..",
    ];
    for job in bad_jobs {
        assert!(store
            .put(&grant(job, "a", n + MINUTE, "NL-MARKER-19"))
            .is_err());
        assert!(store.get(job, "a", n).is_err());
        assert!(store.remove(job, "a").is_err());
        assert!(store.remove_job(job).is_err());
    }
    for name in ["../a", "a/b", "", ".hidden", "1abc", "a b", "a\0b"] {
        assert!(store
            .put(&grant(JOB, name, n + MINUTE, "NL-MARKER-20"))
            .is_err());
        assert!(store.get(JOB, name, n).is_err());
        assert!(store.remove(JOB, name).is_err());
    }
    assert!(!tmp.path("cluster").exists());
    assert!(!tmp.path("x").exists());
    assert!(!tmp.path("a").exists());
}

#[test]
fn grant_debug_hides_value() {
    let marker = "NL-MARKER-21";
    let value = GrantValue::new(marker.to_string());
    let shown = format!("{value:?}");
    assert!(!shown.contains(marker));
    assert!(shown.contains("<hidden>"));
    let shown = format!("{value}");
    assert!(!shown.contains(marker));
    assert!(shown.contains("<hidden>"));
    let file = grant(JOB, "a", 1, marker);
    let shown = format!("{file:?}");
    assert!(!shown.contains(marker));
    let pretty = format!("{file:#?}");
    assert!(!pretty.contains(marker));
}

#[cfg(unix)]
#[test]
fn put_replaces_by_remove_then_create() {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    let tmp = TempDir::new("grant-replace");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "a", n + MINUTE, "NL-MARKER-22"))
        .expect("put");
    let path = root(&tmp).join(JOB).join("a.json");
    let mut old = fs::File::open(&path).expect("open");
    let old_inode = old.metadata().expect("meta").ino();
    store
        .put(&grant(JOB, "a", n + 2 * MINUTE, "NL-MARKER-23"))
        .expect("put again");
    let new_inode = fs::metadata(&path).expect("meta").ino();
    assert_ne!(old_inode, new_inode);
    let mut old_text = String::new();
    old.read_to_string(&mut old_text).expect("read");
    assert!(old_text.contains("NL-MARKER-22"));
    let got = store.get(JOB, "a", n).expect("get").expect("present");
    assert!(got.value.expose() == "NL-MARKER-23");
    assert_eq!(got.expires_ms, n + 2 * MINUTE);
}

#[test]
fn remove_and_remove_job_delete_and_are_idempotent() {
    let tmp = TempDir::new("grant-remove");
    let store = store(&tmp);
    let n = now();
    store.remove(JOB, "a").expect("nothing yet");
    store.remove_job(JOB).expect("nothing yet");
    store
        .put(&grant(JOB, "a", n + MINUTE, "NL-MARKER-24"))
        .expect("put");
    store
        .put(&grant(JOB, "b", n + MINUTE, "NL-MARKER-25"))
        .expect("put");
    store.remove(JOB, "a").expect("remove");
    store.remove(JOB, "a").expect("again");
    assert!(store.get(JOB, "a", n).expect("get").is_none());
    assert!(store.get(JOB, "b", n).expect("get").is_some());
    store.remove_job(JOB).expect("remove_job");
    assert!(!root(&tmp).join(JOB).exists());
    store.remove_job(JOB).expect("again");
}

#[test]
fn malformed_file_is_an_error_without_content() {
    let tmp = TempDir::new("grant-malformed");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "a", n + MINUTE, "NL-MARKER-26"))
        .expect("put");
    let path = root(&tmp).join(JOB).join("a.json");
    let far = n + 100 * MINUTE;
    let cases = [
        "{\"v\":1,\"job\":\"j-0123456789ab\",\"name\":\"a\",\"expiresMs\":NL-MARKER-27}".to_string(),
        format!("{{\"v\":2,\"job\":\"j-0123456789ab\",\"name\":\"a\",\"expiresMs\":{far},\"value\":\"NL-MARKER-28\"}}"),
        "{\"v\":1,\"job\":\"j-0123456789ab\",\"name\":\"a\",\"expiresMs\":\"NL-MARKER-29\",\"value\":\"x\"}".to_string(),
        format!("{{\"v\":1,\"job\":\"j-ba9876543210\",\"name\":\"a\",\"expiresMs\":{far},\"value\":\"NL-MARKER-30\"}}"),
        format!("{{\"v\":1,\"job\":\"j-0123456789ab\",\"name\":\"b\",\"expiresMs\":{far},\"value\":\"NL-MARKER-31\"}}"),
        format!("NL-MARKER-32{}", "x".repeat(300_000)),
    ];
    for case in cases {
        fs::remove_file(&path).expect("remove");
        fs::write(&path, case).expect("write");
        #[cfg(unix)]
        chmod(&path, 0o600);
        let err = store.get(JOB, "a", n).expect_err("malformed");
        assert!(!err.contains("NL-MARKER"));
        assert!(err.contains("a.json"));
    }
}

#[test]
fn excluded_dirs_cover_secret_folders() {
    assert!(EXCLUDED_DIRS.contains(&"cluster/grants"));
    assert!(EXCLUDED_DIRS.contains(&"cluster/logins"));
    assert!(EXCLUDED_DIRS.contains(&"cluster/incoming"));
}
