use std::collections::BTreeSet;

use notchling_locale as t;
use serde_json::Value;

use super::profile::{imports_of, Imported, ImportedLine, TOOLS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    File,
    Keys,
    Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    New,
    Edit,
    Replace,
    Remove,
    Offer,
    Skip,
    InStep,
}

impl Change {
    pub fn writes(self) -> bool {
        matches!(
            self,
            Change::New | Change::Edit | Change::Replace | Change::Remove
        )
    }

    pub fn key(self) -> &'static str {
        match self {
            Change::New => "new",
            Change::Edit => "edit",
            Change::Replace => "replace",
            Change::Remove => "remove",
            Change::Offer => "offer",
            Change::Skip => "skip",
            Change::InStep => "in-step",
        }
    }
}

fn change_of(kind: &str, part: Part) -> Change {
    match kind {
        "create" => Change::New,
        "update" if part == Part::File => Change::Replace,
        "update" => Change::Edit,
        "remove" => Change::Remove,
        "offer" => Change::Offer,
        "skip" => Change::Skip,
        _ => Change::InStep,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyChange {
    pub key: String,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub id: String,
    pub tool: usize,
    pub path: String,
    pub part: Part,
    pub change: Change,
    pub needs: Vec<String>,
    pub reason: Option<String>,
    pub diff: String,
    pub keys: Vec<KeyChange>,
    pub notes: Vec<String>,
}

impl Action {
    pub fn explain(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.id.starts_with("claude.rules.replaced") {
            out.push(t::costs::explain_rules_replaced().to_owned());
        } else if self.id.starts_with("claude.pointer.") {
            out.push(t::costs::explain_pointer().to_owned());
        } else if self.id == "claude.orchestrator.new" {
            out.push(t::costs::explain_orchestrator_new().to_owned());
        } else if self.id == "codex.config" {
            out.push(t::costs::explain_codex_config().to_owned());
        }
        out.extend(self.reason.iter().map(|r| why_text(r)));
        out.extend(self.notes.iter().map(|n| why_text(n)));
        out
    }

    pub fn has_detail(&self) -> bool {
        !self.diff.is_empty() || !self.keys.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub tool: usize,
    pub found: bool,
    pub version: Option<String>,
    pub actions: Vec<Action>,
    pub in_step: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub on: bool,
    pub groups: Vec<Group>,
    pub imports: Vec<Imported>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    pub id: String,
    pub tool: usize,
    pub path: String,
    pub change: Change,
    pub backup: Option<String>,
    pub done: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Restored,
    Removed,
    Gone,
    Conflict,
    Failed,
}

impl Outcome {
    fn parse(text: &str) -> Outcome {
        match text {
            "restored" => Outcome::Restored,
            "removed" => Outcome::Removed,
            "gone" => Outcome::Gone,
            "conflict" => Outcome::Conflict,
            _ => Outcome::Failed,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Outcome::Restored => "restored",
            Outcome::Removed => "removed",
            Outcome::Gone => "gone",
            Outcome::Conflict => "conflict",
            Outcome::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoRow {
    pub path: String,
    pub outcome: Outcome,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Undone {
    pub tool: usize,
    pub rows: Vec<UndoRow>,
}

impl Undone {
    pub fn conflicts(&self) -> Vec<&UndoRow> {
        self.rows
            .iter()
            .filter(|r| r.outcome == Outcome::Conflict)
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Reading,
    Review,
    Applying,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLine {
    Add,
    Del,
    Hunk,
    Header,
    Plain,
}

pub fn diff_line(line: &str) -> DiffLine {
    if line.starts_with("+++") || line.starts_with("---") {
        DiffLine::Header
    } else if line.starts_with("@@") {
        DiffLine::Hunk
    } else if line.starts_with('+') {
        DiffLine::Add
    } else if line.starts_with('-') {
        DiffLine::Del
    } else {
        DiffLine::Plain
    }
}

pub fn fate_text(fate: &str) -> String {
    match fate {
        "kept" => t::costs::fate_kept().to_owned(),
        "regenerated" => t::costs::fate_regenerated().to_owned(),
        other => match other.strip_prefix("profile:") {
            Some(role) => t::costs::fate_moved(&super::profile::role_title(role)),
            None => other.to_owned(),
        },
    }
}

pub fn why_text(text: &str) -> String {
    text.split("; ")
        .map(|part| match part.rsplit_once(": ") {
            Some((key, why)) if known_why(why) => format!("{key}: {}", one_why(why)),
            _ if known_why(part) => one_why(part),
            _ => part.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn known_why(why: &str) -> bool {
    matches!(
        why,
        "kept"
            | "changed-since"
            | "alias-set"
            | "codex-rejects"
            | "toml-layout"
            | "edited"
            | "replacement-skipped"
            | "duplicate-id"
    )
}

fn one_why(why: &str) -> String {
    match why {
        "kept" => t::costs::why_kept(),
        "changed-since" => t::costs::why_changed_since(),
        "alias-set" => t::costs::why_alias_set(),
        "codex-rejects" => t::costs::why_codex_rejects(),
        "toml-layout" => t::costs::why_toml_layout(),
        "edited" => t::costs::why_edited(),
        "replacement-skipped" => t::costs::why_replacement_skipped(),
        _ => t::costs::why_duplicate_id(),
    }
    .to_owned()
}

pub fn key_line(change: &KeyChange) -> String {
    let before = if change.before.is_empty() {
        t::costs::key_unset().to_owned()
    } else {
        change.before.clone()
    };
    t::costs::key_change(&change.key, &before, &change.after)
}

pub fn short_path(path: &str, home: &str) -> String {
    if home.len() > 1 {
        if let Some(rest) = path.strip_prefix(home) {
            if rest.starts_with('/') {
                return format!("~{rest}");
            }
        }
    }
    path.to_owned()
}

pub fn tool_index(name: &str) -> Option<usize> {
    let name = if name == "antigravity" { "agy" } else { name };
    TOOLS.iter().position(|t| *t == name)
}

fn text_of(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn action_of(a: &Value, home: &str) -> Option<Action> {
    let part = match a.get("part")?.as_str()? {
        "keys" => Part::Keys,
        "block" => Part::Block,
        _ => Part::File,
    };
    let strings = |key: &str| -> Vec<String> {
        a.get(key)
            .and_then(Value::as_array)
            .map(|l| {
                l.iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let keys = a
        .get("keys")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|k| KeyChange {
                    key: k["key"].as_str().unwrap_or_default().to_owned(),
                    before: text_of(&k["before"]),
                    after: text_of(&k["after"]),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Action {
        id: a.get("id")?.as_str()?.to_owned(),
        tool: tool_index(a.get("tool")?.as_str()?)?,
        path: short_path(a.get("path")?.as_str()?, home),
        part,
        change: change_of(a.get("kind")?.as_str()?, part),
        needs: strings("needs"),
        reason: a.get("reason").and_then(Value::as_str).map(str::to_owned),
        diff: a
            .get("diff")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        keys,
        notes: strings("notes"),
    })
}

fn versions_of(reply: &Value) -> [Option<String>; 3] {
    let mut out: [Option<String>; 3] = [None, None, None];
    let rows = reply.get("tools").and_then(Value::as_array);
    for row in rows.into_iter().flatten() {
        let Some(tool) = row.get("tool").and_then(Value::as_str).and_then(tool_index) else {
            continue;
        };
        out[tool] = row
            .get("installed")
            .and_then(|i| i.get("version"))
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    out
}

fn report_lines(report: Option<&Value>) -> Vec<ImportedLine> {
    report
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    Some(ImportedLine {
                        number: r.get("line")?.as_u64()?,
                        text: r.get("text")?.as_str()?.to_owned(),
                        fate: r.get("fate")?.as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

impl Plan {
    pub fn from_reply(reply: &Value, versions: Option<&Value>, home: &str) -> Option<Plan> {
        let plan = reply.get("plan")?;
        let detected = plan.get("detected");
        let found = |key: &str| {
            detected
                .and_then(|d| d.get(key))
                .and_then(Value::as_bool)
                .unwrap_or(true)
        };
        let seen = [found("claude"), found("codex"), found("antigravity")];
        let numbers = versions.map(versions_of).unwrap_or_default();
        let all: Vec<Action> = plan
            .get("actions")
            .and_then(Value::as_array)
            .map(|l| l.iter().filter_map(|a| action_of(a, home)).collect())
            .unwrap_or_default();
        let groups = (0..3)
            .map(|tool| {
                let mut actions: Vec<Action> = all
                    .iter()
                    .filter(|a| a.tool == tool && seen[tool])
                    .filter(|a| a.change != Change::InStep)
                    .cloned()
                    .collect();
                actions.sort_by_key(|a| a.change);
                Group {
                    tool,
                    found: seen[tool],
                    version: numbers[tool].clone(),
                    in_step: all
                        .iter()
                        .filter(|a| a.tool == tool && a.change == Change::InStep)
                        .count(),
                    actions,
                }
            })
            .collect();
        let mut imports = plan.get("profile").map(imports_of).unwrap_or_default();
        if imports.is_empty() {
            let lines = report_lines(plan.get("report"));
            if !lines.is_empty() {
                imports.push(Imported {
                    source: String::new(),
                    lines,
                });
            }
        }
        Some(Plan {
            on: reply.get("on").and_then(Value::as_bool).unwrap_or(false),
            groups,
            imports,
        })
    }

    pub fn writes(&self) -> usize {
        self.groups
            .iter()
            .flat_map(|g| &g.actions)
            .filter(|a| a.change.writes())
            .count()
    }

    pub fn action(&self, id: &str) -> Option<&Action> {
        self.groups
            .iter()
            .flat_map(|g| &g.actions)
            .find(|a| a.id == id)
    }
}

#[derive(Clone, Debug)]
pub struct SetupModel {
    pub plan: Option<Plan>,
    pub consent: BTreeSet<String>,
    pub open: BTreeSet<String>,
    pub stage: Stage,
    pub written: Vec<Written>,
    pub undone: Vec<Undone>,
    pub message: Option<String>,
    pub home: String,
}

impl SetupModel {
    pub fn new(home: &str) -> SetupModel {
        SetupModel {
            plan: None,
            consent: BTreeSet::new(),
            open: BTreeSet::new(),
            stage: Stage::Reading,
            written: Vec::new(),
            undone: Vec::new(),
            message: None,
            home: home.to_owned(),
        }
    }

    pub fn show_plan(&mut self, plan: Result<Value, String>, versions: Option<Value>) {
        match plan.map(|r| Plan::from_reply(&r, versions.as_ref(), &self.home)) {
            Ok(Some(plan)) => {
                self.plan = Some(plan);
                self.message = None;
            }
            Ok(None) => {
                self.plan = None;
                self.message = Some(t::costs::setup_unreadable().to_owned());
            }
            Err(why) => {
                self.plan = None;
                self.message = Some(why);
            }
        }
        self.stage = Stage::Review;
    }

    pub fn consent_list(&self) -> Vec<String> {
        self.consent.iter().cloned().collect()
    }

    pub fn ticked(&self, action: &Action) -> bool {
        !action.needs.is_empty() && action.needs.iter().all(|n| self.consent.contains(n))
    }

    pub fn toggle_offer(&mut self, id: &str) -> bool {
        let Some(needs) = self
            .plan
            .as_ref()
            .and_then(|p| p.action(id))
            .map(|a| a.needs.clone())
        else {
            return false;
        };
        if needs.is_empty() {
            return false;
        }
        if needs.iter().all(|n| self.consent.contains(n)) {
            for n in &needs {
                self.consent.remove(n);
            }
        } else {
            self.consent.extend(needs);
        }
        true
    }

    pub fn toggle_diff(&mut self, id: &str) {
        if !self.open.remove(id) {
            self.open.insert(id.to_owned());
        }
    }

    pub fn is_open(&self, id: &str) -> bool {
        self.open.contains(id)
    }

    pub fn install_on(&self) -> bool {
        self.plan.as_ref().is_some_and(|p| p.on)
    }

    pub fn writes(&self) -> usize {
        self.plan.as_ref().map_or(0, Plan::writes)
    }

    pub fn can_apply(&self) -> bool {
        self.stage == Stage::Review && self.install_on() && self.writes() > 0
    }

    pub fn finish_apply(&mut self, reply: Result<Value, (String, Option<Value>)>) {
        let (results, message) = match reply {
            Ok(v) if v.get("on").and_then(Value::as_bool) == Some(false) => {
                self.message = v.get("message").and_then(Value::as_str).map(str::to_owned);
                self.stage = Stage::Review;
                return;
            }
            Ok(v) => (v, None),
            Err((why, data)) => (data.unwrap_or(Value::Null), Some(why)),
        };
        let rows: Vec<Written> = results
            .get("results")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|r| {
                        let id = r.get("id")?.as_str()?.to_owned();
                        let tool = self
                            .plan
                            .as_ref()
                            .and_then(|p| p.action(&id))
                            .map_or(0, |a| a.tool);
                        let path = short_path(r.get("path")?.as_str()?, &self.home);
                        let part = self
                            .plan
                            .as_ref()
                            .and_then(|p| p.action(&id))
                            .map_or(Part::File, |a| a.part);
                        Some(Written {
                            id,
                            tool,
                            path,
                            change: change_of(r.get("kind")?.as_str()?, part),
                            backup: r
                                .get("backup")
                                .and_then(Value::as_str)
                                .map(|b| short_path(b, &self.home)),
                            done: r.get("done").and_then(Value::as_bool).unwrap_or(false),
                            error: r.get("error").and_then(Value::as_str).map(str::to_owned),
                        })
                    })
                    .filter(|w| w.change.writes() || w.error.is_some())
                    .collect()
            })
            .unwrap_or_default();
        if rows.is_empty() && message.is_some() {
            self.plan = None;
            self.message = message;
            self.stage = Stage::Review;
            return;
        }
        self.written = rows;
        self.message = message;
        self.stage = Stage::Done;
    }

    pub fn undoable(&self) -> Vec<usize> {
        let mut tools: Vec<usize> = self
            .written
            .iter()
            .filter(|w| w.done)
            .map(|w| w.tool)
            .collect();
        tools.sort_unstable();
        tools.dedup();
        tools.retain(|t| self.undone.iter().all(|u| u.tool != *t));
        tools
    }

    pub fn finish_undo(&mut self, tool: usize, reply: Result<Value, String>) {
        match reply {
            Ok(v) if v.get("on").and_then(Value::as_bool) == Some(false) => {
                self.message = v.get("message").and_then(Value::as_str).map(str::to_owned);
            }
            Ok(v) => {
                let rows = v
                    .get("results")
                    .and_then(Value::as_array)
                    .map(|list| {
                        list.iter()
                            .map(|r| UndoRow {
                                path: short_path(
                                    r["path"].as_str().unwrap_or_default(),
                                    &self.home,
                                ),
                                outcome: Outcome::parse(r["outcome"].as_str().unwrap_or_default()),
                                detail: r["detail"].as_str().map(str::to_owned),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                self.undone.retain(|u| u.tool != tool);
                self.undone.push(Undone { tool, rows });
                self.message = None;
            }
            Err(why) => self.message = Some(why),
        }
    }

    pub fn undone_for(&self, tool: usize) -> Option<&Undone> {
        self.undone.iter().find(|u| u.tool == tool)
    }
}
