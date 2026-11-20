use crate::outbox_testkit::{rig_with, Options, IDLE, T0};
use crate::reply_screen::{excerpt, keep, screen_name, shape, stamp, KEPT_SCREENS, SCREENS_DIR};
use notchling_engine::reply::exec::Failure;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

const RULE: &str = "────────────────────────────────────────";
const TOKEN: &str = "sk-planted-1234";

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
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn own(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(SCREENS_DIR).join(name)).unwrap()
}

fn head() -> Vec<String> {
    vec!["job: ab12cd34".to_string()]
}

fn body(kept: &str) -> Vec<&str> {
    let at = kept
        .lines()
        .position(|line| line.starts_with("----- screen"));
    kept.lines().skip(at.map_or(0, |n| n + 1)).collect()
}

fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|row| row.to_string()).collect()
}

fn with_secrets() -> Vec<String> {
    lines(&[
        &format!("⏺ the old key is {TOKEN}"),
        "  more old transcript text",
        "",
        "  and a line the person typed: hunter2",
        "",
        "✻ Brewed for 1m 34s · done 11:34 AM",
        "",
        &format!("{RULE}── user-segment-query"),
        "❯ my own draft",
        RULE,
        "  someone@example.invalid",
        "  ⏵⏵ auto mode on (shift+tab to cycle)",
        "  third line under the box",
        "  fourth line under the box",
    ])
}

fn noted() -> Vec<String> {
    lines(&[
        "job: ab12cd34",
        "refused: The session's input box holds other text, so the message wasn't sent.",
        "message: 64 characters",
        "cursor: row 8, column 2",
        "----- what each step saw (3 notes) -----",
        &format!("+1643ms await input: starting: Waking session {TOKEN} · Ctrl+Z to detach"),
        &format!("+7087ms await input: no input box, cursor hidden, last row: {TOKEN} here"),
        "+9000ms await input: input box, 1 row, holding text, cursor at 8,2 in the box, under it: someone@example.invalid",
    ])
}

