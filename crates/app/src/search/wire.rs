//! The search ops (Milestone 5, Task 3) as the panel and the pages window send and read them.
//!
//! The contract: `search.query {text, limit}` answers `{pages: [...], sessions: [...]}`,
//! `pages.list` and `pages.find {query}` answer `{pages: [...]}`; a page is `{id, title, folder,
//! account, updated, link, path}` and a session `{id, tool, title, folder, snippet, highlights:
//! [[start, end]], updated}`. Task 3 lands beside this one, so the readers take what the daemon
//! sends tolerantly: unknown fields are ignored, a missing optional field gets its default, a key
//! may be spelled the Swift way (`url`, `file`, `sessionID`), and a time may be seconds or
//! milliseconds. Adjusting to the final shape means changing this file alone.

use notchling_engine::search::{artifact_id_in, PageEntry, SessionTool};
use serde_json::{json, Value};

/// The search ops (agent tier: reads).
pub mod ops {
    pub const SEARCH_QUERY: &str = "search.query";
    pub const PAGES_LIST: &str = "pages.list";
    pub const PAGES_FIND: &str = "pages.find";
}

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
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    /// The artifact id (the end of its claude.ai link); "" for a page that never got a link.
    pub id: String,
    pub title: String,
    /// The folder holding the local copy.
    pub folder: String,
    pub project: String,
    /// The account that first published it.
    pub account: String,
    /// Seconds since 1970.
    pub updated_secs: f64,
    /// The claude.ai link; "" when there is none.
    pub link: String,
    /// The local copy's file.
    pub path: String,
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
            file: self.path.clone(),
            title: self.title.clone(),
            url: self.link.clone(),
            source: String::new(),
            project: self.project.clone(),
            account: self.account.clone(),
            updated: self.updated_secs,
            version: String::new(),
        }
    }
}

/// A conversation search found.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionRow {
    /// `<tool>:<session id>`.
    pub id: String,
    pub tool: SessionTool,
    pub session_id: String,
    pub title: String,
    pub folder: String,
    /// The line around the match; none when the query matched only the title or folder.
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
        todo!()
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
}

/// Reads `search.query`'s answer. A missing list is empty; a row that can't be read is skipped.
pub fn parse_results(v: &Value) -> Results {
    todo!()
}

/// Reads `pages.list`'s and `pages.find`'s answer.
pub fn parse_pages(v: &Value) -> Vec<Page> {
    todo!()
}

/// One page; `None` without an id, a folder or a path to tell it by.
pub fn parse_page(v: &Value) -> Option<Page> {
    todo!()
}

