//! The search ops (Milestone 5, Task 3; `crates/notchlingd/src/pages_ops.rs` has the shapes) as
//! the panel and the pages window send and read them.
//!
//! `search.query {text?, limit?}` answers `{pages, sessions, indexing}`; `pages.list {}` and
//! `pages.find {query}` answer `{pages, truncated}`. A page is the `pages.json` entry plus `id`
//! (the end of its claude.ai link, `null` without one) and `path` (the local copy, absolute,
//! `null` when the stored names aren't plain); a session is the index's record with `key`
//! (Chat's row id), `snippet` (`null` without a match in the text) and `highlights` (byte ranges
//! into the snippet). The readers here take an answer tolerantly: unknown fields are ignored, a
//! missing or `null` field gets its default, a key may be spelled the Swift way (`link`,
//! `sessionID`), and a time may be seconds or milliseconds. Changing to a new shape means
//! changing this file alone.

use notchling_engine::search::{artifact_id_in, PageEntry, SessionTool};
use notchling_local_api::ops;
use serde_json::{json, Value};

/// One op and its arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub op: &'static str,
    pub args: Value,
}

impl Call {
    /// Pages and sessions for `text`, at most `limit` of each.
    pub fn query(text: &str, limit: usize) -> Call {
        Call {
            op: ops::SEARCH_QUERY,
            args: json!({ "text": text, "limit": limit }),
        }
    }

    pub fn pages_list() -> Call {
        Call {
            op: ops::PAGES_LIST,
            args: json!({}),
        }
    }

    pub fn pages_find(query: &str) -> Call {
        Call {
            op: ops::PAGES_FIND,
            args: json!({ "query": query }),
        }
    }
}

/// One saved Artifact page, as the daemon lists it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    /// The artifact id (the end of its claude.ai link); "" for a page that never got a link.
    pub id: String,
    pub title: String,
    /// The folder's name in the library.
    pub folder: String,
    /// The copy's file name in that folder.
    pub file: String,
    pub project: String,
    /// The account that first published it.
    pub account: String,
    /// Seconds since 1970.
    pub updated_secs: f64,
    /// The claude.ai link; "" when there is none.
    pub link: String,
    /// The local copy, absolute; "" when the daemon has none to give.
    pub path: String,
    /// claude.ai's id of the version last published from this Mac; "" when unknown.
    pub version: String,
}

impl Page {
    /// What tells one page from another: its folder, else its id.
    pub fn key(&self) -> &str {
        if self.folder.is_empty() {
            &self.id
        } else {
            &self.folder
        }
    }

    /// The engine's record of the same page, for `find_pages`.
    pub fn entry(&self) -> PageEntry {
        PageEntry {
            folder: self.folder.clone(),
            file: self.file.clone(),
            title: self.title.clone(),
            url: self.link.clone(),
            source: String::new(),
            project: self.project.clone(),
            account: self.account.clone(),
            updated: self.updated_secs,
            version: self.version.clone(),
        }
    }
}

/// A conversation search found.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionRow {
    /// Chat's row id, `<tool>:<session id>`.
    pub id: String,
    pub tool: SessionTool,
    pub session_id: String,
    pub title: String,
    pub folder: String,
    /// The text around the match; none when only the title or folder matched, or without words.
    pub snippet: Option<String>,
    /// Byte ranges into the snippet to draw bold.
    pub highlights: Vec<(usize, usize)>,
    /// Seconds since 1970.
    pub updated_secs: f64,
}

impl SessionRow {
    pub fn reference(&self) -> SessionRef {
        SessionRef {
            tool: self.tool,
            session_id: self.session_id.clone(),
        }
    }
}

/// What Chat needs to open a conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRef {
    pub tool: SessionTool,
    pub session_id: String,
}

impl SessionRef {
    /// `codex:abc` → Codex, `abc`. A key with an unknown tool is Claude Code's, like the index.
    pub fn parse(key: &str) -> Option<SessionRef> {
        if key.is_empty() {
            return None;
        }
        match key.split_once(':') {
            Some((tool, rest)) => match SessionTool::from_key(tool) {
                Some(_) if rest.is_empty() => None,
                Some(tool) => Some(SessionRef {
                    tool,
                    session_id: rest.to_owned(),
                }),
                None => Some(SessionRef {
                    tool: SessionTool::Claude,
                    session_id: key.to_owned(),
                }),
            },
            None => Some(SessionRef {
                tool: SessionTool::Claude,
                session_id: key.to_owned(),
            }),
        }
    }

    /// The index's key, `<tool>:<session id>`.
    pub fn key(&self) -> String {
        format!("{}:{}", self.tool.key(), self.session_id)
    }
}

