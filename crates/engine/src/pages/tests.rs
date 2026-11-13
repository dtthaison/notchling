use super::content::*;
use super::*;
use crate::search::PageEntry;
use crate::sessions::testutil::TempDir;
use serde_json::{json, Value};
use std::path::Path;

pub(super) const URL_A: &str = "https://claude.ai/artifact/AbCdEfGhIjKlMnOpQrStUv";

pub(super) fn ctx<'a>(account: &'a str, home: &'a Path) -> RecordContext<'a> {
    RecordContext {
        account,
        now: 1_000.0,
        home,
    }
}

pub(super) fn publish_event(cwd: &str, file_path: &str, response: &str) -> Value {
    json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Artifact",
        "cwd": cwd,
        "tool_input": {"file_path": file_path},
        "tool_response": response,
    })
}

fn entry(folder: &str, file: &str) -> PageEntry {
    PageEntry {
        folder: folder.into(),
        file: file.into(),
        title: "T".into(),
        url: String::new(),
        source: String::new(),
        project: String::new(),
        account: String::new(),
        updated: 1.0,
        version: String::new(),
    }
}

#[test]
fn titles_come_from_title_tags_and_headings() {
    assert_eq!(
        page_title(
            b"<html><head><TITLE lang=x>\n  Cost &amp; Report\n  &lt;Q4&gt; </TITLE></head>",
            "html",
            "f.html"
        ),
        "Cost & Report <Q4>"
    );
    assert_eq!(
        page_title(b"intro\n\n# The Plan\n# Second", "md", "f.md"),
        "The Plan"
    );
    assert_eq!(page_title(b"<p>no title</p>", "html", "f.html"), "f.html");
    assert_eq!(page_title(b"<title> </title>", "html", "f.html"), "f.html");
    let long = format!("<title>{}</title>", "x".repeat(300));
    assert_eq!(
        page_title(long.as_bytes(), "html", "f").chars().count(),
        200
    );
}

#[test]
fn a_fragment_is_wrapped_and_a_document_is_not() {
    let wrapped = String::from_utf8(wrapped(b"<h1>Hi</h1>")).unwrap();
    assert!(wrapped.starts_with("<!doctype html>"));
    assert!(wrapped.contains("<body>\n<h1>Hi</h1>\n</body></html>"));
    let doc = b"<!DOCTYPE html><html></html>";
    assert_eq!(super::content::wrapped(doc), doc.to_vec());
}

#[test]
fn folder_names_are_a_slug_and_a_stable_hash() {
    let a = make_folder("Cost Report: Q4!", URL_A);
    assert!(a.starts_with("cost-report-q4-"), "{a}");
    assert_eq!(a.len(), "cost-report-q4-".len() + 8);
    assert_eq!(a, make_folder("Cost Report: Q4!", URL_A));
    assert_ne!(
        a,
        make_folder("Cost Report: Q4!", "https://claude.ai/artifact/other")
    );
    assert!(make_folder("日本語", "k").starts_with("page-"));
    assert!(make_folder(&"a".repeat(100), "k").len() <= 40 + 9);
    assert_eq!(make_folder("x", "a"), "x-8601ec8c");
}

#[test]
fn the_result_names_a_link_and_a_version() {
    let r = r#"{"output":"Published at https://claude.ai/code/artifact/KVBHDXYvY8f1b1AZG6PbAR (version id 1793506664-65e3)"}"#;
    assert_eq!(
        artifact_url_in(r).as_deref(),
        Some("https://claude.ai/code/artifact/KVBHDXYvY8f1b1AZG6PbAR")
    );
    assert_eq!(published_version(r), "1793506664-65e3");
    assert_eq!(
        published_version("(version 1793506664-65e3)"),
        "1793506664-65e3"
    );
    assert_eq!(published_version("no version here"), "");
    assert!(looks_failed("Publish refused: conflict"));
    assert!(!looks_failed("Published"));
    assert!(looks_missing("404 not found"));
    assert!(!looks_missing(&format!("404 {}", "x".repeat(2100))));
    assert_eq!(project_name("/w/notchling"), "notchling");
    assert_eq!(
        project_name("/w/notchling/.claude/worktrees/agent-1"),
        "notchling"
    );
    assert_eq!(project_name(""), "");
}

