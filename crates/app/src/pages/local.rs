//! A page's local copy as the web view loads it: an HTML copy as a file, with read access to its
//! own folder and nothing else, a Markdown one wrapped in a `<pre>` the way the Swift
//! `PagePreview` did. Pure: the web view (`web`, macOS) only loads what this gives it.

use std::path::{Component, Path, PathBuf};

/// What the web view shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageSource {
    /// A copy to load as a file. The page may read what is in `folder` (its own pictures and
    /// scripts) and nothing outside it, and may go nowhere else ([`stays_local`]).
    File { file: PathBuf, folder: PathBuf },
    /// A page's HTML, given whole.
    Html(String),
}

/// What is said when the local copy isn't there any more.
pub const GONE: &str = "The local copy is gone.";

/// What is said when a page's copy is outside the library.
pub const OUTSIDE: &str = "That page's copy isn't in the pages folder.";

/// Text made safe to put inside an HTML element (the Swift `PageLibrary.escape`).
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A Markdown copy, shown as its source in a monospaced block that follows the system's appearance.
pub fn wrap_markdown(text: &str) -> String {
    format!(
        "<!doctype html><meta charset=\"utf-8\"><style>:root{{color-scheme:light dark}}\
         pre{{margin:24px;white-space:pre-wrap;font:13px/1.5 ui-monospace,monospace}}</style>\
         <pre>{}</pre>",
        escape(text)
    )
}

/// What to load for the copy at `path`: a Markdown file wrapped, anything else as a file with its
/// folder readable. `Err` when the file isn't there.
pub fn source_for(path: &Path) -> Result<PageSource, String> {
    if !path.is_file() {
        return Err(GONE.to_owned());
    }
    if path.extension().and_then(|e| e.to_str()) == Some("md") {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(PageSource::Html(wrap_markdown(&text)));
    }
    let folder = path.parent().ok_or(GONE)?.to_path_buf();
    Ok(PageSource::File {
        file: path.to_path_buf(),
        folder,
    })
}

/// `path` as the library's own copy: it has to be in a folder of `pages_dir` once links are
/// followed. The daemon names the path, and the web view is given read access to its folder, so a
/// path that wandered out (a `..`, a link to somewhere else) must not get that far. The path
/// returned has its links resolved. `Err` when it is outside or missing.
pub fn inside(pages_dir: &Path, path: &Path) -> Result<PathBuf, String> {
    let root = std::fs::canonicalize(pages_dir).map_err(|_| OUTSIDE.to_owned())?;
    let found = std::fs::canonicalize(path).map_err(|_| GONE.to_owned())?;
    // A copy is in a folder of the library, never in its root (`pages.json` lives there, and the
    // root is every page's folder at once).
    if found.starts_with(&root) && found.parent().is_some_and(|dir| dir != root) {
        Ok(found)
    } else {
        Err(OUTSIDE.to_owned())
    }
}

/// A percent-encoded URL path as text; `None` when it isn't valid UTF-8 or holds a NUL.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let text = String::from_utf8(out).ok()?;
    (!text.contains('\0')).then_some(text)
}