/// `search.query`'s answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Results {
    pub pages: Vec<Page>,
    pub sessions: Vec<SessionRow>,
    /// Secret Vault entries that fit the words: the list's part only, never a value. The app fills
    /// these from its own vault list; the daemon's answer has none.
    pub secrets: Vec<notchling_engine::vault::VaultEntry>,
    /// The index is being read again: ask once more in a moment.
    pub indexing: bool,
}

/// `pages.list`'s and `pages.find`'s answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageList {
    pub pages: Vec<Page>,
    /// The list stopped short (a reply is one line of at most 1 MiB).
    pub truncated: bool,
}

fn field<'a>(v: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names
        .iter()
        .find_map(|n| v.get(*n))
        .filter(|v| !v.is_null())
}

fn text(v: &Value, names: &[&str]) -> Option<String> {
    field(v, names)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
}

fn flag(v: &Value, name: &str) -> bool {
    field(v, &[name]).and_then(Value::as_bool).unwrap_or(false)
}

/// A time as seconds: a number of 100 billion or more is milliseconds (that is the year 5138 in
/// seconds, and 1973 in milliseconds).
fn secs(v: &Value, names: &[&str]) -> f64 {
    let n = field(v, names).and_then(Value::as_f64).unwrap_or(0.0);
    if n >= 100_000_000_000.0 {
        n / 1000.0
    } else {
        n
    }
}

/// `v[name]` as a list, or `v` itself when it is one.
fn list<'a>(v: &'a Value, name: &str) -> &'a [Value] {
    field(v, &[name])
        .or(Some(v))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// The last part of a path, as a name.
fn last_component(path: &str) -> Option<String> {
    path.rsplit('/').find(|p| !p.is_empty()).map(str::to_owned)
}

