use super::code_tokens::Lang;
use super::file_panel::{kind, load, Body, Kind};
use crate::testutil::Scratch;
use std::path::Path;

#[test]
fn code_json_and_jsonl_are_told_apart_by_name() {
    let text = b"x";
    assert_eq!(kind(Path::new("a/app.rb"), text), Kind::Code(Lang::Ruby));
    assert_eq!(kind(Path::new("a/lib.rs"), text), Kind::Code(Lang::Rust));
    assert_eq!(kind(Path::new("a/x.json"), text), Kind::Json);
    assert_eq!(kind(Path::new("a/x.jsonl"), text), Kind::Jsonl);
    assert_eq!(kind(Path::new("a/x.ndjson"), text), Kind::Jsonl);
    assert_eq!(kind(Path::new("a/x.txt"), text), Kind::Text);
    assert_eq!(kind(Path::new("a/x.json"), &[0, 1, 2]), Kind::Other);
    assert_eq!(kind(Path::new("a/x.rb"), &[0, 1, 2]), Kind::Other);
}

#[test]
fn code_loads_with_its_tokens_once() {
    let s = Scratch::new("view-code");
    let rb = s.write("a.rb", "def go\n  1\nend\n");
    match load(&rb).body {
        Body::Code(code) => {
            assert_eq!(code.lang, Lang::Ruby);
            assert!(!code.spans.is_empty());
            assert_eq!(code.text, "def go\n  1\nend\n");
            assert_eq!(code.lines, 3);
            assert!(code.invalid.is_none());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn valid_json_is_pretty_and_invalid_json_is_raw_with_a_note() {
    let s = Scratch::new("view-json");
    let ok = s.write("a.json", "{\"a\":[1,2]}");
    match load(&ok).body {
        Body::Code(code) => {
            assert_eq!(code.lang, Lang::Json);
            assert_eq!(code.text, "{\n  \"a\": [\n    1,\n    2\n  ]\n}");
            assert!(code.invalid.is_none());
        }
        other => panic!("{other:?}"),
    }
    let bad = s.write("b.json", "{\"a\": ,}");
    match load(&bad).body {
        Body::Code(code) => {
            assert_eq!(code.text, "{\"a\": ,}");
            assert_eq!(code.invalid, Some((1, 7)));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_jsonl_file_loads_its_last_lines_with_numbers() {
    let s = Scratch::new("view-jsonl");
    let body: String = (1..=500)
        .map(|i| format!("{{\"type\":\"user\",\"n\":{i}}}\n"))
        .collect();
    let path = s.write("t.jsonl", &body);
    match load(&path).body {
        Body::Jsonl(j) => {
            assert_eq!(j.rows.len(), super::jsonl_view::PAGE_LINES);
            assert_eq!(j.rows.last().unwrap().number, 500);
            assert_eq!(j.rows[0].number, 500 - super::jsonl_view::PAGE_LINES + 1);
            assert!(j.more());
            assert_eq!(j.rows[0].summary.tag, "user");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_picture_format_opens_as_an_image() {
    for name in ["a.png", "a.jpg", "a.jpeg", "a.gif", "a.webp", "a.svg"] {
        assert!(
            matches!(kind(Path::new(name), b"x"), Kind::Image(_)),
            "{name}"
        );
    }
}

#[cfg(unix)]
fn fifo(s: &Scratch) -> std::path::PathBuf {
    let path = s.path().join("pipe.txt");
    let made = std::process::Command::new("mkfifo").arg(&path).status();
    assert!(made.is_ok_and(|m| m.success()));
    path
}

#[cfg(unix)]
fn within<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx.recv_timeout(std::time::Duration::from_secs(3)).ok()
}

#[cfg(unix)]
#[test]
fn a_pipe_is_not_read_and_does_not_block() {
    let s = Scratch::new("view-fifo");
    let pipe = fifo(&s);
    let body = within(move || load(&pipe).body).expect("load must not block on a pipe");
    assert_eq!(body, Body::Other);
}

#[cfg(unix)]
#[test]
fn a_pipe_is_classified_without_a_read() {
    let s = Scratch::new("view-fifo-opens");
    let pipe = fifo(&s);
    let again = pipe.clone();
    let opens = within(move || super::file_panel::opens(&again)).expect("opens must not block");
    assert!(matches!(opens, crate::chat::page_panel::Opens::System(p) if p == pipe));
}

#[cfg(unix)]
#[test]
fn a_device_is_not_a_regular_file() {
    assert!(super::file_panel::regular(Path::new("/dev/null")).is_none());
    let body = within(|| load(Path::new("/dev/null")).body).expect("no block");
    assert_eq!(body, Body::Other);
}

#[cfg(unix)]
#[test]
fn a_link_to_a_pipe_is_refused_and_a_link_to_a_file_is_kept() {
    let s = Scratch::new("view-links");
    let pipe = fifo(&s);
    let to_pipe = s.path().join("to-pipe.txt");
    std::os::unix::fs::symlink(&pipe, &to_pipe).unwrap();
    assert!(super::file_panel::regular(&to_pipe).is_none());
    let file = s.write("real.txt", "hi\n");
    let to_file = s.path().join("to-file.txt");
    std::os::unix::fs::symlink(&file, &to_file).unwrap();
    assert!(super::file_panel::regular(&to_file).is_some());
    assert!(super::file_panel::regular(&file).is_some());
    assert!(super::file_panel::regular(&s.path().join("none")).is_none());
}
