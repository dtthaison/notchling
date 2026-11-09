//! Files dropped on the reply box (Finder, the screenshot thumbnail): each path goes into the
//! text at the caret, so Claude reads the file where it is, as a pasted path would. The Swift app
//! showed them as attachments and sent the paths after the text; here the path is the text, set
//! apart by spaces and quoted for the shell when it has any.

use std::path::Path;

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
    use std::path::PathBuf;

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
