use serde_json::Value;

use super::setup::{short_path, tool_index};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drift {
    InStep,
    Outdated,
    Edited,
    Missing,
    ChangedByTool,
    Returned,
    Skipped,
}

impl Drift {
    pub fn parse(text: &str) -> Drift {
        match text {
            "in_step" => Drift::InStep,
            "outdated" => Drift::Outdated,
            "edited" => Drift::Edited,
            "missing" => Drift::Missing,
            "changed_by_tool" => Drift::ChangedByTool,
            "returned" => Drift::Returned,
            _ => Drift::Skipped,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Drift::InStep => "in-step",
            Drift::Outdated => "outdated",
            Drift::Edited => "edited",
            Drift::Missing => "missing",
            Drift::ChangedByTool => "changed-by-tool",
            Drift::Returned => "returned",
            Drift::Skipped => "skipped",
        }
    }

    pub fn dot(self) -> Dot {
        match self {
            Drift::InStep => Dot::Done,
            Drift::Edited | Drift::Missing | Drift::Returned => Dot::Needs,
            Drift::Outdated | Drift::ChangedByTool => Dot::Working,
            Drift::Skipped => Dot::Muted,
        }
    }

    pub fn resolutions(self) -> &'static [Resolve] {
        match self {
            Drift::Edited | Drift::ChangedByTool | Drift::Returned => {
                &[Resolve::Keep, Resolve::Restore]
            }
            Drift::Missing | Drift::Outdated => &[Resolve::Restore],
            Drift::InStep | Drift::Skipped => &[],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dot {
    Done,
    Working,
    Needs,
    Muted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolve {
    Keep,
    Restore,
}

impl Resolve {
    pub fn choice(self) -> &'static str {
        match self {
            Resolve::Keep => "keep",
            Resolve::Restore => "restore",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub tool: usize,
    pub path: String,
    pub state: Drift,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckModel {
    pub on: bool,
    pub loaded: bool,
    pub rows: Vec<Row>,
    pub busy: Option<String>,
    pub message: Option<String>,
}

impl CheckModel {
    pub fn show(&mut self, reply: Result<Value, String>, home: &str) {
        self.busy = None;
        match reply {
            Ok(v) => {
                if v.get("on").and_then(Value::as_bool) == Some(false) && v.get("rows").is_none() {
                    self.message = v.get("message").and_then(Value::as_str).map(str::to_owned);
                    self.loaded = true;
                    return;
                }
                self.on = v.get("on").and_then(Value::as_bool).unwrap_or(false);
                self.rows = v
                    .get("rows")
                    .and_then(Value::as_array)
                    .map(|list| {
                        list.iter()
                            .filter_map(|r| {
                                Some(Row {
                                    id: r.get("id")?.as_str()?.to_owned(),
                                    tool: tool_index(r.get("tool")?.as_str()?)?,
                                    path: short_path(r.get("path")?.as_str()?, home),
                                    state: Drift::parse(r.get("state")?.as_str()?),
                                    detail: r
                                        .get("detail")
                                        .and_then(Value::as_str)
                                        .map(str::to_owned),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                self.message = None;
                self.loaded = true;
            }
            Err(why) => {
                self.message = Some(why);
                self.loaded = true;
            }
        }
    }

    pub fn drifted(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| !matches!(r.state, Drift::InStep | Drift::Skipped))
            .count()
    }

    pub fn can_resolve(&self) -> bool {
        self.on && self.busy.is_none()
    }

    pub fn start_resolve(&mut self, id: &str) -> bool {
        if !self.can_resolve() {
            return false;
        }
        self.busy = Some(id.to_owned());
        true
    }
}
