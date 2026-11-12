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

/// The kinds of file drawn as a picture.
const IMAGE_KINDS: [&str; 8] = [
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".heic", ".tiff", ".bmp",
];

/// Whether `path` names a picture, by its kind.
pub fn is_image(path: &Path) -> bool {
    let lower = path.to_string_lossy().to_lowercase();
    IMAGE_KINDS.iter().any(|k| lower.ends_with(k))
}

/// The words of `text` that are pictures, with where each sits (its quotes included): the words
/// read as the shell reads them (single quotes, a backslash before one character), kept when one
/// is an absolute path ending in an image kind. A quote left open ends the reading. Trailing
/// punctuation after a bare path is not part of it.
fn image_spans(text: &str) -> Vec<(std::ops::Range<usize>, PathBuf)> {
    let mut out = Vec::new();
    let mut keep = |word: &mut String, from: usize, to: usize, bare_end: bool| {
        let mut to = to;
        if bare_end {
            while let Some(c) = word.chars().last().filter(|c| ",.;:!?)".contains(*c)) {
                word.pop();
                to -= c.len_utf8();
            }
        }
        let path = PathBuf::from(word.as_str());
        if word.starts_with('/') && is_image(&path) {
            out.push((from..to, path));
        }
        word.clear();
    };
    let mut word = String::new();
    // Where the word being read starts, if one is.
    let mut from: Option<usize> = None;
    // Whether the word's last piece was outside quotes, so its punctuation may be trimmed.
    let mut bare = false;
    let mut chars = text.char_indices();
    while let Some((at, c)) = chars.next() {
        if !c.is_whitespace() && from.is_none() {
            from = Some(at);
        }
        match c {
            '\'' => {
                let mut closed = false;
                for (_, q) in chars.by_ref() {
                    if q == '\'' {
                        closed = true;
                        break;
                    }
                    word.push(q);
                }
                if !closed {
                    return out;
                }
                bare = false;
            }
            '\\' => {
                if let Some((_, next)) = chars.next() {
                    word.push(next);
                }
                bare = false;
            }
            c if c.is_whitespace() => {
                if let Some(start) = from.take() {
                    keep(&mut word, start, at, bare);
                }
            }
            c => {
                word.push(c);
                bare = true;
            }
        }
    }
    if let Some(start) = from {
        keep(&mut word, start, text.len(), bare);
    }
    out
}

