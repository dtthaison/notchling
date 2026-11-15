use std::fs;

#[cfg(unix)]
use super::grant_files_tests::{chmod, mode_of};
use super::grant_files_tests::{grant, listing, live, now, root, store, JOB, JOB2, MINUTE};
use crate::sessions::testutil::TempDir;

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
    assert!(fs::read_to_string(&target).expect("read") == "NL-MARKER-5");
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

#[cfg(unix)]
#[test]
fn put_creates_nothing_in_a_loose_root() {
    let tmp = TempDir::new("grant-loose-root-put");
    let store = store(&tmp);
    fs::create_dir_all(root(&tmp)).expect("mkdir");
    chmod(&root(&tmp), 0o750);
    assert!(store
        .put(&grant(JOB, "a", now() + MINUTE, "NL-MARKER-50"))
        .is_err());
    assert!(listing(&root(&tmp)).is_empty());
    assert_eq!(mode_of(&root(&tmp)), 0o750);
}

#[cfg(unix)]
#[test]
fn sweep_does_not_keep_what_get_would_refuse() {
    let tmp = TempDir::new("grant-sweep-nonprivate");
    let store = store(&tmp);
    let n = now();
    for name in ["good", "open", "linked"] {
        store
            .put(&grant(JOB, name, n + 10 * MINUTE, "NL-MARKER-51"))
            .expect("put");
    }
    let folder = root(&tmp).join(JOB);
    chmod(&folder.join("open.json"), 0o644);
    let copy = tmp.path("copy.json");
    fs::hard_link(folder.join("linked.json"), &copy).expect("hard link");
    assert_eq!(store.sweep(n, &live(&[JOB])).expect("sweep"), 2);
    assert_eq!(listing(&folder), vec!["good.json".to_string()]);
    assert!(copy.exists());
}

#[cfg(unix)]
#[test]
fn a_link_or_file_at_a_live_jobs_folder_is_cleared_not_fatal() {
    use std::os::unix::fs::symlink;
    let tmp = TempDir::new("grant-sweep-live-link");
    let store = store(&tmp);
    let n = now();
    let outside = tmp.path("outside");
    tmp.write("outside/keep.txt", "NL-MARKER-52");
    store
        .put(&grant(JOB2, "a", n + MINUTE, "NL-MARKER-53"))
        .expect("put");
    symlink(&outside, root(&tmp).join(JOB)).expect("link");
    assert_eq!(store.sweep(n, &live(&[JOB, JOB2])).expect("sweep"), 1);
    assert!(fs::symlink_metadata(root(&tmp).join(JOB)).is_err());
    assert_eq!(listing(&outside), vec!["keep.txt".to_string()]);
    assert!(store.get(JOB2, "a", n).expect("get").is_some());

    fs::write(root(&tmp).join(JOB), "x").expect("write");
    assert_eq!(store.sweep(n, &live(&[JOB, JOB2])).expect("sweep"), 1);
    assert!(!root(&tmp).join(JOB).exists());
    assert_eq!(store.sweep(n, &live(&[JOB, JOB2])).expect("sweep"), 0);
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

#[cfg(unix)]
fn unlock(path: &std::path::Path) {
    if path.exists() {
        chmod(path, 0o700);
    }
}

#[cfg(unix)]
#[test]
fn deletion_keeps_going_past_a_locked_subfolder() {
    let tmp = TempDir::new("grant-locked-sub");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "keep", n + 10 * MINUTE, "NL-MARKER-55"))
        .expect("put");
    let folder = root(&tmp).join(JOB);
    let sub = folder.join("sub");
    fs::create_dir(&sub).expect("mkdir");
    fs::write(sub.join("inner.txt"), "x").expect("write");
    fs::write(folder.join("n.json"), "{").expect("write");
    fs::write(folder.join("o.json"), "{").expect("write");
    chmod(&sub, 0o000);
    let swept = store.sweep(n, &live(&[JOB]));
    unlock(&sub);
    assert_eq!(swept.expect("sweep"), 3);
    assert_eq!(listing(&folder), vec!["keep.json".to_string()]);

    fs::create_dir(&sub).expect("mkdir");
    fs::write(sub.join("inner.txt"), "x").expect("write");
    chmod(&sub, 0o000);
    let removed = store.remove_job(JOB);
    unlock(&sub);
    removed.expect("remove_job");
    assert!(!folder.exists());
}

#[cfg(unix)]
#[test]
fn a_read_only_subfolder_is_emptied_and_removed() {
    let tmp = TempDir::new("grant-readonly-sub");
    let store = store(&tmp);
    let n = now();
    store
        .put(&grant(JOB, "keep", n + 10 * MINUTE, "NL-MARKER-56"))
        .expect("put");
    let folder = root(&tmp).join(JOB);
    let sub = folder.join("sub");
    fs::create_dir(&sub).expect("mkdir");
    fs::write(sub.join("inner.txt"), "x").expect("write");
    chmod(&sub, 0o500);
    let swept = store.sweep(n, &live(&[JOB]));
    unlock(&sub);
    assert_eq!(swept.expect("sweep"), 1);
    assert_eq!(listing(&folder), vec!["keep.json".to_string()]);
}

#[cfg(unix)]
#[test]
fn a_mode_000_job_folder_is_deleted_or_swept() {
    let tmp = TempDir::new("grant-locked-job");
    let store = store(&tmp);
    let n = now();
    let folder = root(&tmp).join(JOB);
    store
        .put(&grant(JOB, "a", n + MINUTE, "NL-MARKER-57"))
        .expect("put");
    chmod(&folder, 0o000);
    let removed = store.remove_job(JOB);
    unlock(&folder);
    removed.expect("remove_job");
    assert!(!folder.exists());

    let other = root(&tmp).join(JOB2);
    store
        .put(&grant(JOB2, "a", n + MINUTE, "NL-MARKER-58"))
        .expect("put");
    chmod(&other, 0o000);
    let swept = store.sweep(n, &live(&[]));
    unlock(&other);
    assert_eq!(swept.expect("sweep"), 1);
    assert!(!other.exists());

    store
        .put(&grant(JOB, "b", n + MINUTE, "NL-MARKER-59"))
        .expect("put");
    chmod(&folder, 0o000);
    let swept = store.sweep(n + 2 * MINUTE, &live(&[JOB]));
    unlock(&folder);
    assert_eq!(swept.expect("sweep"), 1);
    assert!(listing(&folder).is_empty());
}
