//! `index.html` in the library: a plain list of everything, regenerated after each save, so the
//! saved pages open from disk whatever account is active.

use super::clock;
use super::content::escape;
use super::library::{PageLibrary, PagesError};
use crate::search::PageEntry;

/// Writes the list page for `list`.
pub(crate) fn write(lib: &PageLibrary, list: &[PageEntry]) -> Result<(), PagesError> {
    notchling_platform::files::write_private_no_follow(&lib.index_path(), html(list).as_bytes())?;
    Ok(())
}

/// The list page's text, newest first.
pub fn html(list: &[PageEntry]) -> String {
    let mut sorted: Vec<&PageEntry> = list.iter().collect();
    sorted.sort_by(|a, b| {
        b.updated
            .partial_cmp(&a.updated)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut rows = String::new();
    for e in sorted {
        let href = format!("{}/{}", percent_encode(&e.folder), e.file);
        let online = if e.url.starts_with("https://claude.ai/") {
            format!("<a href=\"{}\">claude.ai</a>", escape(&e.url))
        } else {
            String::new()
        };
        rows.push_str(&format!(
            "<tr><td><a href=\"{}\">{}</a></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            escape(&href),
            escape(&e.title),
            escape(&e.project),
            escape(&e.account),
            clock::local_text_minutes(e.updated),
            online
        ));
    }
    format!(
        "<!doctype html>\n\
<html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n\
<title>Notchling Pages</title>\n\
<style>\n\
:root{{color-scheme:light dark}}\n\
body{{margin:24px;font:14px system-ui,sans-serif}}\n\
table{{border-collapse:collapse;width:100%}}\n\
th,td{{text-align:left;padding:6px 12px 6px 0;border-bottom:1px solid #8884;vertical-align:top}}\n\
</style></head><body>\n\
<h1>Pages</h1>\n\
<p>{} saved, newest first. Private on claude.ai to the account that published them; these copies open whatever account is active.</p>\n\
<table><tr><th>Title</th><th>Project</th><th>Account</th><th>Saved</th><th>Online</th></tr>\n\
{}</table>\n\
</body></html>\n",
        list.len(),
        rows
    )
}

/// Percent-encodes everything but the characters a URL path allows unescaped.
fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