/// One session; `None` without an id.
pub fn parse_session(v: &Value) -> Option<SessionRow> {
    todo!()
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
        assert_eq!(
            Call::pages_find("KVBHDXYvY8f1b1AZG6PbAR").args,
            json!({"query": "KVBHDXYvY8f1b1AZG6PbAR"})
        );
    }

    #[test]
    fn a_full_answer_is_read_as_the_contract_says() {
        let v = json!({
            "pages": [{
                "id": "KVBHDXYvY8f1b1AZG6PbAR", "title": "Bridge plan", "folder": "/pages/bridge",
                "account": "me@example.com", "updated": 1_800_000_000.5,
                "link": "https://claude.ai/artifact/KVBHDXYvY8f1b1AZG6PbAR", "path": "/pages/bridge/index.html",
                "extra": true
            }],
            "sessions": [{
                "id": "codex:0199a1b2", "tool": "codex", "title": "Fix the bridge", "folder": "/work/nl",
                "snippet": "the bridge was off", "highlights": [[4, 10]], "updated": 1_800_000_100
            }]
        });
        let r = parse_results(&v);
        assert_eq!(r.pages.len(), 1);
        let p = &r.pages[0];
        assert_eq!(p.id, "KVBHDXYvY8f1b1AZG6PbAR");
        assert_eq!(p.title, "Bridge plan");
        assert_eq!(p.folder, "/pages/bridge");
        assert_eq!(p.account, "me@example.com");
        assert_eq!(p.updated_secs, 1_800_000_000.5);
        assert_eq!(p.link, "https://claude.ai/artifact/KVBHDXYvY8f1b1AZG6PbAR");
        assert_eq!(p.path, "/pages/bridge/index.html");
        assert_eq!(p.project, "");
        assert_eq!(p.key(), "/pages/bridge");
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
    fn the_swift_spellings_and_milliseconds_are_read_too() {
        let v = json!({
            "pages": [{
                "folder": "/p/one", "file": "index.html", "url": "https://claude.ai/code/artifact/abcdefghijklmnopqrst",
                "project": "notchling", "updated": 1_800_000_000_000i64
            }],
            "sessions": [{
                "key": "claude:7c1e", "toolKey": "claude", "sessionID": "7c1e", "cwd": "/work/nl/app",
                "modified": 1_800_000_000_000i64, "highlights": [{"start": 1, "end": 3}]
            }]
        });
        let r = parse_results(&v);
        let p = &r.pages[0];
        assert_eq!(p.id, "abcdefghijklmnopqrst", "the id comes from the link");
        assert_eq!(p.title, "one", "no title: the folder's name");
        assert_eq!(p.path, "index.html");
        assert_eq!(p.link, "https://claude.ai/code/artifact/abcdefghijklmnopqrst");
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
            "sessions": [{"title": "no id"}, {"id": ""}, {"id": "agy:9"}]
        });
        let r = parse_results(&v);
        assert_eq!(r.pages.len(), 1);
        assert_eq!(r.pages[0].key(), "idonly12345678901234");
        assert_eq!(r.pages[0].title, "idonly12345678901234");
        assert_eq!(r.sessions.len(), 1);
        assert_eq!(r.sessions[0].tool, SessionTool::Antigravity);
        assert_eq!(r.sessions[0].session_id, "9");
        assert_eq!(r.sessions[0].title, "agy:9", "nothing else to call it");
        // A bad highlight pair is dropped, the rest kept; an empty snippet is none.
        let s = parse_session(&json!({"id": "claude:1", "snippet": "", "highlights": [[2], [0, 2], "x"]}))
            .unwrap();
        assert_eq!(s.snippet, None);
        assert_eq!(s.highlights, vec![(0, 2)]);
    }

    #[test]
    fn pages_list_is_read_with_the_same_reader() {
        let pages = parse_pages(&json!({"pages": [{"id": "a", "folder": "/a"}, {"folder": "/b"}]}));
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].id, "");
        assert_eq!(pages[1].key(), "/b");
        assert!(parse_pages(&json!({"pages": "x"})).is_empty());
        // A bare list works too.
        assert_eq!(parse_pages(&json!([{"folder": "/c"}])).len(), 1);
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
        assert_eq!((odd.tool, odd.session_id.as_str()), (SessionTool::Claude, "other:x"));
        assert_eq!(SessionRef::parse(""), None);
        assert_eq!(SessionRef::parse("codex:"), None);
    }

    #[test]
    fn a_page_becomes_the_engines_entry() {
        let p = parse_page(&json!({"id": "abcdefghijklmnopqrst", "title": "T", "folder": "/f", "path": "/f/i.html",
            "link": "https://claude.ai/artifact/abcdefghijklmnopqrst", "account": "a", "project": "p", "updated": 5}))
        .unwrap();
        let e = p.entry();
        assert_eq!((e.folder.as_str(), e.file.as_str(), e.title.as_str()), ("/f", "/f/i.html", "T"));
        assert_eq!(e.url, p.link);
        assert_eq!((e.account.as_str(), e.project.as_str(), e.updated), ("a", "p", 5.0));
        assert_eq!(e.artifact_id(), Some("abcdefghijklmnopqrst"));
    }
}
