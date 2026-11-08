//! A page's local copy as the web view loads it: an HTML copy by its `file:` URL, a Markdown one
//! wrapped in a `<pre>` the way the Swift `PagePreview` did. Pure: the web view (`web`, macOS)
//! only loads what this gives it.

use std::path::Path;

/// What the web view shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageSource {
    /// A `file:` URL to load.
    Url(String),
    /// A page's HTML, given whole.
    Html(String),
}

/// What is said when the local copy isn't there any more.
pub const GONE: &str = "The local copy is gone.";

/// Whether a byte goes into a URL path as itself (RFC 3986's unreserved set, plus `/`).
fn plain(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/')
}

/// `file://` and the path, each other byte percent-encoded, so a space or a `#` in a folder name
/// reaches the web view as part of the path.
pub fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for byte in path.to_string_lossy().as_bytes() {
        if plain(*byte) {
            url.push(char::from(*byte));
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    url
}

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

/// What to load for the copy at `path`: a Markdown file wrapped, anything else by its URL. `Err`
/// when the file isn't there.
pub fn source_for(path: &Path) -> Result<PageSource, String> {
    if !path.is_file() {
        return Err(GONE.to_owned());
    }
    if path.extension().and_then(|e| e.to_str()) == Some("md") {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(PageSource::Html(wrap_markdown(&text)));
    }
    Ok(PageSource::Url(file_url(path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Scratch;

    #[test]
    fn a_file_url_keeps_plain_bytes_and_encodes_the_rest() {
        assert_eq!(
            file_url(Path::new("/Users/me/Library/Application Support/Notchling/pages/a-b_c.d~/index.html")),
            "file:///Users/me/Library/Application%20Support/Notchling/pages/a-b_c.d~/index.html"
        );
        assert_eq!(file_url(Path::new("/p/#1?/é.html")), "file:///p/%231%3F/%C3%A9.html");
    }

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
        assert_eq!(source_for(&html), Ok(PageSource::Url(file_url(&html))));
        let md = scratch.write("two/page.md", "# Notes");
        assert_eq!(
            source_for(&md),
            Ok(PageSource::Html(wrap_markdown("# Notes")))
        );
        assert_eq!(
            source_for(&scratch.path().join("three/index.html")),
            Err(GONE.to_owned())
        );
        assert_eq!(source_for(scratch.path()), Err(GONE.to_owned()), "a folder isn't a copy");
    }
}
