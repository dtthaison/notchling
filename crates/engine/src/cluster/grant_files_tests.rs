use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::grant_files::{GrantFile, GrantStore, GrantValue, EXCLUDED_DIRS};
use crate::sessions::testutil::TempDir;

const JOB: &str = "j-0123456789ab";
const JOB2: &str = "j-ba9876543210";
const NOW: i64 = 1_000_000;

fn grant(job: &str, name: &str, expires_ms: i64, value: &str) -> GrantFile {
    GrantFile {
        job: job.to_string(),
        name: name.to_string(),
        expires_ms,
        value: GrantValue::new(value.to_string()),
    }
}

fn store(tmp: &TempDir) -> GrantStore {
    GrantStore::new(&tmp.path("cluster"))
}

fn root(tmp: &TempDir) -> PathBuf {
    tmp.path("cluster/grants")
}

fn live(jobs: &[&str]) -> BTreeSet<String> {
    jobs.iter().map(|j| j.to_string()).collect()
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|rd| {
            rd.map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn put_then_get() {
    let tmp = TempDir::new("grant-put-get");
    let store = store(&tmp);
    store
        .put(&grant(JOB, "gh_token", NOW + 10, "NL-MARKER-1"))
        .expect("put");
    let got = store
        .get(JOB, "gh_token", NOW)
        .expect("get")
        .expect("present");
    assert_eq!(got.job, JOB);
    assert_eq!(got.name, "gh_token");
    assert_eq!(got.expires_ms, NOW + 10);
    assert!(got.value.expose() == "NL-MARKER-1");
    assert!(store.get(JOB, "other", NOW).expect("get").is_none());
    assert!(store.get(JOB2, "gh_token", NOW).expect("get").is_none());
}

#[test]
fn stored_format_is_versioned_json() {
    let tmp = TempDir::new("grant-format");
    let store = store(&tmp);
    store
        .put(&grant(JOB, "a", NOW + 5, "NL-MARKER-2"))
        .expect("put");
    let text = fs::read_to_string(root(&tmp).join(JOB).join("a.json")).expect("read");
    let json: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(json["v"], 1);
    assert_eq!(json["job"], JOB);
    assert_eq!(json["name"], "a");
    assert_eq!(json["expiresMs"], NOW + 5);
    assert!(json["value"] == "NL-MARKER-2");
}

#[cfg(unix)]
#[test]
fn files_are_0600_in_0700_folders() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new("grant-modes");
    let store = store(&tmp);
    store
        .put(&grant(JOB, "a", NOW + 5, "NL-MARKER-3"))
        .expect("put");
    let mode = |p: PathBuf| fs::metadata(p).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode(root(&tmp)), 0o700);
    assert_eq!(mode(root(&tmp).join(JOB)), 0o700);
    assert_eq!(mode(root(&tmp).join(JOB).join("a.json")), 0o600);
}

#[cfg(unix)]
#[test]
fn planted_link_is_refused() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let tmp = TempDir::new("grant-link");
    let store = store(&tmp);
    let outside = tmp.path("outside");
    fs::create_dir_all(&outside).expect("mkdir");
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o755)).expect("chmod");
    let sentinel = tmp.write("outside/sentinel.txt", "untouched");
    fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o644)).expect("chmod");
    fs::create_dir_all(root(&tmp)).expect("mkdir");
    fs::set_permissions(root(&tmp), fs::Permissions::from_mode(0o700)).expect("chmod");
    symlink(&outside, root(&tmp).join(JOB)).expect("link");

    assert!(store.put(&grant(JOB, "a", NOW + 5, "NL-MARKER-4")).is_err());
    assert!(store.get(JOB, "a", NOW).is_err());
    assert_eq!(listing(&outside), vec!["sentinel.txt".to_string()]);
    assert_eq!(fs::read_to_string(&sentinel).expect("read"), "untouched");
    let mode = |p: &Path| fs::metadata(p).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode(&outside), 0o755);
    assert_eq!(mode(&sentinel), 0o644);

    fs::remove_file(root(&tmp).join(JOB)).expect("unlink");
    fs::create_dir(root(&tmp).join(JOB)).expect("mkdir");
    fs::set_permissions(root(&tmp).join(JOB), fs::Permissions::from_mode(0o700)).expect("chmod");
    let target = tmp.write("outside/target.json", "NL-MARKER-5");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).expect("chmod");
    symlink(&target, root(&tmp).join(JOB).join("a.json")).expect("link");

    assert!(store.put(&grant(JOB, "a", NOW + 5, "NL-MARKER-6")).is_err());
    let err = store.get(JOB, "a", NOW).expect_err("link refused");
    assert!(!err.contains("NL-MARKER"));
    assert!(err.contains("link"));
    assert_eq!(fs::read_to_string(&target).expect("read"), "NL-MARKER-5");
    assert_eq!(mode(&target), 0o644);
    assert!(fs::symlink_metadata(root(&tmp).join(JOB).join("a.json"))
        .expect("meta")
        .file_type()
        .is_symlink());
}

