use super::*;
use crate::sessions::testutil::TempDir;
use std::io::Write;
use std::path::PathBuf;

fn tick() {
    std::thread::sleep(Duration::from_millis(30));
}

fn said(id: &str) -> String {
    format!(r#"{{"type":"user","sessionId":"{id}","message":{{"role":"user","content":"hi"}}}}"#)
        + "\n"
}

fn continued(id: &str, to: &str) -> String {
    format!(
        r#"{{"type":"continued-in","timestamp":"2026-11-13T07:32:53.533Z","sessionId":"{id}","continuedInSessionId":"{to}"}}"#
    ) + "\n"
}

fn append(path: &Path, text: &str) {
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open");
    file.write_all(text.as_bytes()).expect("append");
}

fn many(dir: &TempDir, count: usize) {
    for n in 0..count {
        let id = format!("s{n:04}");
        dir.write(&format!("p/{id}.jsonl"), &said(&id));
    }
}

fn path_of(dir: &TempDir, name: &str) -> String {
    dir.path(&format!("p/{name}.jsonl"))
        .to_string_lossy()
        .into_owned()
}

#[test]
fn many_files_are_read_once_and_an_unchanged_folder_is_not_scanned_again() {
    let dir = TempDir::new("continued-many");
    many(&dir, 200);
    dir.write("p/a.jsonl", &(said("a") + &continued("a", "b")));
    dir.write("p/b.jsonl", &said("b"));
    let folder = dir.path("p");
    let b = path_of(&dir, "b");

    assert_eq!(earlier(&b), [path_of(&dir, "a")]);
    let first = tail_reads(&folder);
    assert_eq!(first, 202, "every transcript's tail, once");

    for _ in 0..5 {
        assert_eq!(earlier(&b), [path_of(&dir, "a")]);
    }
    assert_eq!(tail_reads(&folder), first, "nothing changed, nothing read");

    tick();
    append(&dir.path("p/s0007.jsonl"), &said("s0007"));
    assert_eq!(earlier(&b), [path_of(&dir, "a")]);
    assert_eq!(tail_reads(&folder), first + 1, "only the file that grew");

    tick();
    dir.write("p/c.jsonl", &said("c"));
    assert_eq!(earlier(&path_of(&dir, "c")), Vec::<String>::new());
    assert_eq!(tail_reads(&folder), first + 2, "only the new file");

    assert_eq!(earlier(&b), [path_of(&dir, "a")]);
    assert_eq!(tail_reads(&folder), first + 2);
}

#[test]
fn a_continued_in_far_from_the_end_of_a_big_file_is_still_found_but_not_beyond_the_window() {
    let dir = TempDir::new("continued-window");
    let filler = "x".repeat(200);
    let line = format!(r#"{{"type":"user","note":"{filler}"}}"#) + "\n";
    let near = continued("near", "b") + &line.repeat(50);
    dir.write("p/near.jsonl", &near);
    let far = continued("far", "b") + &line.repeat(200);
    dir.write("p/far.jsonl", &far);
    dir.write("p/b.jsonl", &said("b"));
    assert_eq!(earlier(&path_of(&dir, "b")), [path_of(&dir, "near")]);
}

#[test]
fn two_files_naming_the_same_session_give_the_one_written_last() {
    let dir = TempDir::new("continued-two");
    let early = r#"{"type":"continued-in","timestamp":"2026-11-13T07:00:00.000Z","sessionId":"x","continuedInSessionId":"b"}"#;
    let late = r#"{"type":"continued-in","timestamp":"2026-11-13T08:00:00.000Z","sessionId":"y","continuedInSessionId":"b"}"#;
    dir.write("p/x.jsonl", &format!("{early}\n"));
    dir.write("p/y.jsonl", &format!("{late}\n"));
    dir.write("p/b.jsonl", &said("b"));
    assert_eq!(earlier(&path_of(&dir, "b")), [path_of(&dir, "y")]);
}

#[test]
fn the_chain_stops_after_a_fixed_number_of_sessions() {
    let dir = TempDir::new("continued-long");
    let count = MAX_CHAIN + 4;
    for n in 0..count {
        let id = format!("s{n:02}");
        let next = format!("s{:02}", n + 1);
        dir.write(
            &format!("p/{id}.jsonl"),
            &(said(&id) + &continued(&id, &next)),
        );
    }
    let last = format!("s{count:02}");
    dir.write(&format!("p/{last}.jsonl"), &said(&last));
    let chain = earlier(&path_of(&dir, &last));
    assert_eq!(chain.len(), MAX_CHAIN);
    assert_eq!(
        chain.last(),
        Some(&path_of(&dir, &format!("s{:02}", count - 1)))
    );
}

#[test]
fn find_earlier_looks_in_the_folder_of_the_working_directory() {
    let dir = TempDir::new("continued-find");
    dir.write(
        "projects/-work-app/a.jsonl",
        &(said("a") + &continued("a", "b")),
    );
    let projects = dir.path("projects").to_string_lossy().into_owned();
    let found = find_earlier(&projects, "b", "/work/app");
    assert_eq!(
        found.map(PathBuf::from),
        Some(dir.path("projects/-work-app/a.jsonl"))
    );
    assert_eq!(find_earlier(&projects, "b", "/work/other"), None);
    assert_eq!(find_earlier(&projects, "nobody", "/work/app"), None);
}

#[test]
fn an_empty_or_unreadable_file_has_no_links() {
    let dir = TempDir::new("continued-empty");
    dir.write("p/empty.jsonl", "");
    dir.write(
        "p/half.jsonl",
        r#"{"type":"continued-in","sessionId":"half","continuedInSes"#,
    );
    dir.write("p/b.jsonl", &said("b"));
    assert_eq!(earlier(&path_of(&dir, "b")), Vec::<String>::new());
    assert_eq!(tail_reads(&dir.path("p")), 2, "the empty file is not read");
}
