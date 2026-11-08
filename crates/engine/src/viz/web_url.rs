//! Which URL the Web tab opens for a session (ConsoleCore's `WebURLResolver`).
//!
//! Page URLs the transcript mentioned come first (newest first, one per origin, API paths cut back
//! to the origin, queries dropped because they carry tokens), and a server that is actually listening
//! beats one that is not. Then a launch config in `<cwd>/.claude/launch.json`, then one in
//! `~/.claude/launch.json` that mentions the folder. If nothing listens, the newest candidate still
//! comes back so the phone can say which server is down.

use super::url::Url;
use serde_json::Value;
use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

pub fn resolve(
    transcript_urls: &[String],
    cwd: &str,
    home: &Path,
    is_listening: &dyn Fn(u16) -> bool,
) -> Option<String> {
    let pages = page_candidates(transcript_urls);
    if let Some(live) = pages.iter().find(|p| is_live(p, is_listening)) {
        return Some(live.clone());
    }
    let launch = launch_urls(cwd, home);
    if let Some(live) = launch.iter().find(|p| is_live(p, is_listening)) {
        return Some(live.clone());
    }
    pages
        .into_iter()
        .next()
        .or_else(|| launch.into_iter().next())
}

/// Page URLs of servers that are listening right now, newest first, for the phone's suggestion chips.
pub fn live_candidates(
    transcript_urls: &[String],
    cwd: &str,
    home: &Path,
    is_listening: &dyn Fn(u16) -> bool,
    limit: usize,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    let all = page_candidates(transcript_urls)
        .into_iter()
        .chain(launch_urls(cwd, home));
    for text in all {
        let Some(url) = Url::parse(&text).filter(|u| is_listening(u.effective_port())) else {
            continue;
        };
        if !seen.insert(url.origin()) {
            continue;
        }
        result.push(text);
        if result.len() == limit {
            break;
        }
    }
    result
}

/// Newest first, one per origin. A path under /api/ (or an asset path) says nothing about the page,
/// so it becomes the origin; a query or fragment is dropped.
pub fn page_candidates(transcript_urls: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for text in transcript_urls.iter().rev() {
        let Some(mut url) = Url::parse_local(text) else {
            continue;
        };
        url.query = None;
        url.fragment = None;
        let path = url.path.as_str();
        if path.starts_with("/api")
            || path.contains("/api/")
            || path.starts_with("/_next")
            || path.starts_with("/static")
            || path.starts_with("/assets")
            || path.contains("/auth/")
            || path.starts_with("/auth")
        {
            url.path.clear();
        }
        if seen.insert(url.origin()) {
            result.push(url.render());
        }
    }
    result
}

fn is_live(text: &str, is_listening: &dyn Fn(u16) -> bool) -> bool {
    Url::parse(text).is_some_and(|u| is_listening(u.effective_port()))
}

fn launch_urls(cwd: &str, home: &Path) -> Vec<String> {
    if cwd.is_empty() {
        return Vec::new();
    }
    let project = Path::new(cwd).join(".claude/launch.json");
    let mut urls = from_launch_file(&project, None);
    // An empty home is "no home folder", never the current directory.
    if !home.as_os_str().is_empty() {
        urls.extend(from_launch_file(
            &home.join(".claude/launch.json"),
            Some(cwd),
        ));
    }
    urls
}

fn from_launch_file(file: &Path, matching_cwd: Option<&str>) -> Vec<String> {
    let Some(launch) = std::fs::read(file)
        .ok()
        .and_then(|data| serde_json::from_slice::<Value>(&data).ok())
    else {
        return Vec::new();
    };
    let mut configurations: Vec<&Value> = launch
        .get("configurations")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter(|c| c.is_object()).collect())
        .unwrap_or_default();
    if let Some(cwd) = matching_cwd {
        configurations.retain(|c| {
            c.get("cwd").and_then(Value::as_str) == Some(cwd)
                || c.get("runtimeArgs")
                    .and_then(Value::as_array)
                    .is_some_and(|args| args.iter().any(|a| a.as_str() == Some(cwd)))
        });
    }
    // A dev server is the one you look at while coding; production previews come second.
    let is_dev = |c: &Value| {
        c.get("name")
            .and_then(Value::as_str)
            .is_some_and(|n| n.contains("dev"))
    };
    configurations.sort_by_key(|c| !is_dev(c));
    configurations
        .into_iter()
        .filter_map(|c| {
            if let Some(url) = c.get("url").and_then(Value::as_str) {
                if Url::parse(url).is_some_and(|u| u.is_http()) {
                    return Some(url.to_string());
                }
            }
            let port = c.get("port").and_then(Value::as_i64)?;
            (1..=65_535)
                .contains(&port)
                .then(|| format!("http://localhost:{port}"))
        })
        .collect()
}

/// A cheap check that something accepts connections on a local port.
pub fn tcp_is_listening(port: u16) -> bool {
    if port == 0 {
        return false;
    }
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok()
}