#[cfg(unix)]
#[test]
fn planted_link_at_the_grants_root_is_refused() {
    use std::os::unix::fs::symlink;
    let tmp = TempDir::new("grant-root-link");
    let store = store(&tmp);
    let outside = tmp.path("outside");
    fs::create_dir_all(&outside).expect("mkdir");
    fs::create_dir_all(tmp.path("cluster")).expect("mkdir");
    symlink(&outside, root(&tmp)).expect("link");
    assert!(store.put(&grant(JOB, "a", NOW + 5, "NL-MARKER-7")).is_err());
    assert!(store.get(JOB, "a", NOW).is_err());
    assert!(store.sweep(NOW, &live(&[])).is_err());
    assert!(listing(&outside).is_empty());
}

#[cfg(unix)]
#[test]
fn loose_folders_are_refused_not_tightened() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new("grant-loose");
    let store = store(&tmp);
    store
        .put(&grant(JOB, "a", NOW + 5, "NL-MARKER-8"))
        .expect("put");
    let folder = root(&tmp).join(JOB);
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).expect("chmod");
    assert!(store.put(&grant(JOB, "b", NOW + 5, "NL-MARKER-9")).is_err());
    assert!(store.get(JOB, "a", NOW).is_err());
    assert_eq!(
        fs::metadata(&folder).expect("meta").permissions().mode() & 0o777,
        0o755
    );
    assert!(!folder.join("b.json").exists());
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).expect("chmod");
    fs::set_permissions(root(&tmp), fs::Permissions::from_mode(0o750)).expect("chmod");
    assert!(store.get(JOB, "a", NOW).is_err());
}

#[test]
fn expired_grant_is_deleted_on_get() {
    let tmp = TempDir::new("grant-expired");
    let store = store(&tmp);
    store
        .put(&grant(JOB, "a", NOW, "NL-MARKER-10"))
        .expect("put");
    assert!(store.get(JOB, "a", NOW).expect("get").is_none());
    assert!(!root(&tmp).join(JOB).join("a.json").exists());
    store
        .put(&grant(JOB, "b", NOW + 1, "NL-MARKER-11"))
        .expect("put");
    assert!(store.get(JOB, "b", NOW).expect("get").is_some());
    assert!(store.get(JOB, "b", NOW + 1).expect("get").is_none());
}

#[test]
fn sweep_removes_expired_and_dead_jobs() {
    let tmp = TempDir::new("grant-sweep");
    let store = store(&tmp);
    store
        .put(&grant(JOB, "old", NOW - 1, "NL-MARKER-12"))
        .expect("put");
    store
        .put(&grant(JOB, "fresh", NOW + 100, "NL-MARKER-13"))
        .expect("put");
    store
        .put(&grant(JOB2, "fresh", NOW + 100, "NL-MARKER-14"))
        .expect("put");
    let removed = store.sweep(NOW, &live(&[JOB])).expect("sweep");
    assert_eq!(removed, 2);
    assert!(store.get(JOB, "fresh", NOW).expect("get").is_some());
    assert!(!root(&tmp).join(JOB).join("old.json").exists());
    assert!(!root(&tmp).join(JOB2).exists());
    assert_eq!(store.sweep(NOW, &live(&[JOB])).expect("sweep"), 0);
}

