use notchling_locale as t;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::chat::html_preview::is_html;
use crate::chat::page_panel::Opens;

pub const MAX_TEXT: usize = 1024 * 1024;
pub const MAX_IMAGE: u64 = 25 * 1024 * 1024;
const SNIFF: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Markdown,
    Html,
    Text,
    Image(Pic),
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pic {
    Png,
    Jpeg,
    Gif,
    Webp,
    Bmp,
    Tiff,
    Svg,
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

pub fn pic(path: &Path) -> Option<Pic> {
    Some(match extension(path).as_str() {
        "png" => Pic::Png,
        "jpg" | "jpeg" => Pic::Jpeg,
        "gif" => Pic::Gif,
        "webp" => Pic::Webp,
        "bmp" => Pic::Bmp,
        "tif" | "tiff" => Pic::Tiff,
        "svg" => Pic::Svg,
        _ => return None,
    })
}

fn reads_as_text(head: &[u8]) -> bool {
    if head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none() && e.valid_up_to() + 4 > head.len(),
    }
}

pub fn kind(path: &Path, head: &[u8]) -> Kind {
    if let Some(pic) = pic(path) {
        return Kind::Image(pic);
    }
    if matches!(
        extension(path).as_str(),
        "md" | "markdown" | "mdown" | "mkd" | "mdx"
    ) {
        return Kind::Markdown;
    }
    match (reads_as_text(head), is_html(path)) {
        (true, true) => Kind::Html,
        (true, false) => Kind::Text,
        (false, _) => Kind::Other,
    }
}

fn head_of(path: &Path) -> Vec<u8> {
    let mut head = Vec::with_capacity(SNIFF);
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(SNIFF as u64).read_to_end(&mut head);
    }
    head
}

pub fn opens(path: &Path) -> Opens {
    if path.is_dir() {
        return Opens::Folder(path.to_path_buf());
    }
    let too_big = std::fs::metadata(path).is_ok_and(|m| pic(path).is_some() && m.len() > MAX_IMAGE);
    match kind(path, &head_of(path)) {
        Kind::Other => Opens::System(path.to_path_buf()),
        Kind::Image(_) if too_big => Opens::System(path.to_path_buf()),
        Kind::Html => Opens::Html(path.to_path_buf()),
        _ => Opens::File(path.to_path_buf()),
    }
}

pub type Stamp = Option<(Option<SystemTime>, u64)>;

pub fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok(), meta.len()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    Markdown { text: String, cut: bool },
    Text { text: String, cut: bool },
    Image { format: Pic, bytes: Vec<u8> },
    Other,
    Missing(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    pub stamp: Stamp,
    pub body: Body,
}

pub fn load(path: &Path) -> Loaded {
    let stamp = stamp(path);
    let missing = |why: &str| Loaded {
        stamp,
        body: Body::Missing(why.to_owned()),
    };
    if stamp.is_none() {
        return missing(t::chat::panel_file_gone());
    }
    let Ok(file) = std::fs::File::open(path) else {
        return missing(t::chat::panel_file_unreadable());
    };
    let mut bytes = Vec::new();
    let limit = if pic(path).is_some() {
        MAX_IMAGE + 1
    } else {
        MAX_TEXT as u64 + 1
    };
    if file.take(limit).read_to_end(&mut bytes).is_err() {
        return missing(t::chat::panel_file_unreadable());
    }
    let cut = pic(path).is_none() && bytes.len() > MAX_TEXT;
    let head_end = bytes.len().min(SNIFF);
    let body = match kind(path, &bytes[..head_end]) {
        Kind::Image(format) if bytes.len() as u64 <= MAX_IMAGE => Body::Image { format, bytes },
        Kind::Markdown | Kind::Html | Kind::Text if bytes.is_empty() => Body::Text {
            text: String::new(),
            cut: false,
        },
        kind @ (Kind::Markdown | Kind::Html | Kind::Text) => {
            bytes.truncate(MAX_TEXT);
            let text = String::from_utf8_lossy(&bytes)
                .trim_end_matches('\u{FFFD}')
                .to_owned();
            if kind == Kind::Markdown {
                Body::Markdown { text, cut }
            } else {
                Body::Text { text, cut }
            }
        }
        _ => Body::Other,
    };
    Loaded { stamp, body }
}

pub fn subtitle(path: &Path, home: Option<&Path>, body: &Body) -> String {
    let folder = path.parent().unwrap_or(Path::new("/"));
    let shown = match home.and_then(|h| folder.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.display()),
        None => folder.display().to_string(),
    };
    match body {
        Body::Markdown { cut: true, .. } | Body::Text { cut: true, .. } => {
            t::chat::panel_file_cut_subtitle(&shown)
        }
        _ => shown,
    }
}