#[test]
fn only_plain_names_are_safe() {
    for ok in ["index.html", "cost-report-1a2b3c4d"] {
        assert!(safe_component(ok), "{ok}");
    }
    for bad in ["", ".", "..", "a/b", "../x", "a\\b", "a\0b", "C:x"] {
        assert!(!safe_component(bad), "{bad:?}");
    }
    assert_eq!(safe_relative("img/a.png"), Some(vec!["img", "a.png"]));
    for bad in ["/etc/passwd", "../x", "a/../b", "./a", "", "a\\b"] {
        assert_eq!(safe_relative(bad), None, "{bad:?}");
    }
}

#[test]
fn a_hand_edited_list_cannot_point_outside_the_library() {
    let dir = TempDir::new("pages-safe");
    let lib = PageLibrary::read_only(dir.path("pages"));
    assert_eq!(lib.local_path(&entry("../..", "etc")), None);
    assert_eq!(lib.local_path(&entry("a", "../../x")), None);
    assert_eq!(
        lib.local_path(&entry("a", "index.html")),
        Some(dir.path("pages/a/index.html"))
    );
    assert!(lib.supporting_files(&entry("..", "x")).is_empty());
    assert!(lib.describe(&entry("..", "x")).contains("damaged"));
}

#[test]
fn a_publish_is_saved_wrapped_and_listed() {
    let dir = TempDir::new("pages-rec");
    let src = dir.write(
        "work/proj/report.html",
        "<title>Cost Report</title><p>hi</p>",
    );
    let lib = PageLibrary::writable(dir.path("pages"));
    let response = format!("Published to {URL_A} (version id 1793506664-65e3)");
    let event = publish_event(
        dir.path("work/proj").to_str().unwrap_or(""),
        "report.html",
        &response,
    );
    let e = lib
        .record(&event, &ctx("me@example.com", dir.root()))
        .unwrap()
        .unwrap();
    assert_eq!(e.title, "Cost Report");
    assert_eq!(e.url, URL_A);
    assert_eq!(e.account, "me@example.com");
    assert_eq!(e.project, "proj");
    assert_eq!(e.version, "1793506664-65e3");
    assert_eq!(e.source, src.to_string_lossy());
    let saved =
        std::fs::read_to_string(dir.path("pages").join(&e.folder).join("index.html")).unwrap();
    assert!(saved.starts_with("<!doctype html>") && saved.contains("<p>hi</p>"));
    let raw: Value = serde_json::from_slice(&std::fs::read(lib.list_path()).unwrap()).unwrap();
    assert_eq!(raw["pages"][0]["folder"], json!(e.folder));
    assert_eq!(raw["pages"][0]["version"], json!("1793506664-65e3"));
    assert!(std::fs::read_to_string(lib.index_path())
        .unwrap()
        .contains("Cost Report"));
    assert_eq!(lib.entries().len(), 1);
    assert_eq!(lib.find("cost").len(), 1);
    assert_eq!(lib.find(URL_A).len(), 1);
}

#[test]
fn the_first_publisher_owns_the_page_and_a_republish_updates_it() {
    let dir = TempDir::new("pages-owner");
    let work = dir.path("work");
    dir.write("work/p.html", "<title>One</title>");
    let lib = PageLibrary::writable(dir.path("pages"));
    let cwd = work.to_str().unwrap_or("");
    let first = lib
        .record(
            &publish_event(cwd, "p.html", &format!("{URL_A} (version 1793506664-aaaa)")),
            &ctx("a@example.com", dir.root()),
        )
        .unwrap()
        .unwrap();
    dir.write("work/p.html", "<title>Two</title>");
    let second = lib
        .record(
            &publish_event(cwd, "p.html", &format!("{URL_A} (version 1793506999-bbbb)")),
            &ctx("b@example.com", dir.root()),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        second.folder, first.folder,
        "the same link keeps its folder"
    );
    assert_eq!(
        second.account, "a@example.com",
        "the first publisher stays the owner"
    );
    assert_eq!(second.title, "Two");
    assert_eq!(second.version, "1793506999-bbbb");
    assert_eq!(lib.entries().len(), 1);
    let third = lib
        .record(
            &publish_event(cwd, "p.html", URL_A),
            &ctx("b@example.com", dir.root()),
        )
        .unwrap()
        .unwrap();
    assert_eq!(third.version, "");
}

