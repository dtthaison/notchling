//! Planning the hook entries in settings.json: which of ours to add, replace or remove, and the
//! new text, with every other byte of the file as it was.
//!
//! # What is ours
//!
//! An entry is ours when its command is `notchling hook` ([`classify`]) **and** it sits in one of
//! the [`SLOTS`]: the prompt hooks (`PreToolUse` matching `AskUserQuestion`, `PermissionRequest`
//! matching `*`) or the page hooks (`PreToolUse`, `PostToolUse` and `PostToolUseFailure`
//! matching `Artifact`). Prompt and page hooks share `PreToolUse` and are told apart by the
//! matcher. A hook of ours anywhere else is reported, never changed. The Swift app's `--hook`
//! entries are reported and never changed: retirement rewrites them, not this.
//!
//! # What a change does
//!
//! Only entries are removed, never the group around other hooks: a group left with no hook of
//! any kind goes (and an event, and `hooks`, left empty by that), while a group that holds the
//! user's hooks too keeps them and everything else it has. New entries go last in their event.
//! Everything is done by splicing ([`super::edit`]); the result is then compared with what the
//! same change does to the parsed data, and a difference is a refusal, not a write.

use super::command::{classify, summary, Origin};
use super::edit::{self, key, Seg};
use super::jsonc::{self, Kind, Node};
use super::render::Json;
use serde_json::{json, Value};

/// A prompt hook may wait this long for an answer: the daemon's hold (290 s) and ten more.
pub const PROMPT_TIMEOUT: u64 = 300;
/// A page hook that may publish as the page's owner.
pub const PAGE_PRE_TIMEOUT: u64 = 60;
/// A page hook that only copies files.
pub const PAGE_TIMEOUT: u64 = 10;

/// What to do about a kind of hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    Leave,
    Install,
    Remove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// Questions and permissions answered from the notch.
    Prompt,
    /// Local copies of Artifact pages.
    Pages,
}

/// A place in settings.json where one of our entries lives.
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub category: Category,
    pub event: &'static str,
    pub matcher: &'static str,
    pub timeout: u64,
}

pub const SLOTS: [Slot; 5] = [
    Slot {
        category: Category::Prompt,
        event: "PreToolUse",
        matcher: "AskUserQuestion",
        timeout: PROMPT_TIMEOUT,
    },
    Slot {
        category: Category::Prompt,
        event: "PermissionRequest",
        matcher: "*",
        timeout: PROMPT_TIMEOUT,
    },
    Slot {
        category: Category::Pages,
        event: "PreToolUse",
        matcher: "Artifact",
        timeout: PAGE_PRE_TIMEOUT,
    },
    Slot {
        category: Category::Pages,
        event: "PostToolUse",
        matcher: "Artifact",
        timeout: PAGE_TIMEOUT,
    },
    Slot {
        category: Category::Pages,
        event: "PostToolUseFailure",
        matcher: "Artifact",
        timeout: PAGE_TIMEOUT,
    },
];

/// What the file should end up holding.
#[derive(Clone, Debug)]
pub struct Wanted {
    /// The hook command ([`super::command::hook_command`]).
    pub command: String,
    pub prompt: Want,
    pub pages: Want,
}

impl Wanted {
    fn want(&self, category: Category) -> Want {
        match category {
            Category::Prompt => self.prompt,
            Category::Pages => self.pages,
        }
    }
}

/// A hook entry of ours or the Swift app's found in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub event: String,
    pub matcher: Option<String>,
    pub origin: Origin,
    pub command: String,
    /// True for an entry of ours in one of the [`SLOTS`]: the ones a change may touch.
    pub managed: bool,
}

#[derive(Clone, Debug)]
pub struct HookPlan {
    /// The file after the change; the text given when nothing changes.
    pub text: String,
    pub changed: bool,
    /// One line per change.
    pub changes: Vec<String>,
    /// What else is worth saying (what is already in place, the Swift app's entries).
    pub notes: Vec<String>,
    /// What the file holds now.
    pub found: Vec<Found>,
}

/// Why the file is left alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused(pub String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn refuse(why: impl Into<String>) -> Refused {
    Refused(why.into())
}

/// One of our or the Swift app's entries with its place.
struct Entry {
    found: Found,
    /// The command as written, for comparing; `found.command` is the short form that is shown.
    raw: String,
    group: usize,
    index: usize,
    kind: Option<String>,
    timeout: Option<u64>,
}

