//! What a published page is made of: its title, the skeleton that lets a fragment render from
//! disk, the folder name, and the facts read out of the Artifact tool's result. All pure; ported
//! from the Swift app's `PageLibrary` (page content, folders, the result's link and version).

use crate::search::lowercase;
use regex::Regex;
use std::sync::OnceLock;

/// A pattern compiled once; `None` if it ever failed to compile (it can't for these constants).
fn re(cell: &'static OnceLock<Option<Regex>>, pattern: &str) -> Option<&'static Regex> {
    cell.get_or_init(|| Regex::new(pattern).ok()).as_ref()
}

/// The title of the page's source: `<title>` for HTML, the first `# ` heading for Markdown, else
/// `fallback`. Whitespace collapses to single spaces and the title is cut to 200 characters.
pub fn page_title(data: &[u8], ext: &str, fallback: &str) -> String {
    let head = &data[..data.len().min(200_000)];
    let text = String::from_utf8_lossy(head);
    let found = if ext == "md" {
        text.split('\n')
            .filter(|l| !l.is_empty())
            .find_map(|l| l.strip_prefix("# "))
            .map(str::to_string)
    } else {
        static TITLE: OnceLock<Option<Regex>> = OnceLock::new();
        static OPEN: OnceLock<Option<Regex>> = OnceLock::new();
        static CLOSE: OnceLock<Option<Regex>> = OnceLock::new();
        re(&TITLE, r"(?is)<title[^>]*>.*?</title>")
            .and_then(|r| r.find(&text))
            .map(|m| {
                let mut inner = m.as_str().to_string();
                if let Some(r) = re(&OPEN, r"(?i)^<title[^>]*>") {
                    inner = r.replace(&inner, "").into_owned();
                }
                if let Some(r) = re(&CLOSE, r"(?i)</title>$") {
                    inner = r.replace(&inner, "").into_owned();
                }
                unescape(&inner)
            })
    };
    let clean = found
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if clean.is_empty() {
        fallback.to_string()
    } else {
        clean.chars().take(200).collect()
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// HTML-escapes text for the list page.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Published pages are fragments (the publisher adds a skeleton), so give them one to render from
/// disk. A page that already starts with a doctype is kept as it is.
pub fn wrapped(data: &[u8]) -> Vec<u8> {
    let head = String::from_utf8_lossy(&data[..data.len().min(2048)]).to_lowercase();
    if head.contains("<!doctype") {
        return data.to_vec();
    }
    let mut out = br#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<style>body{margin:0;font:14px system-ui,sans-serif}</style></head><body>
"#
    .to_vec();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\n</body></html>\n");
    out
}

/// A folder name for a new page: the title as a lowercase ASCII slug (40 characters at most), then
/// the FNV-1a hash of `key` (the link, or the source path when there is none) in 8 hex digits.
pub fn make_folder(title: &str, key: &str) -> String {
    let dashed: String = lowercase(title)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let mut slug: String = dashed
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(40)
        .collect();
    if slug.is_empty() {
        slug = "page".to_string();
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{slug}-{:08x}", hash as u32)
}

/// The first claude.ai artifact link in a tool result, whatever shape the result has.
pub fn artifact_url_in(text: &str) -> Option<String> {
    static URL: OnceLock<Option<Regex>> = OnceLock::new();
    re(&URL, r"https://claude\.ai/(?:code/)?artifact/[A-Za-z0-9_-]+")
        .and_then(|r| r.find(text))
        .map(|m| m.as_str().to_string())
}

/// The version id a publish result names: Claude Code's "version id 1793506664-65e3", or
/// Notchling's own "(version 1793506664-65e3)". Empty when there is none.
pub fn published_version(text: &str) -> String {
    static VERSION: OnceLock<Option<Regex>> = OnceLock::new();
    re(&VERSION, r"version (?:id )?[0-9]{6,}-[0-9a-f]{2,}")
        .and_then(|r| r.find(text))
        .and_then(|m| m.as_str().split(' ').next_back())
        .unwrap_or("")
        .to_string()
}

/// True when a result with no link reads as a refused or failed publish.
pub fn looks_failed(response: &str) -> bool {
    static FAILED: OnceLock<Option<Regex>> = OnceLock::new();
    re(&FAILED, r"(?i)refused|error|failed|conflict").is_some_and(|r| r.is_match(response))
}

/// True when a short result reads as "not found" or "not allowed": a read that failed without
/// saying so.
pub fn looks_missing(response: &str) -> bool {
    static MISSING: OnceLock<Option<Regex>> = OnceLock::new();
    response.chars().count() < 2000
        && re(
            &MISSING,
            r"(?i)404|not found|no access|not allowed|forbidden",
        )
        .is_some_and(|r| r.is_match(response))
}

/// The project's folder name, also from a subagent's `.claude/worktrees/<name>` checkout.
pub fn project_name(cwd: &str) -> String {
    let root = cwd.find("/.claude/worktrees/").map_or(cwd, |i| &cwd[..i]);
    root.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}