/// The pictures `text` names, each once, in order. What [`paths_text`] writes reads back here.
pub fn image_words(text: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for (_, path) in image_spans(text) {
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// A sent message as its bubble shows it: the words, without the pictures `exists` finds (with a
/// space beside each), and those pictures, to be drawn as pictures.
pub fn split_images(text: &str, exists: impl Fn(&Path) -> bool) -> (String, Vec<PathBuf>) {
    let mut words = String::new();
    let mut pictures: Vec<PathBuf> = Vec::new();
    let mut at = 0;
    for (range, path) in image_spans(text) {
        if !exists(&path) {
            continue;
        }
        let mut before = &text[at..range.start];
        let mut end = range.end;
        if let Some(cut) = before.strip_suffix(' ') {
            before = cut;
        } else if text[end..].starts_with(' ') {
            end += 1;
        }
        words.push_str(before);
        at = end;
        if !pictures.contains(&path) {
            pictures.push(path);
        }
    }
    words.push_str(&text[at..]);
    (words.trim().to_owned(), pictures)
}

/// What goes to the session: the typed words, then the attached files' paths after a blank line.
pub fn with_attachments(text: &str, files: &[PathBuf]) -> String {
    let text = text.trim();
    let paths = paths_text(files);
    match (text.is_empty(), paths.is_empty()) {
        (_, true) => text.to_owned(),
        (true, false) => paths,
        (false, false) => format!("{text}\n\n{paths}"),
    }
}

/// A file on this Mac as a `file://` URL, for macOS to open: every byte but the plain ones escaped.
pub fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

/// The file or folder on this Mac a reply's link names, or `None` for a web link or a path that
/// isn't there. Agents link files relative to the session's `folder` (Codex: `<out/a.png>`), and
/// macOS can't open a bare relative path (Finder's error -50). A `:12` or `#L12` after the name
/// is a line, tried off when the whole doesn't exist.
pub fn local_link(url: &str, folder: &str) -> Option<std::path::PathBuf> {
    let raw = match url.strip_prefix("file://") {
        Some(path) => path,
        None if has_scheme(url) => return None,
        None => url,
    };
    let text = percent_decoded(raw);
    let base = |path: &str| -> Option<std::path::PathBuf> {
        let p = if let Some(rest) = path.strip_prefix("~/") {
            std::path::PathBuf::from(std::env::var_os("HOME")?).join(rest)
        } else if path.starts_with('/') {
            std::path::PathBuf::from(path)
        } else if folder.is_empty() {
            return None;
        } else {
            Path::new(folder).join(path.strip_prefix("./").unwrap_or(path))
        };
        p.exists().then_some(p)
    };
    base(&text).or_else(|| {
        let cut = text.rfind("#L").or_else(|| {
            text.rfind(':').filter(|&i| {
                text[i + 1..]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b':')
            })
        })?;
        base(&text[..cut])
    })
}

/// `https:`, `mailto:`: letters before a colon, and no digit after it (`a.rs:12` is a line).
fn has_scheme(url: &str) -> bool {
    let Some(colon) = url.find(':') else {
        return false;
    };
    let scheme = &url[..colon];
    scheme.len() > 1
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        && !url[colon + 1..].starts_with(|c: char| c.is_ascii_digit())
}

/// `%20` and the like back to their bytes; anything that isn't a valid escape stays as written.
pub(crate) fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// Where a row's attached files are kept among the drafts: the row's id and a separator no id has.
pub fn attachments_key(row: &str) -> String {
    format!("{row}\u{1f}files")
}

/// The row an [`attachments_key`] belongs to; `None` for a row's own draft.
pub fn attachments_row(key: &str) -> Option<&str> {
    key.strip_suffix("\u{1f}files")
}

/// Attached files as a draft's text, a line each.
pub fn attachments_text(files: &[PathBuf]) -> String {
    files
        .iter()
        .map(|f| f.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

/// [`attachments_text`] read back.
pub fn attachments_from(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
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

    /// Codex links its files relative to the session's folder (`[a.png](<out/a.png>)`); handed to
    /// macOS as they are, Finder said "The application can't be opened. -50".
    #[test]
    fn a_relative_link_is_the_file_in_the_session_folder() {
        let dir = std::env::temp_dir().join(format!("nl-link-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("out")).unwrap();
        std::fs::write(dir.join("out/S1 a.png"), b"x").unwrap();
        let folder = dir.to_string_lossy().into_owned();
        let at = |url: &str| local_link(url, &folder);
        assert_eq!(at("out/S1 a.png"), Some(dir.join("out/S1 a.png")));
        assert_eq!(at("./out/S1 a.png"), Some(dir.join("out/S1 a.png")));
        assert_eq!(at("out/S1%20a.png"), Some(dir.join("out/S1 a.png")));
        let abs = dir.join("out/S1 a.png").to_string_lossy().into_owned();
        assert_eq!(at(&abs), Some(dir.join("out/S1 a.png")));
        assert_eq!(at(&format!("file://{abs}")), Some(dir.join("out/S1 a.png")));
        // A line after the name (`a.rs:12`, `a.rs#L12`) still finds the file.
        assert_eq!(at("out/S1 a.png:12"), Some(dir.join("out/S1 a.png")));
        assert_eq!(at("out/S1 a.png#L3"), Some(dir.join("out/S1 a.png")));
        // A folder opens too; a web link, a missing file or no session folder is not a file.
        assert_eq!(at("out"), Some(dir.join("out")));
        assert_eq!(at("https://example.com/out"), None);
        assert_eq!(at("mailto:a@b.c"), None);
        assert_eq!(at("out/missing.png"), None);
        assert_eq!(local_link("out/S1 a.png", ""), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
    fn the_image_paths_in_the_text_are_read_back_as_the_shell_reads_them() {
        let text = "look at '/Users/me/Screen Shot.png' and /tmp/a.JPG, not /tmp/notes.md \
                    or rel/b.png; '/tmp/it'\\''s.webp' twice /tmp/a.JPG 'unclosed /tmp/c.png";
        assert_eq!(
            image_words(text),
            vec![
                PathBuf::from("/Users/me/Screen Shot.png"),
                PathBuf::from("/tmp/a.JPG"),
                PathBuf::from("/tmp/it's.webp"),
            ]
        );
        let dropped = [
            PathBuf::from("/tmp/Shot 1.png"),
            PathBuf::from("/tmp/x.gif"),
        ];
        assert_eq!(image_words(&paths_text(&dropped)), dropped.to_vec());
    }

    #[test]
    fn a_message_with_attachments_ends_with_their_paths() {
        let files = [PathBuf::from("/tmp/Shot 1.png"), PathBuf::from("/tmp/a.rs")];
        assert_eq!(
            with_attachments("  look at these \n", &files),
            "look at these\n\n'/tmp/Shot 1.png' /tmp/a.rs"
        );
        assert_eq!(with_attachments("", &files), "'/tmp/Shot 1.png' /tmp/a.rs");
        assert_eq!(with_attachments(" hi ", &[] as &[PathBuf]), "hi");
    }

    #[test]
    fn a_sent_message_shows_its_pictures_apart_from_its_words() {
        let text = "look at these\n\n'/tmp/Shot 1.png' /tmp/a.rs /tmp/gone.png";
        let (words, pictures) = split_images(text, |p| p != Path::new("/tmp/gone.png"));
        assert_eq!(words, "look at these\n\n/tmp/a.rs /tmp/gone.png");
        assert_eq!(pictures, vec![PathBuf::from("/tmp/Shot 1.png")]);
        let (words, pictures) = split_images("'/tmp/x y.png'", |_| true);
        assert_eq!((words.as_str(), pictures.len()), ("", 1));
        let (words, _) = split_images("before /tmp/b.png after", |_| true);
        assert_eq!(words, "before after");
    }

    #[test]
    fn a_path_opens_as_a_file_url_with_its_spaces_escaped() {
        assert_eq!(
            file_url(Path::new("/Users/me/Screen Shot 12.00 AM.png")),
            "file:///Users/me/Screen%20Shot%2012.00%20AM.png"
        );
        assert_eq!(
            file_url(Path::new("/tmp/ảnh#1.png")),
            "file:///tmp/%E1%BA%A3nh%231.png"
        );
    }

    #[test]
    fn attachments_keep_in_the_drafts_under_a_key_no_row_has() {
        let files = vec![PathBuf::from("/tmp/a b.png"), PathBuf::from("/tmp/c.rs")];
        let key = attachments_key("claude:abc");
        assert_eq!(attachments_row(&key), Some("claude:abc"));
        assert_eq!(attachments_row("claude:abc"), None);
        assert_eq!(attachments_from(&attachments_text(&files)), files);
        assert!(attachments_text(&[]).is_empty());
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