fn survey(root: &Node, text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let Some(hooks) = root.get("hooks").and_then(Node::members) else {
        return out;
    };
    for member in hooks {
        let Some(groups) = member.value.items() else {
            continue;
        };
        for (group, node) in groups.iter().enumerate() {
            let matcher = node.get("matcher").and_then(Node::as_str);
            let Some(entries) = node.get("hooks").and_then(Node::items) else {
                continue;
            };
            for (index, entry) in entries.iter().enumerate() {
                let Some(command) = entry.get("command").and_then(Node::as_str) else {
                    continue;
                };
                let Some(origin) = classify(command) else {
                    continue;
                };
                let managed = origin == Origin::Notchling
                    && SLOTS
                        .iter()
                        .any(|s| s.event == member.key && Some(s.matcher) == matcher);
                out.push(Entry {
                    found: Found {
                        event: member.key.clone(),
                        matcher: matcher.map(str::to_string),
                        origin,
                        command: summary(command),
                        managed,
                    },
                    raw: command.to_string(),
                    group,
                    index,
                    kind: entry.get("type").and_then(Node::as_str).map(str::to_string),
                    timeout: entry
                        .get("timeout")
                        .and_then(|t| text.get(t.start..t.end))
                        .and_then(|t| t.parse().ok()),
                });
            }
        }
    }
    out
}

fn open(text: &str) -> Result<(String, Node), Refused> {
    // Nothing to lose in an empty file: start from an empty object.
    let work = if jsonc::is_blank(text) {
        "{\n}\n".to_string()
    } else {
        text.to_string()
    };
    let root = jsonc::parse(&work).map_err(|e| {
        refuse(format!(
            "settings.json is not valid JSON ({e}); it was left as it is"
        ))
    })?;
    if !matches!(root.kind, Kind::Object(_)) {
        return Err(refuse(
            "settings.json does not hold a JSON object; it was left as it is",
        ));
    }
    // Which of two equal keys counts is Claude Code's choice (the last), not the reader's (the
    // first): a plan from the wrong one could remove the user's own hook. Nothing the message
    // says comes from the file.
    if root.count_key("hooks") > 1 {
        return Err(refuse(
            "settings.json has \"hooks\" twice; it was left as it is",
        ));
    }
    if root.get("hooks").is_some_and(Node::has_repeated_key) {
        return Err(refuse(
            "a key is there twice inside \"hooks\"; settings.json was left as it is",
        ));
    }
    Ok((work, root))
}

/// What is in the file now, with nothing changed. A file that cannot be read is refused.
pub fn detect(text: &str) -> Result<Vec<Found>, Refused> {
    let (work, root) = open(text)?;
    Ok(survey(&root, &work).into_iter().map(|e| e.found).collect())
}

fn describe(slot: &Slot) -> String {
    format!("{} ({})", slot.event, slot.matcher)
}

fn group_json(slot: &Slot, command: &str) -> Json {
    Json::obj(vec![
        ("matcher", Json::Str(slot.matcher.to_string())),
        (
            "hooks",
            Json::Arr(vec![Json::obj(vec![
                ("type", Json::Str("command".into())),
                ("command", Json::Str(command.to_string())),
                ("timeout", Json::Num(slot.timeout)),
            ])]),
        ),
    ])
}