#[test]
fn a_page_with_no_link_is_keyed_by_its_source_and_an_empty_account_is_filled_in() {
    let dir = TempDir::new("pages-nolink");
    dir.write("work/p.md", "# Notes\nbody");
    let lib = PageLibrary::writable(dir.path("pages"));
    let cwd = dir.path("work").to_string_lossy().into_owned();
    let a = lib
        .record(&publish_event(&cwd, "p.md", "ok"), &ctx("", dir.root()))
        .unwrap()
        .unwrap();
    assert_eq!(
        (a.file.as_str(), a.title.as_str(), a.url.as_str()),
        ("index.md", "Notes", "")
    );
    let b = lib
        .record(
            &publish_event(&cwd, "p.md", "ok"),
            &ctx("me@example.com", dir.root()),
        )
        .unwrap()
        .unwrap();
    assert_eq!(b.folder, a.folder);
    assert_eq!(b.account, "me@example.com", "an unknown owner is filled in");
    assert_eq!(lib.entries().len(), 1);
}

#[test]
fn a_republish_may_change_the_kind_of_page() {
    let dir = TempDir::new("pages-kind");
    dir.write("w/p.md", "# A");
    dir.write("w/p.html", "<title>A</title>");
    let lib = PageLibrary::writable(dir.path("pages"));
    let cwd = dir.path("w").to_string_lossy().into_owned();
    let c = ctx("a", dir.root());
    let md = lib
        .record(&publish_event(&cwd, "p.md", URL_A), &c)
        .unwrap()
        .unwrap();
    let html = lib
        .record(&publish_event(&cwd, "p.html", URL_A), &c)
        .unwrap()
        .unwrap();
    assert_eq!(md.folder, html.folder);
    let folder = dir.path("pages").join(&html.folder);
    assert!(folder.join("index.html").exists() && !folder.join("index.md").exists());
}

#[test]
fn events_that_are_not_a_page_publish_record_nothing() {
    let dir = TempDir::new("pages-skip");
    dir.write("w/p.html", "<title>A</title>");
    dir.write("w/p.txt", "x");
    let lib = PageLibrary::writable(dir.path("pages"));
    let cwd = dir.path("w").to_string_lossy().into_owned();
    let c = ctx("a", dir.root());
    let mut asset = publish_event(&cwd, "p.html", URL_A);
    asset["tool_input"]["asset"] = json!(true);
    let mut read = publish_event(&cwd, "p.html", URL_A);
    read["tool_input"]["action"] = json!("read");
    for event in [
        publish_event(&cwd, "p.html", "Publish refused: conflict"),
        publish_event(&cwd, "p.html", "error: failed"),
        publish_event(&cwd, "p.txt", URL_A),
        publish_event(&cwd, "gone.html", URL_A),
        asset,
        read,
        json!({"tool_input": {}}),
        json!({}),
    ] {
        assert_eq!(lib.record(&event, &c).unwrap(), None, "{event}");
    }
    assert!(lib.entries().is_empty());
    assert!(lib
        .record(
            &publish_event(&cwd, "p.html", &format!("conflict, see {URL_A}")),
            &c
        )
        .unwrap()
        .is_some());
}

#[test]
fn a_file_over_the_cap_is_not_copied() {
    let dir = TempDir::new("pages-big");
    let big = dir.path("w/big.html");
    std::fs::create_dir_all(dir.path("w")).unwrap();
    let f = std::fs::File::create(&big).unwrap();
    f.set_len(MAX_FILE_BYTES + 1).unwrap();
    let lib = PageLibrary::writable(dir.path("pages"));
    let cwd = dir.path("w").to_string_lossy().into_owned();
    assert_eq!(
        lib.record(
            &publish_event(&cwd, "big.html", URL_A),
            &ctx("a", dir.root())
        )
        .unwrap(),
        None
    );
}

