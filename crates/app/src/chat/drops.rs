use std::path::{Path, PathBuf};

use notchling_kit::text_edit::TextEdit;

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

pub const KEEP_FOR_MS: i64 = 7 * 24 * 3600 * 1000;

pub fn passing(path: &Path) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| s.starts_with("NSIRD_screencaptureui_"))
    })
}

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

pub fn paths_text(paths: &[impl AsRef<Path>]) -> String {
    paths
        .iter()
        .map(|p| quoted(p.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

const IMAGE_KINDS: [&str; 8] = [
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".heic", ".tiff", ".bmp",
];

pub fn is_image(path: &Path) -> bool {
    let lower = path.to_string_lossy().to_lowercase();
    IMAGE_KINDS.iter().any(|k| lower.ends_with(k))
}

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
    let mut from: Option<usize> = None;
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

pub fn image_words(text: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for (_, path) in image_spans(text) {
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

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
    if !pictures.is_empty() {
        words = one_blank_line_at_most(&words);
    }
    (words.trim().to_owned(), pictures)
}

fn one_blank_line_at_most(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blanks = 0;
    for (i, line) in text.split('\n').enumerate() {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

pub fn with_attachments(text: &str, files: &[PathBuf]) -> String {
    let text = text.trim();
    let paths = paths_text(files);
    match (text.is_empty(), paths.is_empty()) {
        (_, true) => text.to_owned(),
        (true, false) => paths,
        (false, false) => format!("{text}\n\n{paths}"),
    }
}

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

pub fn attachments_key(row: &str) -> String {
    format!("{row}\u{1f}files")
}

pub fn attachments_row(key: &str) -> Option<&str> {
    key.strip_suffix("\u{1f}files")
}

pub fn attachments_text(files: &[PathBuf]) -> String {
    files
        .iter()
        .map(|f| f.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn attachments_from(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

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
        assert_eq!(at("out/S1 a.png:12"), Some(dir.join("out/S1 a.png")));
        assert_eq!(at("out/S1 a.png#L3"), Some(dir.join("out/S1 a.png")));
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
    fn a_picture_in_the_middle_of_held_messages_leaves_one_blank_line_not_four() {
        let held = "padding bottom\n\n/tmp/a.png\n\nhi\n\nlook\n\n/tmp/b.png";
        let (words, pictures) = split_images(held, |_| true);
        assert_eq!(words, "padding bottom\n\nhi\n\nlook");
        assert_eq!(pictures.len(), 2);
        let (words, _) = split_images("a\n/tmp/a.png\n \n\n/tmp/b.png\nb", |_| true);
        assert_eq!(words, "a\n\nb");
    }

    #[test]
    fn text_with_no_picture_keeps_every_blank_line() {
        let text = "a\n\n\n\nb /tmp/gone.png";
        let (words, pictures) = split_images(text, |p| p != Path::new("/tmp/gone.png"));
        assert_eq!(words, text);
        assert!(pictures.is_empty());
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
        std::fs::remove_dir_all(&temp).unwrap();
        assert!(kept[0].exists());
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::write(&shot, b"second").unwrap();
        let again = keep(std::slice::from_ref(&shot), &store, 2_000);
        assert_ne!(again[0], kept[0]);
        assert_eq!(std::fs::read(&again[0]).unwrap(), b"second");
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
        let mut s = TextEdit::new("see ");
        insert_paths(&mut s, &[Path::new("/x")]);
        assert_eq!(s.text(), "see /x ");
        let mut empty = TextEdit::new("");
        insert_paths(&mut empty, &[Path::new("/x y"), Path::new("/z")]);
        assert_eq!(empty.text(), "'/x y' /z ");
        let mut mid = TextEdit::new("fix THIS please");
        mid.set_selection(4, 8);
        insert_paths(&mut mid, &[Path::new("/f.rs")]);
        assert_eq!(mid.text(), "fix /f.rs  please");
        assert_eq!(mid.caret(), 10);
        let mut none = TextEdit::new("a");
        let nothing: [PathBuf; 0] = [];
        assert!(!insert_paths(&mut none, &nothing));
        assert_eq!(none.text(), "a");
        assert!(mid.undo());
        assert_eq!(mid.text(), "fix THIS please");
    }
}