fn letters_in(rows: &[&str]) -> bool {
    rows.iter()
        .any(|row| row.chars().any(char::is_alphanumeric))
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
fn a_line_is_kept_as_its_shape_only() {
    assert_eq!(shape("  Hello, wörld 42 ·"), "  ······ ····· ·· ·");
    assert_eq!(shape("──── name"), "──── ····");
    assert_eq!(shape(""), "");
}

#[test]
fn the_single_file_is_the_full_masked_screen_as_before() {
    let dir = scratch("single");
    let file = dir.join("reply-screen.txt");
    keep(
        &file,
        "ab12cd34",
        &Failure::NotCleared,
        &noted(),
        &with_secrets(),
        1_000,
    );
    let single = std::fs::read_to_string(&file).unwrap();
    assert!(
        single.contains(&format!("the old key is {TOKEN}")),
        "{single}"
    );
    assert!(
        single.contains("last row: sk-planted-1234 here"),
        "{single}"
    );
    assert!(
        single.contains("❯ ·· ··· ·····"),
        "the box is masked: {single}"
    );
    assert!(!single.contains("draft"), "{single}");
    assert!(single.contains("----- screen (14 lines) -----"), "{single}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failures_own_file_holds_the_box_its_rules_and_a_few_rows_around_as_shapes() {
    let dir = scratch("own-box");
    let file = dir.join("reply-screen.txt");
    keep(
        &file,
        "ab12cd34",
        &Failure::NotCleared,
        &noted(),
        &with_secrets(),
        1_000,
    );
    let kept = names(&dir);
    assert_eq!(kept, ["19700101T000001.000Z-ab12cd34-not_cleared.txt"]);
    let text = own(&dir, &kept[0]);
    for secret in [
        TOKEN,
        "hunter2",
        "draft",
        "someone@",
        "transcript",
        "Brewed",
    ] {
        assert!(!text.contains(secret), "{secret} leaked: {text}");
    }
    let rows = body(&text);
    assert_eq!(rows.len(), 3 + 1 + 1 + 1 + 3, "{text}");
    assert!(!letters_in(&rows), "only shapes outside the box: {text}");
    assert_eq!(rows[3], shape(&format!("{RULE}── user-segment-query")));
    assert_eq!(
        rows[4], "❯ ·· ··· ·····",
        "the box keeps its shape and mark"
    );
    assert_eq!(rows[5], RULE);
    assert!(text.contains("job: ab12cd34"), "{text}");
    assert!(text.contains("cursor: row 8, column 2"), "{text}");
    assert!(
        text.contains("refused: The session's input box holds other text"),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_notes_of_what_each_step_saw_keep_their_words_and_lose_the_screen_text() {
    let dir = scratch("own-notes");
    let file = dir.join("reply-screen.txt");
    keep(
        &file,
        "ab12cd34",
        &Failure::NoInput,
        &noted(),
        &with_secrets(),
        2_000,
    );
    let text = own(&dir, &names(&dir)[0]);
    assert!(
        !text.contains(TOKEN) && !text.contains("someone@"),
        "{text}"
    );
    let waking = shape(&format!("Waking session {TOKEN} · Ctrl+Z to detach"));
    assert!(
        text.contains(&format!("+1643ms await input: starting: {waking}")),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "no input box, cursor hidden, last row: {}",
            shape(&format!("{TOKEN} here"))
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "holding text, cursor at 8,2 in the box, under it: {}",
            shape("someone@example.invalid")
        )),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_screen_with_no_box_keeps_the_shapes_of_its_last_rows_and_never_the_words() {
    let dir = scratch("own-nobox");
    let file = dir.join("reply-screen.txt");
    let rows: Vec<String> = (0..30)
        .map(|n| format!("  transcript row {n} with {TOKEN}"))
        .chain(["".to_string(), "".to_string()])
        .collect();
    keep(&file, "ab12cd34", &Failure::NoInput, &head(), &rows, 3_000);
    let text = own(&dir, &names(&dir)[0]);
    let kept = body(&text);
    assert_eq!(kept.len(), 20, "{text}");
    assert!(!letters_in(&kept), "{text}");
    assert!(
        !text.contains(TOKEN) && !text.contains("transcript"),
        "{text}"
    );
    assert_eq!(kept[19], shape("  transcript row 29 with sk-planted-1234"));
    let single = std::fs::read_to_string(&file).unwrap();
    assert!(
        single.contains("transcript row 0 with"),
        "as before: {single}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_screen_keeps_no_rows() {
    assert_eq!(excerpt(&[]), Vec::<String>::new());
    assert_eq!(excerpt(&lines(&["", ""])), Vec::<String>::new());
}

#[test]
fn every_failure_leaves_its_own_file_and_the_single_file() {
    let dir = scratch("own");
    let file = dir.join("reply-screen.txt");
    let screen = lines(&["❯ ····"]);
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
    assert!(single.contains("❯ ····"), "{single}");
    let latest = own(&dir, "19700101T000002.000Z-ab12cd34-no_input.txt");
    assert!(latest.contains("job: ab12cd34"), "{latest}");
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
fn two_failures_in_the_same_millisecond_keep_two_files() {
    let dir = scratch("same");
    let file = dir.join("reply-screen.txt");
    let screen = lines(&["❯"]);
    for chars in [11, 22, 33] {
        let head = vec![format!("message: {chars} characters")];
        keep(
            &file,
            "ab12cd34",
            &Failure::NotCleared,
            &head,
            &screen,
            5_000,
        );
    }
    let kept = names(&dir);
    assert_eq!(
        kept,
        [
            "19700101T000005.000Z-ab12cd34-not_cleared-1.txt",
            "19700101T000005.000Z-ab12cd34-not_cleared-2.txt",
            "19700101T000005.000Z-ab12cd34-not_cleared.txt",
        ]
    );
    for (name, chars) in [
        ("19700101T000005.000Z-ab12cd34-not_cleared.txt", 11),
        ("19700101T000005.000Z-ab12cd34-not_cleared-1.txt", 22),
        ("19700101T000005.000Z-ab12cd34-not_cleared-2.txt", 33),
    ] {
        assert!(own(&dir, name).contains(&format!("message: {chars} characters")));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn only_the_last_twenty_screens_are_kept() {
    let dir = scratch("twenty");
    let file = dir.join("reply-screen.txt");
    let screen = lines(&["❯"]);
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
fn what_is_not_a_screen_file_is_neither_counted_nor_removed() {
    let dir = scratch("strays");
    let file = dir.join("reply-screen.txt");
    let folder = dir.join(SCREENS_DIR);
    std::fs::create_dir_all(folder.join("a-folder.txt")).unwrap();
    std::fs::write(folder.join("notes.log"), "mine").unwrap();
    let screen = lines(&["❯"]);
    for n in 0..KEPT_SCREENS + 5 {
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
    let screens = names(&dir)
        .into_iter()
        .filter(|name| name.ends_with(".txt"))
        .count();
    assert_eq!(screens, KEPT_SCREENS);
    assert!(folder.join("a-folder.txt").is_dir());
    assert_eq!(
        std::fs::read_to_string(folder.join("notes.log")).unwrap(),
        "mine"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_folder_that_is_a_link_is_not_written_through() {
    let dir = scratch("link");
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, dir.join(SCREENS_DIR)).unwrap();
    let file = dir.join("reply-screen.txt");
    keep(
        &file,
        "ab12cd34",
        &Failure::NotCleared,
        &head(),
        &lines(&["❯"]),
        1_000,
    );
    assert!(file.exists(), "the single file is written as before");
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_folder_that_was_looser_is_made_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("loose");
    let folder = dir.join(SCREENS_DIR);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    let file = dir.join("reply-screen.txt");
    keep(
        &file,
        "ab12cd34",
        &Failure::NotCleared,
        &head(),
        &lines(&["❯"]),
        1_000,
    );
    let mode = std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn failures_at_the_same_moment_from_many_threads_each_keep_a_file_and_the_single_file_is_one_of_them(
) {
    let dir = scratch("threads");
    let file = dir.join("reply-screen.txt");
    let screen = lines(&["❯"]);
    std::thread::scope(|scope| {
        for n in 0..8 {
            let (file, screen) = (&file, &screen);
            scope.spawn(move || {
                let head = vec![format!("message: {n} characters")];
                keep(file, "ab12cd34", &Failure::NotCleared, &head, screen, 7_000);
            });
        }
    });
    let kept = names(&dir);
    assert_eq!(kept.len(), 8, "{kept:?}");
    let mut seen: Vec<String> = kept
        .iter()
        .map(|name| own(&dir, name).lines().next().unwrap().to_string())
        .collect();
    seen.sort();
    let want: Vec<String> = (0..8).map(|n| format!("message: {n} characters")).collect();
    assert_eq!(seen, want);
    let single = std::fs::read_to_string(&file).unwrap();
    assert!(want.contains(&single.lines().next().unwrap().to_string()));
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
    let text = own(&dir, &kept[0]);
    assert!(text.contains("job: idle0001"), "{text}");
    assert!(
        text.contains("❯ ···· ······"),
        "the box keeps its shape: {text}"
    );
    assert!(!text.contains("tell") && !text.contains("nobody"), "{text}");
    assert!(
        !text.contains("shortcuts"),
        "the rows under the box are shapes: {text}"
    );
    assert!(file.exists());
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