#[test]
fn supporting_files_are_copied_inside_the_folder_only() {
    let dir = TempDir::new("pages-support");
    dir.write("w/p.html", "<title>S</title>");
    dir.write("w/img/a.png", "png");
    dir.write("w/data.json", "{}");
    dir.write("outside.txt", "secret");
    let lib = PageLibrary::writable(dir.path("pages"));
    let cwd = dir.path("w").to_string_lossy().into_owned();
    let mut event = publish_event(&cwd, "p.html", URL_A);
    event["tool_input"]["files"] = json!({
        "img/a.png": "img/a.png",
        "data.json": {"from": "data.json"},
        "../escape.txt": "../outside.txt",
        "/abs.txt": "../outside.txt",
        "a/../../b.txt": "../outside.txt",
        "index.html": "data.json",
        "other.html": {"artifact": "x", "path": "y"},
    });
    let e = lib.record(&event, &ctx("a", dir.root())).unwrap().unwrap();
    let folder = dir.path("pages").join(&e.folder);
    assert_eq!(
        std::fs::read_to_string(folder.join("img/a.png")).unwrap(),
        "png"
    );
    assert_eq!(
        std::fs::read_to_string(folder.join("data.json")).unwrap(),
        "{}"
    );
    assert!(!folder.parent().unwrap().join("escape.txt").exists());
    assert!(!dir.path("pages/abs.txt").exists());
    assert!(!dir.path("b.txt").exists() && !dir.path("pages/b.txt").exists());
    assert!(!folder.join("other.html").exists());
    assert!(std::fs::read_to_string(folder.join("index.html"))
        .unwrap()
        .contains("<title>S</title>"));
    assert_eq!(lib.supporting_files(&e), vec!["data.json", "img/a.png"]);
    dir.write("w/assets/c.css", "css");
    let mut event = publish_event(&cwd, "p.html", URL_A);
    event["tool_input"]["root"] = json!("assets");
    event["tool_input"]["files"] = json!([{"path": "c.css"}]);
    let e = lib.record(&event, &ctx("a", dir.root())).unwrap().unwrap();
    assert!(dir.path("pages").join(&e.folder).join("c.css").exists());
}

#[test]
fn a_backfill_keeps_its_own_time_and_account() {
    let dir = TempDir::new("pages-backfill");
    dir.write("w/p.html", "<title>Old</title>");
    let lib = PageLibrary::writable(dir.path("pages"));
    let mut event = publish_event(&dir.path("w").to_string_lossy(), "p.html", URL_A);
    event["notchling_backfill"] = json!({"account": "old@example.com", "updated": 42.5});
    let e = lib
        .record(&event, &ctx("now@example.com", dir.root()))
        .unwrap()
        .unwrap();
    assert_eq!((e.account.as_str(), e.updated), ("old@example.com", 42.5));
}

#[test]
fn a_read_only_library_never_writes() {
    let dir = TempDir::new("pages-ro");
    dir.write("w/p.html", "<title>A</title>");
    let lib = PageLibrary::read_only(dir.path("pages"));
    let event = publish_event(&dir.path("w").to_string_lossy(), "p.html", URL_A);
    assert!(matches!(
        lib.record(&event, &ctx("a", dir.root())),
        Err(PagesError::ReadOnly)
    ));
    assert!(matches!(lib.remove("x"), Err(PagesError::ReadOnly)));
    assert!(!dir.path("pages").exists(), "not even the folder");
}

#[test]
fn two_publishes_at_once_keep_both_entries() {
    let dir = TempDir::new("pages-race");
    let pages = dir.path("pages");
    let mut handles = Vec::new();
    for n in 0..8 {
        dir.write(&format!("w/p{n}.html"), &format!("<title>Page {n}</title>"));
        let pages = pages.clone();
        let cwd = dir.path("w").to_string_lossy().into_owned();
        let home = dir.root().to_path_buf();
        handles.push(std::thread::spawn(move || {
            let lib = PageLibrary::writable(pages);
            let url = format!("https://claude.ai/artifact/Race{n:018}");
            lib.record(
                &publish_event(&cwd, &format!("p{n}.html"), &url),
                &ctx("a", &home),
            )
            .map(|e| e.is_some())
            .unwrap_or(false)
        }));
    }
    for h in handles {
        assert!(h.join().unwrap(), "every writer got the lock");
    }
    let lib = PageLibrary::read_only(pages);
    assert_eq!(lib.entries().len(), 8, "no entry was lost");
}

