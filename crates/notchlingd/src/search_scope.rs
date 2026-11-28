use crate::pages_ops::PagesOps;
use crate::reads_ops::{no_session_search, ReadsOps};
use notchling_engine::organizations::PERSONAL_ID;
use notchling_engine::search::PageEntry;
use notchling_local_api::{ops, ApiError, Caller};
use serde_json::{json, Value};
use std::path::Path;

pub const AGENT_WINDOW_DAYS: u64 = 14;
pub const AGENT_WINDOW_SECS: f64 = AGENT_WINDOW_DAYS as f64 * 86_400.0;
pub const POOL: usize = 1000;

pub trait Placing {
    fn org_of_folder(&self, folder: &str) -> String;
    fn claimed_by(&self, path: &str) -> Option<String>;
    fn only_personal(&self) -> bool;
}

pub struct Reach<'a> {
    pub org: &'a str,
    pub placing: &'a dyn Placing,
    pub now: f64,
}

pub fn page_org(placing: &dyn Placing, cwd: &str, source: &str) -> Option<String> {
    if notchling_engine::organizations::folders::standardize(Path::new(cwd)).is_some() {
        return Some(placing.org_of_folder(cwd));
    }
    if let Some(org) = placing.claimed_by(source) {
        return Some(org);
    }
    placing.only_personal().then(|| PERSONAL_ID.to_string())
}

impl Reach<'_> {
    pub fn page(&self, entry: &PageEntry) -> bool {
        page_org(self.placing, &entry.cwd, &entry.source).is_some_and(|org| org == self.org)
    }

    pub fn session(&self, folder: &str, modified: f64) -> bool {
        !folder.is_empty()
            && modified >= self.now - AGENT_WINDOW_SECS
            && self.placing.org_of_folder(folder) == self.org
    }
}

impl Placing for ReadsOps {
    fn org_of_folder(&self, folder: &str) -> String {
        self.folder_org(folder)
    }

    fn claimed_by(&self, path: &str) -> Option<String> {
        self.folder_claim(path)
    }

    fn only_personal(&self) -> bool {
        self.view_only_personal()
    }
}

fn no_session(op: &str, args: &Value) -> Value {
    if op == ops::SEARCH_QUERY {
        return no_session_search(args);
    }
    if op == ops::SEARCH_FACETS {
        return json!({"values": [], "disabled": [], "dropped": [], "reason": "no_session"});
    }
    json!({"pages": [], "truncated": false, "reason": "no_session"})
}

pub fn call(
    reads: Option<&ReadsOps>,
    pages: &PagesOps,
    caller: &Caller,
    op: &str,
    args: &Value,
) -> Result<Value, ApiError> {
    let Some((reads, org)) = reads.and_then(|r| r.search_org(caller).map(|org| (r, org))) else {
        return Ok(no_session(op, args));
    };
    pages.call_scoped(op, args, &org, reads)
}

#[cfg(test)]
#[path = "search_scope_tests.rs"]
mod tests;
