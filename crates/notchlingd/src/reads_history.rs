use super::*;
use crate::search_redact::mask;
use crate::search_scope::{Reach, AGENT_WINDOW_DAYS, AGENT_WINDOW_SECS};
use notchling_engine::sessions::history::{self, Item, Kind, Query, PAGE};
use notchling_engine::sessions::Roots;
use std::collections::HashSet;

const MAX_LIMIT: usize = 100;
const EXITED: &str = "Exited";

fn bad(message: &str) -> ApiError {
    ApiError::bad_request(message)
}

fn limit_of(args: &Value) -> Result<usize, ApiError> {
    match args.get("limit") {
        None | Some(Value::Null) => Ok(PAGE),
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=MAX_LIMIT as u64).contains(n))
            .map(|n| n as usize)
            .ok_or_else(|| bad("limit is a whole number from 1 to 100")),
    }
}

fn cursor_of(args: &Value) -> Result<Option<String>, ApiError> {
    match args.get("before") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if history::valid_cursor(s) => Ok(Some(s.clone())),
        Some(_) => Err(bad("before is the next cursor of an earlier reply")),
    }
}

fn text(row: &Value, key: &str) -> String {
    row.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn stem(path: &str) -> Option<String> {
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
}

fn row_of(item: &Item) -> Value {
    json!({
        "kind": match item.kind { Kind::Ended => "ended", Kind::Past => "past" },
        "id": item.id,
        "title": mask(&item.title),
        "folder": item.folder,
        "configDir": item.config_dir,
        "modifiedMs": item.modified_ms,
    })
}

impl ReadsOps {
    fn live_rows(&self) -> (Vec<Item>, HashSet<String>) {
        let list = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        let mut ended = Vec::new();
        let mut seen = HashSet::new();
        for row in list.as_array().into_iter().flatten() {
            let id = text(row, "id");
            for key in ["resumeSessionId", "id"] {
                let value = text(row, key);
                if !value.is_empty() {
                    seen.insert(value);
                }
            }
            if let Some(name) = stem(&text(row, "transcriptPath")) {
                seen.insert(name);
            }
            if text(row, "label") != EXITED || id.is_empty() {
                continue;
            }
            let cwd = text(row, "cwd");
            let name = text(row, "name");
            ended.push(Item {
                kind: Kind::Ended,
                title: if name.is_empty() {
                    text(row, "project")
                } else {
                    name
                },
                id,
                folder: cwd,
                config_dir: row
                    .get("configDir")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                path: None,
                modified_ms: row.get("sinceMs").and_then(Value::as_i64).unwrap_or(0),
            });
        }
        (ended, seen)
    }

    pub(super) fn history(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let limit = limit_of(args)?;
        let before = cursor_of(args)?;
        let Some(org) = self.search_org(caller) else {
            return Ok(no_session(
                json!({"sessions": [], "next": null, "window_days": AGENT_WINDOW_DAYS}),
            ));
        };
        let now = (self.now)();
        let reach = Reach {
            org: &org,
            placing: self,
            now,
        };
        let store = &self.claude.env().lanes;
        let sources = history::sources(&Roots {
            main_dir: store.main_dir.clone(),
            lanes_file: store.lanes_file.clone(),
        });
        let (ended, skip) = self.live_rows();
        let keep =
            |folder: &str, modified_ms: i64| reach.session(folder, modified_ms as f64 / 1000.0);
        let page = self.history.page(&Query {
            sources: &sources,
            ended,
            skip: &skip,
            since_ms: Some(((now - AGENT_WINDOW_SECS) * 1000.0) as i64),
            keep: &keep,
            after: before.as_deref(),
            limit,
        });
        Ok(json!({
            "sessions": page.items.iter().map(row_of).collect::<Vec<_>>(),
            "next": page.next,
            "organization": org,
            "masked": true,
            "window_days": AGENT_WINDOW_DAYS,
        }))
    }
}
