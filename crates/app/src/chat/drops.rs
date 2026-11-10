//! Files dropped on the reply box (Finder, the screenshot thumbnail): each path goes into the
//! text at the caret, so Claude reads the file where it is, as a pasted path would. The Swift app
//! showed them as attachments and sent the paths after the text; here the path is the text, set
//! apart by spaces and quoted for the shell when it has any.

use std::path::{Path, PathBuf};

use notchling_kit::text_edit::TextEdit;

/// A path as a shell takes it: as is, or in single quotes when it holds a space, a quote or a
/// character the shell would read (a `'` inside becomes `'\''`).
pub fn quoted(path: &Path) -> String {
    let text = path.to_string_lossy();
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_alphanumeric() || "/._-+:@%,=~".contains(c));
    if plain {
        text.into_owned()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// How long a kept copy stays: long enough for the session to read it, and to read it again.
pub const KEEP_FOR_MS: i64 = 7 * 24 * 3600 * 1000;

/// A file macOS deletes soon after the drop: the screenshot thumbnail hands over a file in
/// `…/TemporaryItems/NSIRD_screencaptureui_…/`, gone as soon as the thumbnail is.
pub fn passing(path: &Path) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| s.starts_with("NSIRD_screencaptureui_"))
    })
}

/// The paths to put in the box: a [`passing`] file copied first into `store/<now_ms>/` (its own
/// name kept) so the session can still read it after macOS deletes the original; every other path
/// as it is, and a copy that fails keeps the original. Copies older than [`KEEP_FOR_MS`] go.
pub fn keep(paths: &[PathBuf], store: &Path, now_ms: i64) -> Vec<PathBuf> {
    if paths.iter().any(|p| passing(p)) {
        prune(store, now_ms);
    }
    let mut stamp = now_ms;
    paths
        .iter()
        .map(|path| {
            let (true, Some(name)) = (passing(path), path.file_name()) else {
                return path.clone();
            };
            // A folder of its own, so two drops of one name never meet.
            let mut dir = store.join(stamp.to_string());
            while dir.join(name).exists() {
                stamp += 1;
                dir = store.join(stamp.to_string());
            }
            let copy = dir.join(name);
            let done = std::fs::create_dir_all(&dir).and_then(|_| std::fs::copy(path, &copy));
            match done {
                Ok(_) => copy,
                Err(_) => {
                    let _ = std::fs::remove_dir(&dir);
                    path.clone()
                }
            }
        })
        .collect()
}

