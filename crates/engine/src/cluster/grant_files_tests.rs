use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::grant_files::{GrantFile, GrantStore, GrantValue, EXCLUDED_DIRS};
use crate::sessions::testutil::TempDir;

const JOB: &str = "j-0123456789ab";
const JOB2: &str = "j-ba9876543210";
const MINUTE: i64 = 60_000;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

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

#[cfg(unix)]
fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("chmod");
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
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

#[cfg(unix)]
#[test]
fn planted_link_is_refused() {
    use std::os::unix::fs::symlink;
    let tmp = TempDir::new("grant-link");
    let store = store(&tmp);
    let n = now();
    let outside = tmp.path("outside");
    fs::create_dir_all(&outside).expect("mkdir");
    chmod(&outside, 0o700);
    let sentinel = tmp.write("outside/sentinel.txt", "untouched");
    chmod(&sentinel, 0o600);
    fs::create_dir_all(root(&tmp)).expect("mkdir");
    chmod(&root(&tmp), 0o700);
    symlink(&outside, root(&tmp).join(JOB)).expect("link");

    let err = store
        .put(&grant(JOB, "a", n + MINUTE, "NL-MARKER-4"))
        .expect_err("link refused");
    assert!(err.contains("link"));
    let err = store.get(JOB, "a", n).expect_err("link refused");
    assert!(err.contains("link"));
    assert_eq!(listing(&outside), vec!["sentinel.txt".to_string()]);
    assert_eq!(fs::read_to_string(&sentinel).expect("read"), "untouched");
    assert_eq!(mode_of(&outside), 0o700);
    assert_eq!(mode_of(&sentinel), 0o600);

    fs::remove_file(root(&tmp).join(JOB)).expect("unlink");
    fs::create_dir(root(&tmp).join(JOB)).expect("mkdir");
    chmod(&root(&tmp).join(JOB), 0o700);
    let target = tmp.write("outside/target.json", "NL-MARKER-5");
    chmod(&target, 0o644);
    symlink(&target, root(&tmp).join(JOB).join("a.json")).expect("link");

    assert!(store
        .put(&grant(JOB, "a", n + MINUTE, "NL-MARKER-6"))
        .is_err());
    let err = store.get(JOB, "a", n).expect_err("link refused");
    assert!(!err.contains("NL-MARKER"));
    assert!(err.contains("link"));
    assert_eq!(fs::read_to_string(&target).expect("read"), "NL-MARKER-5");
    assert_eq!(mode_of(&target), 0o644);
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
    let n = now();
    let outside = tmp.path("outside");
    fs::create_dir_all(&outside).expect("mkdir");
    chmod(&outside, 0o700);
    fs::create_dir_all(tmp.path("cluster")).expect("mkdir");
    symlink(&outside, root(&tmp)).expect("link");
    let err = store
        .put(&grant(JOB, "a", n + MINUTE, "NL-MARKER-7"))
        .expect_err("refused");
    assert!(err.contains("link"));
    assert!(store.get(JOB, "a", n).is_err());
    assert!(store.sweep(n, &live(&[])).is_err());
    assert!(listing(&outside).is_empty());
}

#[cfg(unix)]
#[test]
fn a_hard_link_at_the_file_path_is_not_read() {
    let tmp = TempDir::new("grant-hardlink");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "keep", n + MINUTE, "NL-MARKER-39"))
        .expect("put");
    let outside = tmp.path("outside.json");
    let planted = grant(JOB, "a", n + MINUTE, "NL-MARKER-40");
    fs::write(&outside, serde_json::to_string(&planted).expect("json")).expect("write");
    chmod(&outside, 0o600);
    fs::hard_link(&outside, root(&tmp).join(JOB).join("a.json")).expect("hard link");
    let err = store.get(JOB, "a", n).expect_err("refused");
    assert!(!err.contains("NL-MARKER"));
    assert!(err.contains("hard link"));
    assert!(store
        .get(JOB, "keep", n)
        .expect("get")
        .is_some_and(|g| g.value.expose() == "NL-MARKER-39"));
}

#[cfg(unix)]
#[test]
fn a_file_open_to_others_is_not_read_and_put_replaces_it() {
    let tmp = TempDir::new("grant-file-mode");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "a", n + MINUTE, "NL-MARKER-41"))
        .expect("put");
    let path = root(&tmp).join(JOB).join("a.json");
    chmod(&path, 0o644);
    let err = store.get(JOB, "a", n).expect_err("refused");
    assert!(!err.contains("NL-MARKER"));
    store
        .put(&grant(JOB, "a", n + MINUTE, "NL-MARKER-42"))
        .expect("replace");
    assert_eq!(mode_of(&path), 0o600);
    assert!(store
        .get(JOB, "a", n)
        .expect("get")
        .is_some_and(|g| g.value.expose() == "NL-MARKER-42"));
}

#[cfg(unix)]
#[test]
fn loose_folders_refuse_put_and_get_but_not_deletion() {
    let tmp = TempDir::new("grant-loose");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "a", n + MINUTE, "NL-MARKER-8"))
        .expect("put");
    let folder = root(&tmp).join(JOB);
    chmod(&folder, 0o750);
    assert!(store
        .put(&grant(JOB, "b", n + MINUTE, "NL-MARKER-9"))
        .is_err());
    let err = store.get(JOB, "a", n).expect_err("loose");
    assert!(!err.contains("NL-MARKER"));
    assert_eq!(mode_of(&folder), 0o750);
    assert!(!folder.join("b.json").exists());
    chmod(&folder, 0o700);
    chmod(&root(&tmp), 0o750);
    assert!(store.get(JOB, "a", n).is_err());
    assert!(store
        .put(&grant(JOB, "b", n + MINUTE, "NL-MARKER-43"))
        .is_err());
    assert_eq!(mode_of(&root(&tmp)), 0o750);
}