pub fn title(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

pub fn folder_of(path: &Path) -> PathBuf {
    path.parent().unwrap_or(Path::new("/")).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Scratch;

    #[test]
    fn kinds_come_from_the_name_then_from_whether_it_reads_as_text() {
        let text = b"fn main() {}\n";
        assert_eq!(kind(Path::new("a/PLAN.MD"), text), Kind::Markdown);
        assert_eq!(kind(Path::new("a/x.png"), &[0, 1]), Kind::Image(Pic::Png));
        assert_eq!(kind(Path::new("a/x.JPG"), &[]), Kind::Image(Pic::Jpeg));
        assert_eq!(kind(Path::new("a/main.rs"), text), Kind::Text);
        assert_eq!(kind(Path::new("a/Makefile"), text), Kind::Text);
        assert_eq!(
            kind(Path::new("a/movie.mp4"), &[0, 0, 0, 0x20, b'f']),
            Kind::Other
        );
        assert_eq!(
            kind(Path::new("a/doc.pdf"), b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n"),
            Kind::Other
        );
        let cut = &"héllo wörld".as_bytes()[..2];
        assert_eq!(kind(Path::new("a/n.txt"), cut), Kind::Text);
        assert_eq!(kind(Path::new("a/empty"), &[]), Kind::Text);
    }

    #[test]
    fn a_click_opens_by_what_is_there() {
        let s = Scratch::new("filepanel-opens");
        let md = s.write("film/plan.md", "# Plan");
        let rs = s.write("film/main.rs", "fn main() {}");
        let png = s.write("film/shot.png", "not really a png");
        let bin = s.write("film/blob.bin", "");
        std::fs::write(&bin, [0u8, 1, 2, 0, 255]).unwrap();
        let folder = md.parent().unwrap().to_path_buf();
        assert_eq!(opens(&folder), Opens::Folder(folder));
        assert_eq!(opens(&md), Opens::File(md));
        assert_eq!(opens(&rs), Opens::File(rs));
        assert_eq!(opens(&png), Opens::File(png));
        assert_eq!(opens(&bin), Opens::System(bin));
    }

    #[test]
    fn markdown_and_text_load_and_a_big_file_is_cut_with_a_note() {
        let s = Scratch::new("filepanel-load");
        let md = s.write("plan.md", "# Plan\n\nBody");
        let loaded = load(&md);
        assert_eq!(
            loaded.body,
            Body::Markdown {
                text: "# Plan\n\nBody".into(),
                cut: false
            }
        );
        assert!(loaded.stamp.is_some());

        let big = s.write("big.log", &"line of text\n".repeat(MAX_TEXT / 8));
        match load(&big).body {
            Body::Text { text, cut } => {
                assert!(cut);
                assert!(text.len() <= MAX_TEXT);
                assert!(text.starts_with("line of text\n"));
            }
            other => panic!("{other:?}"),
        }

        let empty = s.write("empty.txt", "");
        assert_eq!(
            load(&empty).body,
            Body::Text {
                text: String::new(),
                cut: false
            }
        );
    }

    #[test]
    fn a_picture_loads_its_bytes_and_a_gone_file_says_so() {
        let s = Scratch::new("filepanel-pic");
        let png = s.write("a.png", "bytes");
        assert_eq!(
            load(&png).body,
            Body::Image {
                format: Pic::Png,
                bytes: b"bytes".to_vec()
            }
        );
        let gone = s.path().join("gone.md");
        let loaded = load(&gone);
        assert_eq!(loaded.stamp, None);
        assert_eq!(loaded.body, Body::Missing("This file is gone.".into()));
        let blob = s.path().join("blob.bin");
        std::fs::write(&blob, [0u8, 9, 0]).unwrap();
        assert_eq!(load(&blob).body, Body::Other);
    }

    #[test]
    fn the_stamp_changes_when_the_file_does() {
        let s = Scratch::new("filepanel-stamp");
        let f = s.write("a.md", "one");
        let before = stamp(&f);
        std::fs::write(&f, "one and more").unwrap();
        assert_ne!(stamp(&f), before, "the size changed");
        std::fs::remove_file(&f).unwrap();
        assert_eq!(stamp(&f), None);
    }

    #[test]
    fn the_header_names_the_file_and_its_folder() {
        let home = Path::new("/Users/me");
        let path = Path::new("/Users/me/film/plan/plan.md");
        let body = Body::Markdown {
            text: String::new(),
            cut: false,
        };
        assert_eq!(title(path), "plan.md");
        assert_eq!(subtitle(path, Some(home), &body), "~/film/plan");
        assert_eq!(
            subtitle(Path::new("/Users/me/a.md"), Some(home), &body),
            "~"
        );
        assert_eq!(subtitle(Path::new("/tmp/a.md"), Some(home), &body), "/tmp");
        let cut = Body::Text {
            text: String::new(),
            cut: true,
        };
        assert_eq!(
            subtitle(Path::new("/tmp/a.log"), None, &cut),
            "/tmp · first 1 MB shown"
        );
    }
}
