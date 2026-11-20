use crate::outbox_testkit::{rig_with, Options, IDLE, T0};
use crate::reply_screen::{keep, screen_name, stamp, KEPT_SCREENS, SCREENS_DIR};
use notchling_engine::reply::exec::Failure;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-reply-screens-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.join(SCREENS_DIR))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn head() -> Vec<String> {
    vec!["job: ab12cd34".to_string()]
}

#[test]
fn a_time_is_written_as_a_sortable_utc_stamp() {
    assert_eq!(stamp(0), "19700101T000000.000Z");
    assert_eq!(stamp(1_795_149_934_289), "20261120T044534.289Z");
    assert_eq!(stamp(951_868_799_999), "20000229T235959.999Z");
}

#[test]
fn a_screen_is_named_by_time_session_and_why() {
    assert_eq!(
        screen_name(
            1_795_149_934_289,
            "102ba791-6682-4cb4-a0f3-1b151ded51f9",
            &Failure::NotCleared
        ),
        "20261120T044534.289Z-102ba791-not_cleared.txt"
    );
    assert_eq!(
        screen_name(0, "../etc/passwd", &Failure::NoInput),
        "19700101T000000.000Z----etc-p-no_input.txt"
    );
    assert_eq!(
        screen_name(0, "ab12cd34", &Failure::Mixed),
        "19700101T000000.000Z-ab12cd34-mixed.txt"
    );
}

#[test]
fn every_failure_leaves_its_own_file_and_the_single_file() {
    let dir = scratch("own");
    let file = dir.join("reply-screen.txt");
    let screen = vec!["❯ ····".to_string()];
    keep(
        &file,
        "ab12cd34",
        &Failure::NotCleared,
        &head(),
        &screen,
        1_000,
    );
    keep(
        &file,
        "ab12cd34",
        &Failure::NoInput,
        &head(),
        &screen,
        2_000,
    );
    assert_eq!(
        names(&dir),
        [
            "19700101T000001.000Z-ab12cd34-not_cleared.txt",
            "19700101T000002.000Z-ab12cd34-no_input.txt",
        ]
    );
    let single = std::fs::read_to_string(&file).unwrap();
    let own = std::fs::read_to_string(
        dir.join(SCREENS_DIR)
            .join("19700101T000002.000Z-ab12cd34-no_input.txt"),
    )
    .unwrap();
    assert_eq!(single, own, "the single file is the latest failure");
    assert!(
        own.contains("job: ab12cd34") && own.contains("❯ ····"),
        "{own}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir.join(SCREENS_DIR)), 0o700);
        let first = dir
            .join(SCREENS_DIR)
            .join("19700101T000001.000Z-ab12cd34-not_cleared.txt");
        assert_eq!(mode(&first), 0o600);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn only_the_last_twenty_screens_are_kept() {
    let dir = scratch("twenty");
    let file = dir.join("reply-screen.txt");
    let screen = vec!["❯".to_string()];
    for n in 0..KEPT_SCREENS + 7 {
        let at = (n as i64 + 1) * 1_000;
        keep(
            &file,
            "ab12cd34",
            &Failure::NotCleared,
            &head(),
            &screen,
            at,
        );
    }
    let kept = names(&dir);
    assert_eq!(kept.len(), KEPT_SCREENS);
    assert_eq!(
        kept.first().map(String::as_str),
        Some("19700101T000008.000Z-ab12cd34-not_cleared.txt"),
        "the oldest seven are gone"
    );
    assert_eq!(
        kept.last().map(String::as_str),
        Some("19700101T000027.000Z-ab12cd34-not_cleared.txt")
    );
    assert!(file.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_reply_through_the_outbox_leaves_a_masked_file_of_its_own() {
    let dir = scratch("outbox");
    let file = dir.join("reply-screen.txt");
    let r = rig_with(Options {
        screen: crate::acting_ops_tests::PROMPT,
        reply_screen: Some(file.clone()),
        ..Options::default()
    });
    r.deaf.store(true, Ordering::SeqCst);
    r.send("job-idle", "tell nobody");
    r.postmen.postman(IDLE).pass(r.at(T0));
    let kept = names(&dir);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(kept[0].ends_with("-idle0001-not_sent.txt"), "{kept:?}");
    let own = std::fs::read_to_string(dir.join(SCREENS_DIR).join(&kept[0])).unwrap();
    assert_eq!(own, std::fs::read_to_string(&file).unwrap());
    assert!(
        own.contains("❯ ···· ······"),
        "the box keeps its shape: {own}"
    );
    assert!(!own.contains("tell") && !own.contains("nobody"), "{own}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_reply_that_goes_in_leaves_no_screen() {
    let dir = scratch("sent");
    let file = dir.join("reply-screen.txt");
    let r = rig_with(Options {
        reply_screen: Some(file.clone()),
        ..Options::default()
    });
    r.send("job-idle", "hi");
    r.postmen.postman(IDLE).pass(r.at(T0));
    assert_eq!(names(&dir), Vec::<String>::new());
    assert!(!file.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