/// Plans the change `wanted` asks for in `text` (settings.json as it is now; empty for a file
/// that is empty or missing).
pub fn plan_hooks(text: &str, wanted: &Wanted) -> Result<HookPlan, Refused> {
    let (work, root) = open(text)?;
    let entries = survey(&root, &work);
    let found: Vec<Found> = entries.iter().map(|e| e.found.clone()).collect();
    let mut changes = Vec::new();
    let mut notes = Vec::new();
    let mut removals: Vec<&Entry> = Vec::new();
    let mut appends: Vec<&Slot> = Vec::new();

    for slot in &SLOTS {
        let want = wanted.want(slot.category);
        if want == Want::Leave {
            continue;
        }
        let ours: Vec<&Entry> = entries
            .iter()
            .filter(|e| {
                e.found.managed
                    && e.found.event == slot.event
                    && e.found.matcher.as_deref() == Some(slot.matcher)
            })
            .collect();
        let keep = (want == Want::Install)
            .then(|| {
                ours.iter().position(|e| {
                    e.raw == wanted.command
                        && e.kind.as_deref() == Some("command")
                        && e.timeout == Some(slot.timeout)
                })
            })
            .flatten();
        let place = describe(slot);
        for (i, entry) in ours.iter().enumerate() {
            if Some(i) == keep {
                continue;
            }
            removals.push(entry);
            let why = match (want, keep) {
                (Want::Remove, _) => format!("remove {place}: {}", entry.found.command),
                (_, Some(_)) => format!("remove {place}: a second copy of the hook"),
                _ => format!(
                    "replace {place}: {} -> {} (timeout {} s)",
                    entry.found.command, wanted.command, slot.timeout
                ),
            };
            changes.push(why);
        }
        if want == Want::Install {
            match keep {
                Some(_) => notes.push(format!("{place}: already installed")),
                None => {
                    appends.push(slot);
                    if ours.is_empty() {
                        changes.push(format!(
                            "add {place}: {} (timeout {} s)",
                            wanted.command, slot.timeout
                        ));
                    }
                }
            }
        } else if ours.is_empty() {
            notes.push(format!("{place}: not installed"));
        }
    }
    for entry in &entries {
        let f = &entry.found;
        let place = format!("{} ({})", f.event, f.matcher.as_deref().unwrap_or("any"));
        match (f.origin, f.managed) {
            (Origin::Swift, _) => notes.push(format!(
                "the Swift app's hook is in {place}; it was not changed"
            )),
            (Origin::Notchling, false) => notes.push(format!(
                "a notchling hook in {place} is not one of ours to change; it was left alone"
            )),
            _ => {}
        }
    }

    if removals.is_empty() && appends.is_empty() {
        return Ok(HookPlan {
            text: text.to_string(),
            changed: false,
            changes,
            notes,
            found,
        });
    }
    let before = check_shape(&root, &work, &appends, &removals)?;

    let mut out = work.clone();
    let mut gone: Vec<(&str, usize, usize)> = removals
        .iter()
        .map(|e| (e.found.event.as_str(), e.group, e.index))
        .collect();
    gone.sort_by_key(|e| std::cmp::Reverse((e.1, e.2)));
    for (event, group, index) in &gone {
        out = remove_entry(&out, event, *group, *index)?;
    }
    for slot in &appends {
        out = add_group(&out, slot, &group_json(slot, &wanted.command))?;
    }

    // The same change made to the data must give the same data.
    let after = jsonc::parse(&out)
        .map(|n| jsonc::to_value(&n, &out))
        .map_err(|_| refuse("the edit did not give valid JSON; nothing was written"))?;
    let adds: Vec<(&str, Value)> = appends
        .iter()
        .map(|s| (s.event, group_json(s, &wanted.command).to_value()))
        .collect();
    if after != model(before, &gone, &adds) {
        return Err(refuse(
            "the edit did not give the expected result; nothing was written",
        ));
    }
    Ok(HookPlan {
        text: out,
        changed: true,
        changes,
        notes,
        found,
    })
}

/// Refuses a file whose shape makes the edit ambiguous, and returns its data.
fn check_shape(
    root: &Node,
    text: &str,
    appends: &[&Slot],
    removals: &[&Entry],
) -> Result<Value, Refused> {
    if let Some(hooks) = root.get("hooks") {
        let Some(members) = hooks.members() else {
            return Err(refuse("\"hooks\" is not an object; it was left as it is"));
        };
        let events = appends
            .iter()
            .map(|s| s.event)
            .chain(removals.iter().map(|e| e.found.event.as_str()));
        for event in events {
            let value = members.iter().find(|m| m.key == event).map(|m| &m.value);
            if value.is_some_and(|v| v.items().is_none()) {
                return Err(refuse(format!(
                    "\"{event}\" is not a list; settings.json was left as it is"
                )));
            }
        }
    }
    Ok(jsonc::to_value(root, text))
}

fn member_index(node: &Node, name: &str) -> Option<usize> {
    node.members()?.iter().position(|m| m.key == name)
}

fn edit_failed(e: edit::EditError) -> Refused {
    refuse(format!("the edit failed ({e}); nothing was written"))
}