#[test]
fn remove_forgets_the_page_and_its_folder() {
    let dir = TempDir::new("pages-remove");
    dir.write("w/p.html", "<title>A</title>");
    let lib = PageLibrary::writable(dir.path("pages"));
    let e = lib
        .record(
            &publish_event(&dir.path("w").to_string_lossy(), "p.html", URL_A),
            &ctx("a", dir.root()),
        )
        .unwrap()
        .unwrap();
    dir.write("keep.txt", "k");
    lib.remove("..").unwrap();
    assert!(dir.path("keep.txt").exists());
    lib.remove(&e.folder).unwrap();
    assert!(lib.entries().is_empty());
    assert!(!dir.path("pages").join(&e.folder).exists());
}

#[test]
fn entries_are_newest_first_and_bad_ones_are_skipped() {
    let dir = TempDir::new("pages-read");
    dir.write(
        "pages/pages.json",
        r#"{"pages":[{"folder":"a","file":"index.html","updated":1},
        {"title":"no folder"},{"folder":"b","file":"index.html","updated":5,"title":"B"}]}"#,
    );
    let lib = PageLibrary::read_only(dir.path("pages"));
    let list = lib.entries();
    assert_eq!(
        list.iter().map(|e| e.folder.as_str()).collect::<Vec<_>>(),
        ["b", "a"]
    );
    assert_eq!(
        list[1].title, "a",
        "the folder stands in for a missing title"
    );
    assert!(lib.list_modified().is_some());
    assert!(PageLibrary::read_only(dir.path("nothing"))
        .entries()
        .is_empty());
    dir.write("pages2/pages.json", "not json");
    assert!(PageLibrary::read_only(dir.path("pages2"))
        .entries()
        .is_empty());
}

#[test]
fn describe_names_the_copy_and_who_can_republish() {
    let dir = TempDir::new("pages-describe");
    dir.write("w/p.html", "<title>Cost</title>");
    dir.write("w/x.css", "css");
    let lib = PageLibrary::writable(dir.path("pages"));
    let mut event = publish_event(&dir.path("w").to_string_lossy(), "p.html", URL_A);
    event["tool_input"]["files"] = json!({"x.css": "x.css"});
    let e = lib
        .record(&event, &ctx("me@example.com", dir.root()))
        .unwrap()
        .unwrap();
    std::fs::write(dir.path("pages").join(&e.folder).join(".hidden"), "h").unwrap();
    let text = lib.describe(&e);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "Title: Cost");
    assert_eq!(lines[1], format!("Link: {URL_A}"));
    assert_eq!(lines[2], "Published by: me@example.com");
    assert_eq!(lines[3], "Project: w");
    assert!(lines[4].starts_with("Last published: 19"), "{}", lines[4]);
    assert_eq!(
        lines[5],
        format!(
            "Local copy: {}",
            dir.path("pages")
                .join(&e.folder)
                .join("index.html")
                .display()
        )
    );
    assert_eq!(lines[6], "Supporting files (next to it): x.css");
    assert_eq!(lines[7], format!("Original source file: {}", e.source));
    let mut bare = e.clone();
    bare.url.clear();
    bare.account.clear();
    bare.project.clear();
    bare.source.clear();
    let text = lib.describe(&bare);
    assert!(text.contains("Link: (never published to claude.ai)"));
    assert!(text.contains("Published by: unknown account"));
    assert!(!text.contains("Project:") && !text.contains("Original source"));
}

#[test]
fn an_id_or_query_with_dots_and_slashes_never_becomes_a_path() {
    let dir = TempDir::new("pages-traversal");
    dir.write("w/p.html", "<title>A</title>");
    dir.write("secret.txt", "secret");
    let lib = PageLibrary::writable(dir.path("pages"));
    lib.record(
        &publish_event(&dir.path("w").to_string_lossy(), "p.html", URL_A),
        &ctx("a", dir.root()),
    )
    .unwrap();
    for query in [
        "../secret.txt",
        "../../etc/passwd",
        "https://claude.ai/artifact/../../etc/passwd",
        "claude.ai/code/artifact/..%2F..%2Fsecret",
        "AbCdEfGhIjKlMnOpQrStUv/../../secret",
    ] {
        assert!(lib.find(query).is_empty(), "{query}");
    }
    let found = lib.find("AbCdEfGhIjKlMnOpQrStUv");
    assert_eq!(found.len(), 1);
    let path = lib.local_path(&found[0]).unwrap();
    assert!(path.starts_with(dir.path("pages")));
    assert!(!path.components().any(|c| c.as_os_str() == ".."));
}