/// Removes the copies' folders older than [`KEEP_FOR_MS`]; folders not named by a time stay.
fn prune(store: &Path, now_ms: i64) {
    let Ok(entries) = std::fs::read_dir(store) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<i64>().ok())
            .is_some_and(|at| now_ms - at > KEEP_FOR_MS);
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// The dropped paths as one piece of text, a space between them.
pub fn paths_text(paths: &[impl AsRef<Path>]) -> String {
    paths
        .iter()
        .map(|p| quoted(p.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Puts the paths at the caret (over the selection), with a space before them unless the caret
/// follows one or starts the text, and one after so typing goes on. `false` when there was
/// nothing to put.
pub fn insert_paths(edit: &mut TextEdit, paths: &[impl AsRef<Path>]) -> bool {
    let text = paths_text(paths);
    if text.is_empty() {
        return false;
    }
    let before = &edit.text()[..edit.selection().start];
    let lead = if before.is_empty() || before.ends_with(char::is_whitespace) {
        ""
    } else {
        " "
    };
    edit.paste(&format!("{lead}{text} "));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_with_spaces_or_quotes_is_single_quoted_for_the_shell() {
        assert_eq!(
            quoted(Path::new("/Users/me/notes.md")),
            "/Users/me/notes.md"
        );
        assert_eq!(
            quoted(Path::new("/Users/me/Screen Shot.png")),
            "'/Users/me/Screen Shot.png'"
        );
        assert_eq!(quoted(Path::new("/tmp/it's.txt")), r"'/tmp/it'\''s.txt'");
        assert_eq!(quoted(Path::new("/tmp/$HOME")), "'/tmp/$HOME'");
        assert_eq!(quoted(Path::new("/tmp/ảnh.png")), "/tmp/ảnh.png");
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("notchling-drops-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_the_screenshot_thumbnails_short_lived_files_count_as_passing() {
        assert!(passing(Path::new(
            "/var/folders/x1/T/TemporaryItems/NSIRD_screencaptureui_ab12/Screenshot 2026-11-10 at 17.10.png"
        )));
        assert!(passing(Path::new(
            "/private/var/folders/x1/T/TemporaryItems/NSIRD_screencaptureui_ab12/a.png"
        )));
        assert!(!passing(Path::new("/Users/me/Desktop/Screenshot.png")));
        assert!(!passing(Path::new("/var/folders/x1/T/build.log")));
    }

    #[test]
    fn a_passing_file_is_copied_where_it_stays_and_others_keep_their_path() {
        let dir = scratch("keep");
        let temp = dir.join("TemporaryItems/NSIRD_screencaptureui_1");
        std::fs::create_dir_all(&temp).unwrap();
        let shot = temp.join("Screenshot 1.png");
        std::fs::write(&shot, b"png").unwrap();
        let mine = dir.join("Desktop/notes.md");
        let store = dir.join("drops");
        let kept = keep(&[shot.clone(), mine.clone()], &store, 1_000);
        assert_eq!(kept[1], mine, "a file that stays keeps its path");
        assert!(kept[0].starts_with(&store));
        assert_eq!(
            kept[0].file_name().unwrap().to_string_lossy(),
            "Screenshot 1.png"
        );
        assert_eq!(std::fs::read(&kept[0]).unwrap(), b"png");
        // macOS deletes its file; the copy stays.
        std::fs::remove_dir_all(&temp).unwrap();
        assert!(kept[0].exists());
        // The same name dropped again doesn't overwrite the first.
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::write(&shot, b"second").unwrap();
        let again = keep(std::slice::from_ref(&shot), &store, 2_000);
        assert_ne!(again[0], kept[0]);
        assert_eq!(std::fs::read(&again[0]).unwrap(), b"second");
        // A file already gone keeps its path: the box shows what was dropped.
        let gone = temp.join("gone.png");
        assert_eq!(keep(std::slice::from_ref(&gone), &store, 3_000), vec![gone]);
    }

    #[test]
    fn dropped_paths_join_with_spaces() {
        let paths = [PathBuf::from("/a/b.rs"), PathBuf::from("/c d/e.png")];
        assert_eq!(paths_text(&paths), "/a/b.rs '/c d/e.png'");
        let none: [PathBuf; 0] = [];
        assert_eq!(paths_text(&none), "");
    }

    #[test]
    fn paths_land_at_the_caret_set_apart_by_spaces() {
        let mut e = TextEdit::new("look at");
        assert!(insert_paths(&mut e, &[Path::new("/a/b.rs")]));
        assert_eq!(e.text(), "look at /a/b.rs ");
        assert_eq!(e.caret(), e.text().len());
        // After a space, or at the start, no extra space before.
        let mut s = TextEdit::new("see ");
        insert_paths(&mut s, &[Path::new("/x")]);
        assert_eq!(s.text(), "see /x ");
        let mut empty = TextEdit::new("");
        insert_paths(&mut empty, &[Path::new("/x y"), Path::new("/z")]);
        assert_eq!(empty.text(), "'/x y' /z ");
        // Over a selection, in the middle.
        let mut mid = TextEdit::new("fix THIS please");
        mid.set_selection(4, 8);
        insert_paths(&mut mid, &[Path::new("/f.rs")]);
        assert_eq!(mid.text(), "fix /f.rs  please");
        assert_eq!(mid.caret(), 10);
        // Nothing dropped, nothing done.
        let mut none = TextEdit::new("a");
        let nothing: [PathBuf; 0] = [];
        assert!(!insert_paths(&mut none, &nothing));
        assert_eq!(none.text(), "a");
        // One undo step.
        assert!(mid.undo());
        assert_eq!(mid.text(), "fix THIS please");
    }
}