#[test]
fn sweep_without_a_root_is_nothing() {
    let tmp = TempDir::new("grant-sweep-empty");
    assert_eq!(store(&tmp).sweep(NOW, &live(&[])).expect("sweep"), 0);
    assert!(!root(&tmp).exists());
}

#[test]
fn sweep_deletes_malformed_and_stray_files_in_live_jobs() {
    let tmp = TempDir::new("grant-sweep-bad");
    let store = store(&tmp);
    store
        .put(&grant(JOB, "ok", NOW + 100, "NL-MARKER-15"))
        .expect("put");
    let folder = root(&tmp).join(JOB);
    fs::write(folder.join("broken.json"), "{ NL-MARKER-16").expect("write");
    fs::write(folder.join("stray.txt"), "x").expect("write");
    assert_eq!(store.sweep(NOW, &live(&[JOB])).expect("sweep"), 2);
    assert_eq!(listing(&folder), vec!["ok.json".to_string()]);
}

#[cfg(unix)]
#[test]
fn sweep_and_remove_job_unlink_a_planted_link_without_following_it() {
    use std::os::unix::fs::symlink;
    let tmp = TempDir::new("grant-sweep-link");
    let store = store(&tmp);
    let outside = tmp.path("outside");
    tmp.write("outside/keep.txt", "NL-MARKER-17");
    store
        .put(&grant(JOB2, "a", NOW + 5, "NL-MARKER-18"))
        .expect("put");
    symlink(&outside, root(&tmp).join(JOB)).expect("link");
    assert_eq!(store.sweep(NOW, &live(&[JOB2])).expect("sweep"), 1);
    assert!(fs::symlink_metadata(root(&tmp).join(JOB)).is_err());
    assert_eq!(listing(&outside), vec!["keep.txt".to_string()]);

    symlink(&outside, root(&tmp).join(JOB)).expect("link");
    store.remove_job(JOB).expect("remove_job");
    assert!(fs::symlink_metadata(root(&tmp).join(JOB)).is_err());
    assert_eq!(listing(&outside), vec!["keep.txt".to_string()]);
    assert!(store.get(JOB2, "a", NOW).expect("get").is_some());
}

#[test]
fn bad_job_id_builds_no_path() {
    let tmp = TempDir::new("grant-bad-ids");
    let store = store(&tmp);
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
            .put(&grant(job, "a", NOW + 5, "NL-MARKER-19"))
            .is_err());
        assert!(store.get(job, "a", NOW).is_err());
        assert!(store.remove(job, "a").is_err());
        assert!(store.remove_job(job).is_err());
    }
    for name in ["../a", "a/b", "", ".hidden", "1abc", "a b", "a\0b"] {
        assert!(store
            .put(&grant(JOB, name, NOW + 5, "NL-MARKER-20"))
            .is_err());
        assert!(store.get(JOB, name, NOW).is_err());
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
    let file = grant(JOB, "a", NOW, marker);
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
    store
        .put(&grant(JOB, "a", NOW + 5, "NL-MARKER-22"))
        .expect("put");
    let path = root(&tmp).join(JOB).join("a.json");
    let mut old = fs::File::open(&path).expect("open");
    let old_inode = old.metadata().expect("meta").ino();
    store
        .put(&grant(JOB, "a", NOW + 9, "NL-MARKER-23"))
        .expect("put again");
    let new_inode = fs::metadata(&path).expect("meta").ino();
    assert_ne!(old_inode, new_inode);
    let mut old_text = String::new();
    old.read_to_string(&mut old_text).expect("read");
    assert!(old_text.contains("NL-MARKER-22"));
    let got = store.get(JOB, "a", NOW).expect("get").expect("present");
    assert!(got.value.expose() == "NL-MARKER-23");
    assert_eq!(got.expires_ms, NOW + 9);
}

