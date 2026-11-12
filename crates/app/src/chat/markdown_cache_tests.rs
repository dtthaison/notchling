use std::cell::Cell;
use std::rc::Rc;

use super::*;
use crate::testutil::Scratch;

/// The real disk under a scratch home, counting every question asked of it.
struct Counting {
    home: PathBuf,
    asked: Rc<Cell<usize>>,
}

impl Probe for Counting {
    fn exists(&self, path: &Path) -> bool {
        self.asked.set(self.asked.get() + 1);
        path.exists()
    }
    fn is_dir(&self, path: &Path) -> bool {
        self.asked.set(self.asked.get() + 1);
        path.is_dir()
    }
    fn home(&self) -> Option<&Path> {
        Some(&self.home)
    }
}

fn cache(s: &Scratch) -> (Cache<Counting>, Rc<Cell<usize>>) {
    let asked = Rc::new(Cell::new(0));
    let probe = Counting {
        home: s.path().to_path_buf(),
        asked: asked.clone(),
    };
    (Cache::with_probe(probe), asked)
}

/// Where the first paragraph's spans link, by their text.
fn links(message: &Message) -> Vec<(String, Option<String>)> {
    let spans = match message.first() {
        Some(Part::Paragraph(inline)) => &inline.spans,
        other => panic!("not a paragraph: {other:?}"),
    };
    spans
        .iter()
        .filter(|s| s.link.is_some() || s.code)
        .map(|s| (s.text.clone(), s.link.clone()))
        .collect()
}

#[test]
fn a_message_drawn_again_is_neither_parsed_nor_looked_up_again() {
    let s = Scratch::new("md-cache-hit");
    s.write("src/a.rs", "");
    let folder = s.path().to_string_lossy().into_owned();
    let (mut cache, asked) = cache(&s);
    let text = "See `src/a.rs` and `src/b.rs`.\n\n- one\n- two";
    let first = cache.message("m1", text, &folder);
    assert!(asked.get() > 0, "the first draw asks the disk");
    let after_first = asked.get();
    let again = cache.message("m1", text, &folder);
    assert!(Rc::ptr_eq(&first, &again), "the same prepared message");
    assert_eq!(asked.get(), after_first, "no disk on a second draw");
    let file = s.path().join("src/a.rs").to_string_lossy().into_owned();
    assert_eq!(
        links(&first),
        vec![
            ("src/a.rs".to_owned(), Some(file)),
            ("src/b.rs".to_owned(), None)
        ]
    );
    assert!(matches!(first.get(1), Some(Part::List(items)) if items.len() == 2));
}

#[test]
fn a_changed_text_or_folder_is_prepared_again() {
    let s = Scratch::new("md-cache-text");
    let folder = s.path().to_string_lossy().into_owned();
    let (mut cache, _) = cache(&s);
    let first = cache.message("m1", "Writing", &folder);
    let grown = cache.message("m1", "Writing more", &folder);
    assert!(!Rc::ptr_eq(&first, &grown));
    assert!(
        matches!(grown.first(), Some(Part::Paragraph(i)) if i.spans[0].text == "Writing more"),
        "a reply still streaming shows its new words"
    );
    let moved = cache.message("m1", "Writing more", "/elsewhere");
    assert!(!Rc::ptr_eq(&grown, &moved), "another folder links again");
}

#[test]
fn a_reload_asks_the_disk_again_and_finds_a_new_file() {
    let s = Scratch::new("md-cache-reload");
    let folder = s.path().to_string_lossy().into_owned();
    let (mut cache, asked) = cache(&s);
    let text = "Wrote `out/report.md`.";
    assert_eq!(
        links(&cache.message("m1", text, &folder)),
        vec![("out/report.md".to_owned(), None)]
    );
    let report = s.write("out/report.md", "# Report");
    // Until the next load, the disk's earlier answer stands: draws never ask it.
    let before = asked.get();
    assert_eq!(
        links(&cache.message("m1", text, &folder)),
        vec![("out/report.md".to_owned(), None)]
    );
    assert_eq!(asked.get(), before);
    cache.reloaded();
    assert_eq!(
        links(&cache.message("m1", text, &folder)),
        vec![(
            "out/report.md".to_owned(),
            Some(report.to_string_lossy().into_owned())
        )]
    );
    assert!(asked.get() > before, "the reload's draw asked the disk");
}

#[test]
fn another_session_forgets_every_message() {
    let s = Scratch::new("md-cache-switch");
    let (mut cache, asked) = cache(&s);
    let first = cache.message("m1", "Hello /tmp", "");
    cache.switched();
    assert!(cache.messages.is_empty());
    let before = asked.get();
    let again = cache.message("m1", "Hello /tmp", "");
    assert!(!Rc::ptr_eq(&first, &again));
    assert_eq!(first, again);
    assert!(asked.get() > before);
}

#[test]
fn is_file_is_asked_once_per_load() {
    let s = Scratch::new("md-cache-file");
    let picture = s.write("shot.png", "png");
    let (mut cache, asked) = cache(&s);
    assert!(cache.is_file(&picture));
    assert!(!cache.is_file(s.path()), "a folder is not a file");
    let before = asked.get();
    assert!(cache.is_file(&picture));
    assert_eq!(asked.get(), before);
    std::fs::remove_file(&picture).unwrap();
    assert!(cache.is_file(&picture), "still this load's answer");
    cache.reloaded();
    assert!(!cache.is_file(&picture));
}

#[test]
fn linked_images_are_kept_only_when_they_are_files() {
    let s = Scratch::new("md-cache-images");
    let shot = s.write("shot.png", "png");
    let (mut cache, _) = cache(&s);
    let text = format!(
        "Made [this]({}) and [that]({}/gone.png).",
        shot.display(),
        s.path().display()
    );
    let message = cache.message("m1", &text, "");
    match message.first() {
        Some(Part::Paragraph(inline)) => {
            assert_eq!(inline.images, vec![shot.to_string_lossy().into_owned()])
        }
        other => panic!("not a paragraph: {other:?}"),
    }
}

#[test]
fn every_block_kind_is_prepared() {
    let s = Scratch::new("md-cache-kinds");
    let (mut cache, _) = cache(&s);
    let text = "# Title\n\npara\n\n> quoted\n\n```rust\nfn x() {}\n```\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n---";
    let message = cache.message("m1", text, "");
    let kinds: Vec<&str> = message
        .iter()
        .map(|p| match p {
            Part::Heading { .. } => "heading",
            Part::Paragraph(_) => "paragraph",
            Part::List(_) => "list",
            Part::Quote(_) => "quote",
            Part::Code { .. } => "code",
            Part::Table { .. } => "table",
            Part::Rule => "rule",
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["heading", "paragraph", "quote", "code", "table", "rule"]
    );
    match &message[4] {
        Part::Table { header, rows } => {
            assert_eq!(header.len(), 2);
            assert_eq!(rows[0][1][0].text, "2");
        }
        other => panic!("not a table: {other:?}"),
    }
}