/// `[start, end]` pairs, or `{start, end}` objects; anything else is skipped.
fn pairs(v: Option<&Value>) -> Vec<(usize, usize)> {
    v.and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let (start, end) = match item {
                        Value::Array(pair) if pair.len() == 2 => {
                            (pair[0].as_u64(), pair[1].as_u64())
                        }
                        Value::Object(_) => (
                            field(item, &["start"]).and_then(Value::as_u64),
                            field(item, &["end"]).and_then(Value::as_u64),
                        ),
                        _ => (None, None),
                    };
                    Some((usize::try_from(start?).ok()?, usize::try_from(end?).ok()?))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Reads `search.query`'s answer. A missing list is empty; a row that can't be read is skipped.
pub fn parse_results(v: &Value) -> Results {
    Results {
        pages: list(v, "pages").iter().filter_map(parse_page).collect(),
        sessions: list(v, "sessions")
            .iter()
            .filter_map(parse_session)
            .collect(),
        indexing: flag(v, "indexing"),
        secrets: Vec::new(),
    }
}

/// Reads `pages.list`'s and `pages.find`'s answer.
pub fn parse_pages(v: &Value) -> PageList {
    PageList {
        pages: list(v, "pages").iter().filter_map(parse_page).collect(),
        truncated: flag(v, "truncated"),
    }
}

/// One page; `None` without an id, a folder or a path to tell it by.
pub fn parse_page(v: &Value) -> Option<Page> {
    if !v.is_object() {
        return None;
    }
    let link = text(v, &["url", "link"]).unwrap_or_default();
    let id = text(v, &["id", "artifactId"])
        .or_else(|| artifact_id_in(&link))
        .unwrap_or_default();
    let folder = text(v, &["folder"]).unwrap_or_default();
    let path = text(v, &["path"]).unwrap_or_default();
    if id.is_empty() && folder.is_empty() && path.is_empty() {
        return None;
    }
    let title = text(v, &["title"])
        .or_else(|| last_component(&folder))
        .unwrap_or_else(|| id.clone());
    Some(Page {
        id,
        title,
        folder,
        file: text(v, &["file"]).unwrap_or_default(),
        project: text(v, &["project"]).unwrap_or_default(),
        account: text(v, &["account"]).unwrap_or_default(),
        updated_secs: secs(v, &["updated", "modified"]),
        link,
        path,
        version: text(v, &["version"]).unwrap_or_default(),
    })
}

/// One session; `None` without a key.
pub fn parse_session(v: &Value) -> Option<SessionRow> {
    let id = text(v, &["key", "id"])?;
    let from_key = SessionRef::parse(&id);
    let tool = text(v, &["tool", "toolKey"])
        .and_then(|t| SessionTool::from_key(&t))
        .or_else(|| from_key.as_ref().map(|r| r.tool))
        .unwrap_or(SessionTool::Claude);
    let session_id = text(v, &["session_id", "sessionId", "sessionID"])
        .or_else(|| from_key.map(|r| r.session_id))
        .unwrap_or_else(|| id.clone());
    let folder = text(v, &["folder", "cwd"]).unwrap_or_default();
    let title = text(v, &["title"])
        .or_else(|| last_component(&folder))
        .unwrap_or_else(|| id.clone());
    Some(SessionRow {
        id,
        tool,
        session_id,
        title,
        folder,
        snippet: text(v, &["snippet"]),
        highlights: pairs(field(v, &["highlights"])),
        updated_secs: secs(v, &["modified", "updated"]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calls_carry_the_contracts_arguments() {
        assert_eq!(
            Call::query("bridge", 30),
            Call {
                op: "search.query",
                args: json!({"text": "bridge", "limit": 30})
            }
        );
        assert_eq!(Call::pages_list().op, "pages.list");
        assert_eq!(Call::pages_list().args, json!({}));
        assert_eq!(Call::pages_find("x").op, "pages.find");
        assert_eq!(
            Call::pages_find("KVBHDXYvY8f1b1AZG6PbAR").args,
            json!({"query": "KVBHDXYvY8f1b1AZG6PbAR"})
        );
    }

    #[test]
    fn a_full_answer_is_read_as_the_daemon_sends_it() {
        let v = json!({
            "pages": [{
                "id": "KVBHDXYvY8f1b1AZG6PbAR", "title": "Bridge plan", "url": "https://claude.ai/artifact/KVBHDXYvY8f1b1AZG6PbAR",
                "project": "notchling", "account": "me@example.com", "updated": 1_800_000_000.5, "version": "1793506664-65e3",
                "folder": "bridge-plan-1a2b3c4d", "file": "index.html", "source": "/w/plan.html",
                "path": "/Users/me/Library/Application Support/Notchling/pages/bridge-plan-1a2b3c4d/index.html",
                "extra": true
            }],
            "sessions": [{
                "key": "codex:0199a1b2", "tool": "codex", "session_id": "0199a1b2", "title": "Fix the bridge",
                "folder": "/work/nl", "path": "/codex/sessions/rollout.jsonl", "config_dir": null,
                "modified": 1_800_000_100, "snippet": "the bridge was off", "highlights": [[4, 10]], "score": 61.5
            }],
            "indexing": true
        });
        let r = parse_results(&v);
        assert!(r.indexing);
        assert_eq!(r.pages.len(), 1);
        let p = &r.pages[0];
        assert_eq!(p.id, "KVBHDXYvY8f1b1AZG6PbAR");
        assert_eq!(p.title, "Bridge plan");
        assert_eq!(p.folder, "bridge-plan-1a2b3c4d");
        assert_eq!(p.file, "index.html");
        assert_eq!(p.project, "notchling");
        assert_eq!(p.account, "me@example.com");
        assert_eq!(p.updated_secs, 1_800_000_000.5);
        assert_eq!(p.link, "https://claude.ai/artifact/KVBHDXYvY8f1b1AZG6PbAR");
        assert_eq!(p.version, "1793506664-65e3");
        assert!(p.path.ends_with("/pages/bridge-plan-1a2b3c4d/index.html"));
        assert_eq!(p.key(), "bridge-plan-1a2b3c4d");
        let s = &r.sessions[0];
        assert_eq!(s.id, "codex:0199a1b2");
        assert_eq!(s.tool, SessionTool::Codex);
        assert_eq!(s.session_id, "0199a1b2");
        assert_eq!(s.title, "Fix the bridge");
        assert_eq!(s.folder, "/work/nl");
        assert_eq!(s.snippet.as_deref(), Some("the bridge was off"));
        assert_eq!(s.highlights, vec![(4, 10)]);
        assert_eq!(s.updated_secs, 1_800_000_100.0);
        assert_eq!(
            s.reference(),
            SessionRef {
                tool: SessionTool::Codex,
                session_id: "0199a1b2".into()
            }
        );
    }

    #[test]
    fn nulls_the_swift_spellings_and_milliseconds_are_read_too() {
        let v = json!({
            "pages": [{
                "id": null, "folder": "one-9f3e", "file": "index.html", "path": null,
                "link": "https://claude.ai/code/artifact/abcdefghijklmnopqrst",
                "project": "notchling", "updated": 1_800_000_000_000i64
            }],
            "sessions": [{
                "id": "claude:7c1e", "toolKey": "claude", "sessionID": "7c1e", "cwd": "/work/nl/app",
                "updated": 1_800_000_000_000i64, "snippet": null, "highlights": [{"start": 1, "end": 3}]
            }]
        });
        let r = parse_results(&v);
        assert!(!r.indexing);
        let p = &r.pages[0];
        assert_eq!(p.id, "abcdefghijklmnopqrst", "no id: the link's");
        assert_eq!(p.title, "one-9f3e", "no title: the folder's name");
        assert_eq!(p.path, "", "null: no copy to load");
        assert_eq!(p.file, "index.html");
        assert_eq!(
            p.link,
            "https://claude.ai/code/artifact/abcdefghijklmnopqrst"
        );
        assert_eq!(p.project, "notchling");
        assert_eq!(p.updated_secs, 1_800_000_000.0);
        let s = &r.sessions[0];
        assert_eq!(s.tool, SessionTool::Claude);
        assert_eq!(s.session_id, "7c1e");
        assert_eq!(s.folder, "/work/nl/app");
        assert_eq!(s.title, "app", "no title: the folder's name");
        assert_eq!(s.snippet, None);
        assert_eq!(s.highlights, vec![(1, 3)]);
        assert_eq!(s.updated_secs, 1_800_000_000.0);
    }

    #[test]
    fn missing_lists_and_unreadable_rows_are_skipped() {
        assert_eq!(parse_results(&json!({})), Results::default());
        assert_eq!(parse_results(&json!("nope")), Results::default());
        let v = json!({
            "pages": [{"title": "no identity"}, 7, {"id": "idonly12345678901234"}],
            "sessions": [{"title": "no key"}, {"key": ""}, {"key": "agy:9"}]
        });
        let r = parse_results(&v);
        assert_eq!(r.pages.len(), 1);
        assert_eq!(r.pages[0].key(), "idonly12345678901234");
        assert_eq!(r.pages[0].title, "idonly12345678901234");
        assert_eq!(r.sessions.len(), 1);
        assert_eq!(r.sessions[0].tool, SessionTool::Antigravity);
        assert_eq!(r.sessions[0].session_id, "9");
        assert_eq!(r.sessions[0].title, "agy:9", "nothing else to call it");
        // A bad highlight is dropped, the rest kept; an empty snippet is none.
        let s = parse_session(
            &json!({"key": "claude:1", "snippet": "", "highlights": [[2], [0, 2], "x", [-1, 2]]}),
        )
        .unwrap();
        assert_eq!(s.snippet, None);
        assert_eq!(s.highlights, vec![(0, 2)]);
    }

    #[test]
    fn a_page_list_says_when_it_was_cut_short() {
        let list = parse_pages(
            &json!({"pages": [{"id": "a", "folder": "a-1"}, {"folder": "b-2"}], "truncated": true}),
        );
        assert_eq!(list.pages.len(), 2);
        assert!(list.truncated);
        assert_eq!(list.pages[1].id, "");
        assert_eq!(list.pages[1].key(), "b-2");
        let none = parse_pages(&json!({"pages": "x"}));
        assert!(none.pages.is_empty() && !none.truncated);
        // A bare list works too.
        assert_eq!(parse_pages(&json!([{"folder": "c-3"}])).pages.len(), 1);
    }

    #[test]
    fn a_session_ref_round_trips_through_the_index_key() {
        let r = SessionRef::parse("codex:0199a1b2").unwrap();
        assert_eq!(r.tool, SessionTool::Codex);
        assert_eq!(r.session_id, "0199a1b2");
        assert_eq!(r.key(), "codex:0199a1b2");
        let bare = SessionRef::parse("7c1e-uuid").unwrap();
        assert_eq!(bare.tool, SessionTool::Claude);
        assert_eq!(bare.session_id, "7c1e-uuid");
        let odd = SessionRef::parse("other:x").unwrap();
        assert_eq!(
            (odd.tool, odd.session_id.as_str()),
            (SessionTool::Claude, "other:x")
        );
        assert_eq!(SessionRef::parse(""), None);
        assert_eq!(SessionRef::parse("codex:"), None);
    }

    #[test]
    fn a_page_becomes_the_engines_entry() {
        let p = parse_page(
            &json!({"id": "abcdefghijklmnopqrst", "title": "T", "folder": "f-1", "file": "i.html",
            "path": "/pages/f-1/i.html", "url": "https://claude.ai/artifact/abcdefghijklmnopqrst",
            "account": "a", "project": "p", "updated": 5, "version": "v1"}),
        )
        .unwrap();
        let e = p.entry();
        assert_eq!(
            (e.folder.as_str(), e.file.as_str(), e.title.as_str()),
            ("f-1", "i.html", "T")
        );
        assert_eq!(e.url, p.link);
        assert_eq!(
            (
                e.account.as_str(),
                e.project.as_str(),
                e.updated,
                e.version.as_str()
            ),
            ("a", "p", 5.0, "v1")
        );
        assert_eq!(e.artifact_id(), Some("abcdefghijklmnopqrst"));
    }
}
