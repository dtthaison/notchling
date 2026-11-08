use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "nl-delegate-media-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn age(path: &Path, seconds: u64) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(seconds))
        .unwrap();
}

fn names(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn only_media_written_since_the_run_began_is_found_oldest_first() {
    let dir = scratch("since");
    for name in ["old.png", "cup.jpg", "a.MP4", "plan.md", "noext"] {
        fs::write(dir.join(name), "x").unwrap();
    }
    age(&dir.join("old.png"), 3600);
    age(&dir.join("cup.jpg"), 20);
    let found = media_in(&dir, SystemTime::now() - Duration::from_secs(60));
    assert_eq!(names(&found), ["cup.jpg", "a.MP4"]);
    fs::create_dir(dir.join("sub.png")).unwrap();
    let again = media_in(&dir, SystemTime::now() - Duration::from_secs(60));
    assert_eq!(again.len(), 2, "a folder named like an image is not media");
    assert!(media_in(&dir.join("missing"), SystemTime::UNIX_EPOCH).is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn copying_never_overwrites() {
    let dir = scratch("copy");
    fs::write(dir.join("cup.jpg"), "one").unwrap();
    fs::write(dir.join("plain"), "two").unwrap();
    let out = dir.join("out");
    let files = vec![dir.join("cup.jpg"), dir.join("plain")];
    let first = copy_to(&files, &out).unwrap();
    let second = copy_to(&files, &out).unwrap();
    let third = copy_to(&files[..1], &out).unwrap();
    assert_eq!(names(&first), ["cup.jpg", "plain"]);
    assert_eq!(names(&second), ["cup-2.jpg", "plain-2"]);
    assert_eq!(names(&third), ["cup-3.jpg"]);
    assert_eq!(fs::read_to_string(out.join("cup-2.jpg")).unwrap(), "one");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_session_id_with_a_separator_or_dots_finds_nothing() {
    let root = scratch("ids");
    let secret = root.join("secret");
    fs::create_dir_all(&secret).unwrap();
    fs::write(secret.join("loot.png"), "x").unwrap();
    let inside = root.join("images");
    fs::create_dir_all(inside.join("s1")).unwrap();
    fs::write(inside.join("s1").join("ok.png"), "x").unwrap();
    let since = SystemTime::now() - Duration::from_secs(60);
    assert_eq!(names(&session_media(&inside, "s1", since)), ["ok.png"]);
    for bad in ["../secret", "..", ".", "", "a/b", "s1/..", "a\\b", "x\0y"] {
        assert!(!safe_component(bad), "{bad:?}");
        assert!(session_media(&inside, bad, since).is_empty(), "{bad:?}");
        assert_eq!(session_folder(&inside, bad), None);
    }
    assert!(safe_component("d67413c2-a0f3-4381-8a8e-e98b541d6d76"));
    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn a_symlink_is_never_followed_out_of_the_folder() {
    use std::os::unix::fs::symlink;
    let root = scratch("links");
    let outside = root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("private.png"), "secret").unwrap();
    let images = root.join("images");
    fs::create_dir_all(images.join("s1")).unwrap();
    // A file that points out, and a session folder that points out.
    symlink(
        outside.join("private.png"),
        images.join("s1").join("link.png"),
    )
    .unwrap();
    fs::write(images.join("s1").join("real.png"), "x").unwrap();
    symlink(&outside, images.join("s2")).unwrap();
    let since = SystemTime::now() - Duration::from_secs(60);
    assert_eq!(names(&session_media(&images, "s1", since)), ["real.png"]);
    assert!(
        session_media(&images, "s2", since).is_empty(),
        "a folder that is a symlink"
    );

    // Copying refuses a link even when handed one.
    let out = root.join("out");
    let err = copy_to(&[images.join("s1").join("link.png")], &out).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(
        !out.join("link.png").exists(),
        "nothing was written for the link"
    );
    let _ = fs::remove_dir_all(&root);
}
