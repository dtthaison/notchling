use std::path::PathBuf;

use crate::pages::dates;
use crate::search::wire::Page;

pub const DEFAULT_WIDTH: f32 = 520.0;
pub const MIN_WIDTH: f32 = 320.0;
pub const LEAVES: f32 = 380.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactLink {
    pub id: String,
    pub code: bool,
}

impl ArtifactLink {
    pub fn live_url(&self) -> String {
        let code = if self.code { "code/" } else { "" };
        format!("https://claude.ai/{code}artifact/{}", self.id)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Opens {
    Saved(Box<Page>),
    Live(ArtifactLink),
    File(PathBuf),
    Diff(String),
    Folder(PathBuf),
    System(PathBuf),
    Browser,
}

fn is_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

pub fn artifact_link(url: &str) -> Option<ArtifactLink> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let after_scheme = ["https://", "http://"]
        .iter()
        .find_map(|s| lower.strip_prefix(s).map(|_| &url[s.len()..]))?;
    let (host, path) = after_scheme.split_at(after_scheme.find('/')?);
    if !matches!(
        host.to_ascii_lowercase().as_str(),
        "claude.ai" | "www.claude.ai"
    ) {
        return None;
    }
    let (rest, code) = match path.strip_prefix("/code/artifact/") {
        Some(rest) => (rest, true),
        None => (path.strip_prefix("/artifact/")?, false),
    };
    let id: String = rest.chars().take_while(|&c| is_id_char(c)).collect();
    let tail = &rest[id.len()..];
    if id.is_empty() || !(tail.is_empty() || tail.starts_with(['/', '?', '#'])) {
        return None;
    }
    Some(ArtifactLink { id, code })
}

const BASE58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

pub fn uuid_from_short(id: &str) -> Option<String> {
    if id.is_empty() || id.len() > 22 {
        return None;
    }
    let mut value: u128 = 0;
    for byte in id.bytes() {
        let digit = BASE58.iter().position(|&b| b == byte)? as u128;
        value = value.checked_mul(58)?.checked_add(digit)?;
    }
    let hex = format!("{value:032x}");
    Some(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

fn as_uuid(id: &str) -> Option<String> {
    let is_uuid = id.len() == 36
        && id.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
    if is_uuid {
        Some(id.to_ascii_lowercase())
    } else {
        uuid_from_short(id)
    }
}

pub fn same_artifact(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || as_uuid(a).is_some_and(|u| Some(u) == as_uuid(b))
}

pub fn decide(url: &str, pages: &[Page]) -> Opens {
    let Some(link) = artifact_link(url) else {
        return Opens::Browser;
    };
    let saved = pages
        .iter()
        .filter(|p| !p.path.is_empty() && same_artifact(&p.id, &link.id))
        .max_by(|a, b| a.updated_secs.total_cmp(&b.updated_secs));
    match saved {
        Some(page) => Opens::Saved(Box::new(page.clone())),
        None => Opens::Live(link),
    }
}

pub fn subtitle(opens: &Opens) -> String {
    match opens {
        Opens::Saved(page) if page.updated_secs > 0.0 => {
            format!("Saved copy · {}", dates::short(page.updated_secs))
        }
        Opens::Saved(_) => "Saved copy".to_owned(),
        Opens::Live(_) => "Live on claude.ai".to_owned(),
        Opens::File(_) | Opens::Diff(_) | Opens::Folder(_) | Opens::System(_) | Opens::Browser => {
            String::new()
        }
    }
}

fn live_host(host: &str) -> bool {
    let under = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    under("claude.ai") || under("claudeusercontent.com") || host == "challenges.cloudflare.com"
}

pub fn live_allows(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("about:") {
        return true;
    }
    let lower = lower.strip_prefix("blob:").unwrap_or(&lower);
    let Some(rest) = lower.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => authority,
    };
    !host.is_empty() && live_host(host)
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Covers {
    pub sheet: bool,
    pub phone: bool,
    pub inbox: bool,
    pub parts: bool,
}

pub fn page_covered(covers: Covers) -> bool {
    covers.sheet || covers.phone || covers.inbox || covers.parts
}

pub fn opens_in_browser(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub fn appkit_frame(bounds: Frame, parent_height: f64, flipped: bool, scale: f64) -> Frame {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let snap = |v: f64| (v * scale).round() / scale;
    let (left, right) = (snap(bounds.x), snap(bounds.x + bounds.width.max(0.0)));
    let (top, bottom) = (snap(bounds.y), snap(bounds.y + bounds.height.max(0.0)));
    let y = if flipped {
        top
    } else {
        snap(parent_height) - bottom
    };
    Frame {
        x: left,
        y,
        width: right - left,
        height: bottom - top,
    }
}

pub fn panel_width(wanted: f32, available: f32) -> f32 {
    let most = (available - LEAVES).max(MIN_WIDTH);
    wanted.clamp(MIN_WIDTH, most)
}

pub fn dragged_width(pointer_x: f32, area_left: f32, area_right: f32) -> f32 {
    panel_width(area_right - pointer_x, area_right - area_left)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_what_is_drawn_over_the_panel_hides_its_page() {
        assert!(!page_covered(Covers::default()));
        for covers in [
            Covers {
                sheet: true,
                ..Covers::default()
            },
            Covers {
                phone: true,
                ..Covers::default()
            },
            Covers {
                inbox: true,
                ..Covers::default()
            },
            Covers {
                parts: true,
                ..Covers::default()
            },
        ] {
            assert!(page_covered(covers));
        }
    }

    fn page(id: &str, path: &str, updated: f64) -> Page {
        Page {
            id: id.into(),
            title: format!("Page {id}"),
            folder: format!("folder-{id}-{updated}"),
            link: format!("https://claude.ai/artifact/{id}"),
            path: path.into(),
            updated_secs: updated,
            ..Page::default()
        }
    }

    #[test]
    fn both_link_forms_give_their_id() {
        let short = artifact_link("https://claude.ai/artifact/KVBHDXYvY8f1b1AZG6PbAR").unwrap();
        assert_eq!(short.id, "KVBHDXYvY8f1b1AZG6PbAR");
        assert!(!short.code);
        let code = artifact_link(
            "https://claude.ai/code/artifact/95b3a80d-bd70-429a-b800-d1089310408a?tab=1#x",
        )
        .unwrap();
        assert_eq!(code.id, "95b3a80d-bd70-429a-b800-d1089310408a");
        assert!(code.code);
        for url in [
            "HTTPS://Claude.AI/artifact/abc123/",
            "https://www.claude.ai/artifact/abc123?x=1",
            "http://claude.ai/artifact/abc123#top",
            "  https://claude.ai/artifact/abc123  ",
        ] {
            assert_eq!(
                artifact_link(url).map(|l| l.id),
                Some("abc123".into()),
                "{url}"
            );
        }
        assert_eq!(
            artifact_link("http://www.claude.ai/artifact/abc123?evil=1")
                .unwrap()
                .live_url(),
            "https://claude.ai/artifact/abc123"
        );
        assert_eq!(
            code.live_url(),
            "https://claude.ai/code/artifact/95b3a80d-bd70-429a-b800-d1089310408a"
        );
    }

    #[test]
    fn other_links_are_not_artifacts() {
        for url in [
            "https://claude.ai/chat/abc",
            "https://claude.ai/artifact/",
            "https://claude.ai/artifacts/abc",
            "https://claude.ai/code/abc",
            "https://claude.ai.evil.example/artifact/abc",
            "https://evil.example/claude.ai/artifact/abc",
            "https://user@claude.ai/artifact/abc",
            "https://claude.ai:8443/artifact/abc",
            "https://claude.ai/artifact/abc%2F..",
            "https://claude.ai/artifact/abc.html",
            "ftp://claude.ai/artifact/abc",
            "claude.ai/artifact/abc",
            "javascript:alert('https://claude.ai/artifact/abc')",
            "",
        ] {
            assert_eq!(artifact_link(url), None, "{url}");
        }
    }

    #[test]
    fn a_short_id_is_its_uuid_in_base58() {
        assert_eq!(
            uuid_from_short("KVBHDXYvY8f1b1AZG6PbAR").as_deref(),
            Some("95b3a80d-bd70-429a-b800-d1089310408a")
        );
        assert_eq!(
            uuid_from_short("2PZDwnLbEHYhvkf9Y6uRnB").as_deref(),
            Some("0b3f5c1e-8a2d-4e6f-9c7b-1d2e3f4a5b6c")
        );
        assert_eq!(
            uuid_from_short("YcVfxkQb6JRzqk5kF2tNLv").as_deref(),
            Some("ffffffff-ffff-ffff-ffff-ffffffffffff")
        );
        assert_eq!(uuid_from_short("0VBHDXYvY8f1b1AZG6PbAR"), None);
        assert_eq!(uuid_from_short("zzzzzzzzzzzzzzzzzzzzzz"), None);
        assert_eq!(uuid_from_short("KVBHDXYvY8f1b1AZG6PbARx"), None);
        assert_eq!(uuid_from_short(""), None);
        assert!(same_artifact(
            "KVBHDXYvY8f1b1AZG6PbAR",
            "95B3A80D-BD70-429A-B800-D1089310408A"
        ));
        assert!(same_artifact("abc", "abc"));
        assert!(!same_artifact("abc", "ABC"), "a short id is case-sensitive");
        assert!(!same_artifact("", ""));
        assert!(!same_artifact(
            "KVBHDXYvY8f1b1AZG6PbAR",
            "0b3f5c1e-8a2d-4e6f-9c7b-1d2e3f4a5b6c"
        ));
    }

    #[test]
    fn a_saved_artifact_opens_its_copy_and_any_other_the_live_page() {
        let pages = vec![
            page("KVBHDXYvY8f1b1AZG6PbAR", "/pages/a/index.html", 10.0),
            page("KVBHDXYvY8f1b1AZG6PbAR", "/pages/b/index.html", 20.0),
            page("Hs8fJ2kLm3nP4qR5tU6vW7", "", 30.0),
        ];
        for url in [
            "https://claude.ai/artifact/KVBHDXYvY8f1b1AZG6PbAR",
            "https://claude.ai/code/artifact/95b3a80d-bd70-429a-b800-d1089310408a",
        ] {
            match decide(url, &pages) {
                Opens::Saved(p) => assert_eq!(p.path, "/pages/b/index.html", "{url}"),
                other => panic!("{url}: {other:?}"),
            }
        }
        assert_eq!(
            decide("https://claude.ai/artifact/Hs8fJ2kLm3nP4qR5tU6vW7", &pages),
            Opens::Live(ArtifactLink {
                id: "Hs8fJ2kLm3nP4qR5tU6vW7".into(),
                code: false
            })
        );
        assert!(matches!(
            decide("https://claude.ai/artifact/Zx1cV2bN3mA4sD5fG6hJ7k", &[]),
            Opens::Live(_)
        ));
        assert_eq!(decide("https://example.com/x", &pages), Opens::Browser);
        assert_eq!(decide("https://claude.ai/new", &pages), Opens::Browser);
    }

    #[test]
    fn the_subtitle_says_which_it_is() {
        let saved = Opens::Saved(Box::new(page("a", "/p/a/index.html", 0.0)));
        assert_eq!(subtitle(&saved), "Saved copy");
        let live = Opens::Live(ArtifactLink {
            id: "a".into(),
            code: false,
        });
        assert_eq!(subtitle(&live), "Live on claude.ai");
        let dated = Opens::Saved(Box::new(page("a", "/p/a/index.html", 1_800_000_000.0)));
        assert!(subtitle(&dated).starts_with("Saved copy · "));
    }

    #[test]
    fn a_live_page_stays_on_claude_ai() {
        for url in [
            "https://claude.ai/artifact/abc",
            "https://CLAUDE.ai/login?next=x",
            "https://www.claude.ai/",
            "https://api.claude.ai/x",
            "https://abc.claudeusercontent.com/frame",
            "https://claudeusercontent.com/",
            "https://challenges.cloudflare.com/turnstile",
            "https://claude.ai:443/x",
            "about:blank",
            "about:srcdoc",
            "blob:https://claude.ai/1234-5678",
        ] {
            assert!(live_allows(url), "{url}");
        }
        for url in [
            "http://claude.ai/artifact/abc",
            "https://claude.ai.evil.example/",
            "https://evilclaude.ai/",
            "https://evil.example/?u=https://claude.ai",
            "https://claude.ai@evil.example/",
            "https://user:pw@claude.ai/",
            "https://accounts.google.com/o/oauth2",
            "https://cloudflare.com/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "blob:https://evil.example/1",
            "blob:null/1",
            "https:///x",
            "",
        ] {
            assert!(!live_allows(url), "{url}");
        }
        assert!(opens_in_browser("https://example.com"));
        assert!(opens_in_browser("HTTP://example.com"));
        assert!(!opens_in_browser("file:///etc/passwd"));
        assert!(!opens_in_browser("javascript:alert(1)"));
        assert!(!opens_in_browser("mailto:a@b.c"));
    }

    #[test]
    fn the_frame_is_turned_over_for_appkit_and_put_on_device_pixels() {
        let panel = Frame {
            x: 700.0,
            y: 105.0,
            width: 520.0,
            height: 595.0,
        };
        assert_eq!(
            appkit_frame(panel, 700.0, false, 2.0),
            Frame {
                x: 700.0,
                y: 0.0,
                width: 520.0,
                height: 595.0
            }
        );
        assert_eq!(appkit_frame(panel, 700.0, true, 2.0).y, 105.0);
        let odd = Frame {
            x: 10.25,
            y: 20.5,
            width: 100.5,
            height: 50.25,
        };
        assert_eq!(
            appkit_frame(odd, 400.0, true, 2.0),
            Frame {
                x: 10.5,
                y: 20.5,
                width: 100.5,
                height: 50.5
            }
        );
        assert_eq!(
            appkit_frame(odd, 400.0, false, 1.0),
            Frame {
                x: 10.0,
                y: 400.0 - 71.0,
                width: 101.0,
                height: 50.0
            }
        );
        let none = Frame {
            x: 5.0,
            y: 5.0,
            width: -3.0,
            height: 0.0,
        };
        let f = appkit_frame(none, 100.0, false, 0.0);
        assert_eq!((f.width, f.height, f.y), (0.0, 0.0, 95.0));
    }

    #[test]
    fn the_width_leaves_the_conversation_room() {
        assert_eq!(panel_width(DEFAULT_WIDTH, 1200.0), 520.0);
        assert_eq!(panel_width(1000.0, 1200.0), 820.0);
        assert_eq!(panel_width(100.0, 1200.0), MIN_WIDTH);
        assert_eq!(panel_width(520.0, 500.0), MIN_WIDTH);
        assert_eq!(dragged_width(600.0, 260.0, 1260.0), 620.0);
        assert_eq!(dragged_width(300.0, 260.0, 1260.0), 1000.0 - LEAVES);
        assert_eq!(dragged_width(1250.0, 260.0, 1260.0), MIN_WIDTH);
    }
}