#[test]
fn remove_and_remove_job_delete_and_are_idempotent() {
    let tmp = TempDir::new("grant-remove");
    let store = store(&tmp);
    store.remove(JOB, "a").expect("nothing yet");
    store.remove_job(JOB).expect("nothing yet");
    store
        .put(&grant(JOB, "a", NOW + 5, "NL-MARKER-24"))
        .expect("put");
    store
        .put(&grant(JOB, "b", NOW + 5, "NL-MARKER-25"))
        .expect("put");
    store.remove(JOB, "a").expect("remove");
    store.remove(JOB, "a").expect("again");
    assert!(store.get(JOB, "a", NOW).expect("get").is_none());
    assert!(store.get(JOB, "b", NOW).expect("get").is_some());
    store.remove_job(JOB).expect("remove_job");
    assert!(!root(&tmp).join(JOB).exists());
    store.remove_job(JOB).expect("again");
}

#[test]
fn malformed_file_is_an_error_without_content() {
    let tmp = TempDir::new("grant-malformed");
    let store = store(&tmp);
    store
        .put(&grant(JOB, "a", NOW + 5, "NL-MARKER-26"))
        .expect("put");
    let path = root(&tmp).join(JOB).join("a.json");
    let cases = [
        "{\"v\":1,\"job\":\"j-0123456789ab\",\"name\":\"a\",\"expiresMs\":NL-MARKER-27}".to_string(),
        "{\"v\":2,\"job\":\"j-0123456789ab\",\"name\":\"a\",\"expiresMs\":9999999,\"value\":\"NL-MARKER-28\"}".to_string(),
        "{\"v\":1,\"job\":\"j-0123456789ab\",\"name\":\"a\",\"expiresMs\":\"NL-MARKER-29\",\"value\":\"x\"}".to_string(),
        "{\"v\":1,\"job\":\"j-ba9876543210\",\"name\":\"a\",\"expiresMs\":9999999,\"value\":\"NL-MARKER-30\"}".to_string(),
        "{\"v\":1,\"job\":\"j-0123456789ab\",\"name\":\"b\",\"expiresMs\":9999999,\"value\":\"NL-MARKER-31\"}".to_string(),
        format!("NL-MARKER-32{}", "x".repeat(300_000)),
    ];
    for case in cases {
        fs::remove_file(&path).expect("remove");
        fs::write(&path, case).expect("write");
        let err = store.get(JOB, "a", NOW).expect_err("malformed");
        assert!(!err.contains("NL-MARKER"));
        assert!(err.contains("a.json"));
    }
}

#[cfg(unix)]
#[test]
fn a_special_file_at_the_path_is_refused_without_blocking() {
    let tmp = TempDir::new("grant-fifo");
    let store = store(&tmp);
    store
        .put(&grant(JOB, "keep", NOW + 5, "NL-MARKER-33"))
        .expect("put");
    let fifo = root(&tmp).join(JOB).join("a.json");
    let c = std::ffi::CString::new(fifo.to_str().expect("utf8")).expect("cstr");
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    assert!(store.get(JOB, "a", NOW).is_err());
    assert!(store
        .put(&grant(JOB, "a", NOW + 5, "NL-MARKER-34"))
        .is_err());
    assert_eq!(store.sweep(NOW, &live(&[JOB])).expect("sweep"), 1);
    assert_eq!(
        listing(&root(&tmp).join(JOB)),
        vec!["keep.json".to_string()]
    );
}

#[test]
fn excluded_dirs_cover_secret_folders() {
    assert!(EXCLUDED_DIRS.contains(&"cluster/grants"));
    assert!(EXCLUDED_DIRS.contains(&"cluster/logins"));
    assert!(EXCLUDED_DIRS.contains(&"cluster/incoming"));
}
