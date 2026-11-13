use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageSource {
    File { file: PathBuf, folder: PathBuf },
    Html(String),
}

pub const GONE: &str = "The local copy is gone.";

pub const OUTSIDE: &str = "That page's copy isn't in the pages folder.";

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn wrap_markdown(text: &str) -> String {
    format!(
        "<!doctype html><meta charset=\"utf-8\"><style>:root{{color-scheme:light dark}}\
         pre{{margin:24px;white-space:pre-wrap;font:13px/1.5 ui-monospace,monospace}}</style>\
         <pre>{}</pre>",
        escape(text)
    )
}

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

pub fn inside(pages_dir: &Path, path: &Path) -> Result<PathBuf, String> {
    let root = std::fs::canonicalize(pages_dir).map_err(|_| OUTSIDE.to_owned())?;
    let found = std::fs::canonicalize(path).map_err(|_| GONE.to_owned())?;
    if found.starts_with(&root) && found.parent().is_some_and(|dir| dir != root) {
        Ok(found)
    } else {
        Err(OUTSIDE.to_owned())
    }
}

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
        assert!(!go(&format!("{base}/other/index.html")));
        assert!(!go(&format!("{base}/abcd/index.html")));
        assert!(!go("file:///etc/passwd"));
        assert!(!go("file:///Users/me/.claude.json"));
        assert!(!go(&format!("{base}/abc/../other/index.html")));
        assert!(!go(&format!("{base}/abc/%2e%2e/%2e%2e/vault.json")));
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
        let climbed = pages.join("abc/../abc/index.html");
        assert_eq!(inside(&pages, &climbed), Ok(canonical));
        let elsewhere = scratch.write("elsewhere/index.html", "x");
        assert_eq!(inside(&pages, &elsewhere), Err(OUTSIDE.to_owned()));
        let sibling = scratch.write("pages-old/abc/index.html", "x");
        assert_eq!(inside(&pages, &sibling), Err(OUTSIDE.to_owned()));
        let out = pages.join("abc/../../elsewhere/index.html");
        assert_eq!(inside(&pages, &out), Err(OUTSIDE.to_owned()));
        let list = scratch.write("pages/pages.json", "{}");
        assert_eq!(inside(&pages, &list), Err(OUTSIDE.to_owned()));
        assert_eq!(
            inside(&pages, &pages.join("abc/none.html")),
            Err(GONE.to_owned())
        );
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
