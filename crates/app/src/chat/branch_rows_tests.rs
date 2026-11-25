use super::*;
use crate::chat::git::{runs, DiffText};

fn file(path: &str, added: u64) -> ChangedFile {
    ChangedFile {
        path: path.into(),
        status: "M".into(),
        from: None,
        added: Some(added),
        removed: Some(0),
        uncommitted: false,
    }
}

fn files(n: usize) -> Vec<ChangedFile> {
    (0..n)
        .map(|i| file(&format!("f{i}.rs"), i as u64))
        .collect()
}

fn ready(text: &str, truncated: bool) -> Slot {
    Slot::from_diff(&DiffText {
        runs: runs(text),
        truncated,
        untracked: false,
    })
}

#[test]
fn closed_files_are_one_row_each_in_the_order_shown() {
    let f = files(4);
    let got = rows(&f, &[3, 1], &HashMap::new());
    assert_eq!(got, [Row::File(3), Row::File(1)]);
    assert_eq!(rows(&f, &[9], &HashMap::new()), Vec::<Row>::new());
}

#[test]
fn an_open_file_shows_a_loading_note_its_chunks_or_why_it_failed() {
    let f = files(3);
    let mut open = HashMap::new();
    open.insert("f1.rs".to_owned(), Slot::Loading);
    let got = rows(&f, &[0, 1, 2], &open);
    assert_eq!(got.len(), 4);
    assert_eq!(got[1], Row::File(1));
    assert!(matches!(&got[2], Row::Note(text, Ink::TextMuted) if text.contains("Reading")));
    open.insert("f1.rs".to_owned(), Slot::Failed("nope".into()));
    assert_eq!(
        rows(&f, &[1], &open)[1],
        Row::Note("nope".into(), Ink::Expired)
    );
    open.insert(
        "f1.rs".to_owned(),
        ready("diff --git a/x b/x\n@@ -1 +1 @@\n-a\n+b\n", true),
    );
    let got = rows(&f, &[1], &open);
    assert_eq!(got[0], Row::File(1));
    let inks: Vec<Ink> = got[1..]
        .iter()
        .map(|r| match r {
            Row::Chunk(ink, _) | Row::Note(_, ink) => *ink,
            Row::File(_) => panic!("one file"),
        })
        .collect();
    assert_eq!(
        inks,
        [
            Ink::TextFaint,
            Ink::AccentText,
            Ink::Expired,
            Ink::Done,
            Ink::TextMuted
        ]
    );
    open.insert("f1.rs".to_owned(), ready("", false));
    assert!(matches!(&rows(&f, &[1], &open)[1], Row::Note(text, _) if text.contains("no changes")));
}

#[test]
fn a_long_diff_is_cut_into_chunks_of_eighty_lines() {
    let text: String = (0..200).map(|i| format!("+line {i}\n")).collect();
    let text = format!("diff --git a/x b/x\n@@ -0,0 +1,200 @@\n{text}");
    let Slot::Ready { chunks, .. } = ready(&text, false) else {
        panic!("ready");
    };
    let added: Vec<usize> = chunks
        .iter()
        .filter(|(ink, _)| *ink == Ink::Done)
        .map(|(_, t)| t.split('\n').count())
        .collect();
    assert_eq!(added, [80, 80, 40]);
}

#[test]
fn a_change_is_the_smallest_middle_to_replace_and_the_rest_keeps_its_heights() {
    let f = files(5);
    let closed = rows(&f, &[0, 1, 2, 3, 4], &HashMap::new());
    let mut open = HashMap::new();
    open.insert("f2.rs".to_owned(), Slot::Loading);
    let opened = rows(&f, &[0, 1, 2, 3, 4], &open);
    assert_eq!(splice(&closed, &opened), (3, 0, 1));
    assert_eq!(splice(&opened, &closed), (3, 1, 0));
    assert_eq!(splice(&closed, &closed), (5, 0, 0));
    open.insert("f2.rs".to_owned(), ready("@@ -1 +1 @@\n+a\n", false));
    let loaded = rows(&f, &[0, 1, 2, 3, 4], &open);
    let (start, old, new) = splice(&opened, &loaded);
    assert_eq!(start, 3);
    assert_eq!((old, new), (1, loaded.len() - opened.len() + 1));
    assert_eq!(splice(&[], &closed), (0, 0, 5));
    assert_eq!(splice(&closed, &[]), (0, 5, 0));
}

#[test]
fn filtering_replaces_the_rows_that_differ() {
    let f = files(6);
    let all = rows(&f, &[0, 1, 2, 3, 4, 5], &HashMap::new());
    let some = rows(&f, &[1, 2], &HashMap::new());
    let (start, old, new) = splice(&all, &some);
    assert_eq!((start, new), (0, 2));
    assert_eq!(old, 6);
}

#[test]
fn a_new_stat_refetches_only_the_open_files_that_changed_and_drops_gone_ones() {
    let old = vec![file("a.rs", 1), file("b.rs", 2), file("c.rs", 3)];
    let mut new = vec![file("a.rs", 1), file("b.rs", 5)];
    let mut open = HashMap::new();
    for p in ["a.rs", "b.rs", "c.rs"] {
        open.insert(p.to_owned(), Slot::Loading);
    }
    let got = refetch(&old, &new, &open);
    assert_eq!(got.gone, ["c.rs"]);
    assert_eq!(got.again, ["b.rs"]);
    new.push(file("d.rs", 1));
    open.insert("d.rs".to_owned(), Slot::Loading);
    assert_eq!(refetch(&old, &new, &open).again, ["b.rs", "d.rs"]);
    assert_eq!(refetch(&old, &old, &open).gone, ["d.rs"]);
}