#[cfg(unix)]
#[test]
fn deletion_works_in_loose_folders() {
    let tmp = TempDir::new("grant-loose-delete");
    let store = store(&tmp);
    let n = now();
    let later = n + 2 * MINUTE;
    let loosen = || {
        chmod(&root(&tmp), 0o750);
        for job in [JOB, JOB2] {
            let folder = root(&tmp).join(job);
            if folder.exists() {
                chmod(&folder, 0o750);
            }
        }
    };
    store
        .put(&grant(JOB, "old", n + MINUTE, "NL-MARKER-44"))
        .expect("put");
    store
        .put(&grant(JOB, "gone", n + 5 * MINUTE, "NL-MARKER-45"))
        .expect("put");
    store
        .put(&grant(JOB2, "x", n + 5 * MINUTE, "NL-MARKER-46"))
        .expect("put");
    loosen();
    assert_eq!(store.sweep(later, &live(&[JOB])).expect("sweep"), 2);
    assert!(!root(&tmp).join(JOB).join("old.json").exists());
    assert!(!root(&tmp).join(JOB2).exists());
    assert!(root(&tmp).join(JOB).join("gone.json").exists());
    store.remove(JOB, "gone").expect("remove");
    assert!(!root(&tmp).join(JOB).join("gone.json").exists());

    chmod(&root(&tmp), 0o700);
    chmod(&root(&tmp).join(JOB), 0o700);
    store
        .put(&grant(JOB2, "y", n + 5 * MINUTE, "NL-MARKER-47"))
        .expect("put");
    loosen();
    store.remove_job(JOB2).expect("remove_job");
    assert!(!root(&tmp).join(JOB2).exists());

    chmod(&root(&tmp), 0o700);
    chmod(&root(&tmp).join(JOB), 0o700);
    store
        .put(&grant(JOB, "z", n + MINUTE, "NL-MARKER-48"))
        .expect("put");
    loosen();
    assert!(store.get(JOB, "z", later).expect("get").is_none());
    assert!(!root(&tmp).join(JOB).join("z.json").exists());
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

#[cfg(unix)]
#[test]
fn sweep_and_remove_job_unlink_a_planted_link_without_following_it() {
    use std::os::unix::fs::symlink;
    let tmp = TempDir::new("grant-sweep-link");
    let store = store(&tmp);
    let n = now();
    let outside = tmp.path("outside");
    tmp.write("outside/keep.txt", "NL-MARKER-17");
    store
        .put(&grant(JOB2, "a", n + MINUTE, "NL-MARKER-18"))
        .expect("put");
    symlink(&outside, root(&tmp).join(JOB)).expect("link");
    assert_eq!(store.sweep(n, &live(&[JOB2])).expect("sweep"), 1);
    assert!(fs::symlink_metadata(root(&tmp).join(JOB)).is_err());
    assert_eq!(listing(&outside), vec!["keep.txt".to_string()]);

    symlink(&outside, root(&tmp).join(JOB)).expect("link");
    store.remove_job(JOB).expect("remove_job");
    assert!(fs::symlink_metadata(root(&tmp).join(JOB)).is_err());
    assert_eq!(listing(&outside), vec!["keep.txt".to_string()]);
    assert!(store.get(JOB2, "a", n).expect("get").is_some());
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

#[cfg(unix)]
#[test]
fn a_special_file_at_the_path_is_refused_without_blocking() {
    let tmp = TempDir::new("grant-fifo");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "keep", n + MINUTE, "NL-MARKER-33"))
        .expect("put");
    let fifo = root(&tmp).join(JOB).join("a.json");
    let c = std::ffi::CString::new(fifo.to_str().expect("utf8")).expect("cstr");
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    assert!(store.get(JOB, "a", n).is_err());
    assert!(store
        .put(&grant(JOB, "a", n + MINUTE, "NL-MARKER-34"))
        .is_err());
    assert_eq!(store.sweep(n, &live(&[JOB])).expect("sweep"), 1);
    assert_eq!(
        listing(&root(&tmp).join(JOB)),
        vec!["keep.json".to_string()]
    );
}

#[cfg(unix)]
#[test]
fn concurrent_puts_gets_and_sweeps_never_lose_a_fresh_grant() {
    use std::sync::Arc;
    let tmp = TempDir::new("grant-concurrent");
    let store = Arc::new(store(&tmp));
    let n = now();
    let mut handles = Vec::new();
    for t in 0..4 {
        let store = Arc::clone(&store);
        handles.push(std::thread::spawn(move || {
            for i in 0..25 {
                let name = format!("s{t}_{i}");
                store
                    .put(&grant(JOB, &name, n + 10 * MINUTE, "NL-MARKER-49"))
                    .expect("put");
                store.sweep(n, &live(&[JOB])).expect("sweep");
                assert!(store.get(JOB, &name, n).expect("get").is_some());
            }
        }));
    }
    for handle in handles {
        handle.join().expect("thread");
    }
    assert_eq!(listing(&root(&tmp).join(JOB)).len(), 100);
}

#[test]
fn excluded_dirs_cover_secret_folders() {
    assert!(EXCLUDED_DIRS.contains(&"cluster/grants"));
    assert!(EXCLUDED_DIRS.contains(&"cluster/logins"));
    assert!(EXCLUDED_DIRS.contains(&"cluster/incoming"));
}