/// Whether a navigation stays on the local copy: `about:` (a blank page), or a `file:` URL for a
/// path inside `folder`, the folder the page was loaded from. Nothing else: not another folder,
/// not another site, not a script URL. `folder` is `None` for a page given as HTML, which has no
/// files of its own.
pub fn stays_local(url: &str, folder: Option<&Path>) -> bool {
    let scheme = |name: &str| {
        url.get(..name.len())
            .is_some_and(|s| s.eq_ignore_ascii_case(name))
    };
    if scheme("about:") {
        return true;
    }
    let (true, Some(folder)) = (scheme("file:"), folder) else {
        return false;
    };
    // `file:///path` or `file://localhost/path`: an empty host, or this machine's.
    let Some(rest) = url["file:".len()..].strip_prefix("//") else {
        return false;
    };
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') {
        return false;
    }
    let path = path.split(['?', '#']).next().unwrap_or("");
    let Some(path) = percent_decode(path) else {
        return false;
    };
    let path = Path::new(&path);
    path.components()
        .all(|c| !matches!(c, Component::ParentDir))
        && path.starts_with(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Scratch;

    #[test]
    fn markdown_is_shown_as_escaped_source() {
        assert_eq!(escape("a < b & c > d"), "a &lt; b &amp; c &gt; d");
        let html = wrap_markdown("# Title\n<script>x</script>");
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<pre># Title\n&lt;script&gt;x&lt;/script&gt;</pre>"));
        assert!(html.contains("color-scheme:light dark"));
    }

    #[test]
    fn the_source_follows_the_file_and_says_when_it_is_gone() {
        let scratch = Scratch::new("pages-local");
        let html = scratch.write("one/index.html", "<p>hi</p>");
        assert_eq!(
            source_for(&html),
            Ok(PageSource::File {
                file: html.clone(),
                folder: scratch.path().join("one")
            })
        );
        let md = scratch.write("two/page.md", "# Notes");
        assert_eq!(
            source_for(&md),
            Ok(PageSource::Html(wrap_markdown("# Notes")))
        );
        assert_eq!(
            source_for(&scratch.path().join("three/index.html")),
            Err(GONE.to_owned())
        );
        assert_eq!(
            source_for(scratch.path()),
            Err(GONE.to_owned()),
            "a folder isn't a copy"
        );
    }

    #[test]
    fn only_the_pages_own_folder_is_navigated_to() {
        let folder = Path::new("/Users/me/Library/Application Support/Notchling/pages/abc");
        let go = |url: &str| stays_local(url, Some(folder));
        let base = "file:///Users/me/Library/Application%20Support/Notchling/pages";
        assert!(go(&format!("{base}/abc/index.html")));
        assert!(go(&format!("{base}/abc/img/a.png?x=1#top")));
        assert!(go(
            "FILE://localhost/Users/me/Library/Application%20Support/Notchling/pages/abc/b.html"
        ));
        assert!(go("about:blank"));
        // Another page's folder, a sibling whose name starts the same, and the rest of the disk.
        assert!(!go(&format!("{base}/other/index.html")));
        assert!(!go(&format!("{base}/abcd/index.html")));
        assert!(!go("file:///etc/passwd"));
        assert!(!go("file:///Users/me/.claude.json"));
        // A way back out, plain and encoded.
        assert!(!go(&format!("{base}/abc/../other/index.html")));
        assert!(!go(&format!("{base}/abc/%2e%2e/%2e%2e/vault.json")));
        // Other hosts and schemes, and shapes that aren't a file URL at all.
        assert!(!go(
            "file://evil.example/Users/me/Library/Application%20Support/Notchling/pages/abc/x"
        ));
        assert!(!go(
            "file:Users/me/Library/Application%20Support/Notchling/pages/abc/x"
        ));
        assert!(!go("https://claude.ai/artifact/x"));
        assert!(!go("http://example.com"));
        assert!(!go("javascript:alert(1)"));
        assert!(!go("data:text/html,hi"));
        assert!(!go(""));
        assert!(!go("file:///Users/me/%zz"));
        assert!(!go(&format!("{base}/abc/%00")));
        // A page given as HTML has no folder: only a blank page.
        assert!(stays_local("about:blank", None));
        assert!(!stays_local("file:///etc/passwd", None));
    }

    #[test]
    fn a_copy_must_be_in_a_folder_of_the_library() {
        let scratch = Scratch::new("pages-inside");
        let pages = scratch.path().join("pages");
        let page = scratch.write("pages/abc/index.html", "<p>hi</p>");
        let canonical = std::fs::canonicalize(&page).expect("a path");
        assert_eq!(inside(&pages, &page), Ok(canonical.clone()));
        // The same file by a path that climbs out and back in.
        let climbed = pages.join("abc/../abc/index.html");
        assert_eq!(inside(&pages, &climbed), Ok(canonical));
        // A file somewhere else, and one beside the library.
        let elsewhere = scratch.write("elsewhere/index.html", "x");
        assert_eq!(inside(&pages, &elsewhere), Err(OUTSIDE.to_owned()));
        let sibling = scratch.write("pages-old/abc/index.html", "x");
        assert_eq!(inside(&pages, &sibling), Err(OUTSIDE.to_owned()));
        // A climb out of the library.
        let out = pages.join("abc/../../elsewhere/index.html");
        assert_eq!(inside(&pages, &out), Err(OUTSIDE.to_owned()));
        // The library's own file, not in a page folder (it would give read access to all of them).
        let list = scratch.write("pages/pages.json", "{}");
        assert_eq!(inside(&pages, &list), Err(OUTSIDE.to_owned()));
        // Gone.
        assert_eq!(
            inside(&pages, &pages.join("abc/none.html")),
            Err(GONE.to_owned())
        );
        // A library folder that isn't there.
        assert_eq!(
            inside(&scratch.path().join("no-pages"), &page),
            Err(OUTSIDE.to_owned())
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_library_is_followed_before_it_is_trusted() {
        let scratch = Scratch::new("pages-link");
        let pages = scratch.path().join("pages");
        scratch.write("secret/vault.json", "{}");
        std::fs::create_dir_all(pages.join("abc")).expect("a folder");
        std::os::unix::fs::symlink(scratch.path().join("secret"), pages.join("abc/linked"))
            .expect("a link");
        let through = pages.join("abc/linked/vault.json");
        assert_eq!(inside(&pages, &through), Err(OUTSIDE.to_owned()));
    }
}