/// Removes one hook entry; a group, an event or `hooks` left with nothing goes with it.
fn remove_entry(text: &str, event: &str, group: usize, index: usize) -> Result<String, Refused> {
    let root = jsonc::parse(text).map_err(|_| refuse("the edit did not give valid JSON"))?;
    let missing = || refuse("the file changed while it was being planned");
    let hooks = root.get("hooks").ok_or_else(missing)?;
    let groups = hooks.get(event).ok_or_else(missing)?;
    let node = groups
        .items()
        .and_then(|g| g.get(group))
        .ok_or_else(missing)?;
    let own = node
        .get("hooks")
        .and_then(Node::items)
        .ok_or_else(missing)?;
    let only_ours = own.len() == 1
        && node
            .members()
            .is_some_and(|m| m.iter().all(|m| m.key == "matcher" || m.key == "hooks"));
    let done = if !only_ours {
        let path = [key("hooks"), key(event), Seg::At(group), key("hooks")];
        edit::remove(text, &path, index)
    } else if groups.items().is_some_and(|g| g.len() > 1) {
        edit::remove(text, &[key("hooks"), key(event)], group)
    } else if hooks.members().is_some_and(|m| m.len() > 1) {
        let at = member_index(hooks, event).ok_or_else(missing)?;
        edit::remove(text, &[key("hooks")], at)
    } else {
        let at = member_index(&root, "hooks").ok_or_else(missing)?;
        edit::remove(text, &[], at)
    };
    done.map_err(edit_failed)
}

/// Adds a group at the end of an event, making the event and `hooks` if they are not there.
fn add_group(text: &str, slot: &Slot, group: &Json) -> Result<String, Refused> {
    let root = jsonc::parse(text).map_err(|_| refuse("the edit did not give valid JSON"))?;
    let done = match root.get("hooks") {
        None => edit::append(
            text,
            &[],
            Some("hooks"),
            &Json::obj(vec![(slot.event, Json::Arr(vec![group.clone()]))]),
        ),
        Some(hooks) if hooks.get(slot.event).is_none() => edit::append(
            text,
            &[key("hooks")],
            Some(slot.event),
            &Json::Arr(vec![group.clone()]),
        ),
        Some(_) => edit::append(text, &[key("hooks"), key(slot.event)], None, group),
    };
    done.map_err(edit_failed)
}

/// The data after the change, worked out directly on the data: the check on the splicing.
fn model(mut root: Value, removals: &[(&str, usize, usize)], adds: &[(&str, Value)]) -> Value {
    let Some(top) = root.as_object_mut() else {
        return root;
    };
    if !removals.is_empty() {
        if let Some(Value::Object(hooks)) = top.get_mut("hooks") {
            let mut events: Vec<String> = removals.iter().map(|r| r.0.to_string()).collect();
            events.sort();
            events.dedup();
            for event in events {
                let Some(Value::Array(groups)) = hooks.get_mut(&event) else {
                    continue;
                };
                let mut kept = Vec::new();
                for (g, group) in std::mem::take(groups).into_iter().enumerate() {
                    kept.extend(strip_group(group, |h| {
                        removals
                            .iter()
                            .any(|r| r.0 == event && r.1 == g && r.2 == h)
                    }));
                }
                if kept.is_empty() {
                    hooks.remove(&event);
                } else {
                    hooks.insert(event, Value::Array(kept));
                }
            }
            if hooks.is_empty() {
                top.remove("hooks");
            }
        }
    }
    for (event, group) in adds {
        let hooks = top.entry("hooks").or_insert_with(|| json!({}));
        if let Some(hooks) = hooks.as_object_mut() {
            let list = hooks.entry(event.to_string()).or_insert_with(|| json!([]));
            if let Some(list) = list.as_array_mut() {
                list.push(group.clone());
            }
        }
    }
    root
}

/// A group without the entries `gone` picks; `None` when nothing but our entries were in it.
fn strip_group(mut group: Value, gone: impl Fn(usize) -> bool) -> Option<Value> {
    let clean = group
        .as_object()
        .is_some_and(|o| o.keys().all(|k| k == "matcher" || k == "hooks"));
    let Some(Value::Array(entries)) = group.get_mut("hooks") else {
        return Some(group);
    };
    let before = entries.len();
    let rest: Vec<Value> = std::mem::take(entries)
        .into_iter()
        .enumerate()
        .filter(|(h, _)| !gone(*h))
        .map(|(_, v)| v)
        .collect();
    let touched = rest.len() != before;
    let empty = rest.is_empty();
    *entries = rest;
    if touched && empty && clean {
        None
    } else {
        Some(group)
    }
}
