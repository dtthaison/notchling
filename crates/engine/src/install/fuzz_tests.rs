//! Many generated settings files, in many layouts, through every change. Each change is checked
//! inside `plan_hooks` against the same change made to the data; here the files are the input
//! nobody wrote by hand: inline and spread-out containers mixed, comments, trailing commas,
//! tabs, `\r\n`, a final newline or none.

use super::command::{classify, Origin};
use super::hooks::{plan_hooks, Want};
use super::testutil::*;
use serde_json::{json, Value};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

/// How a file is laid out.
struct Style {
    unit: &'static str,
    colon: &'static str,
    nl: &'static str,
    /// Percent of containers written on one line.
    inline: u64,
    jsonc: bool,
}

fn layout(v: &Value, style: &Style, rng: &mut Rng, depth: usize, out: &mut String) {
    let pad = |d: usize| style.unit.repeat(d);
    match v {
        Value::Object(map) if !map.is_empty() => {
            let inline = rng.chance(style.inline);
            out.push('{');
            let n = map.len();
            for (i, (k, val)) in map.iter().enumerate() {
                if inline {
                    out.push_str(if i == 0 { "" } else { ", " });
                } else {
                    out.push_str(style.nl);
                    out.push_str(&pad(depth + 1));
                }
                out.push_str(&format!(
                    "{}{}",
                    serde_json::to_string(k).unwrap(),
                    style.colon
                ));
                layout(val, style, rng, depth + 1, out);
                tail(i + 1 == n, inline, style, rng, out);
            }
            if !inline {
                out.push_str(style.nl);
                out.push_str(&pad(depth));
            }
            out.push('}');
        }
        Value::Array(items) if !items.is_empty() => {
            let inline = rng.chance(style.inline);
            out.push('[');
            let n = items.len();
            for (i, val) in items.iter().enumerate() {
                if inline {
                    out.push_str(if i == 0 { "" } else { ", " });
                } else {
                    out.push_str(style.nl);
                    out.push_str(&pad(depth + 1));
                }
                layout(val, style, rng, depth + 1, out);
                tail(i + 1 == n, inline, style, rng, out);
            }
            if !inline {
                out.push_str(style.nl);
                out.push_str(&pad(depth));
            }
            out.push(']');
        }
        other => out.push_str(&serde_json::to_string(other).unwrap()),
    }
}

/// The comma after an item, a trailing one sometimes, and a comment sometimes.
fn tail(last: bool, inline: bool, style: &Style, rng: &mut Rng, out: &mut String) {
    let comma = !inline && (!last || (style.jsonc && rng.chance(40)));
    if comma {
        out.push(',');
    }
    if style.jsonc && !inline && rng.chance(30) {
        out.push_str(" // note");
    }
}

fn user_command(rng: &mut Rng) -> String {
    rng.pick(&[
        "~/bin/guard.sh",
        "echo hi",
        "prettier --write f",
        "notchling-lint hook",
    ])
    .to_string()
}

fn entry(rng: &mut Rng, slot_matcher: &str) -> Value {
    let command = match rng.below(6) {
        0 => CMD.to_string(),
        1 => "'/old/dir/notchling' hook".to_string(),
        2 => "'/Applications/Notchling.app/Contents/MacOS/Notchling' --hook".to_string(),
        _ => user_command(rng),
    };
    let mut e = json!({"type": "command", "command": command});
    if rng.chance(60) {
        e["timeout"] = json!(*rng.pick(&[5u64, 10, 60, 300]));
    }
    if rng.chance(15) {
        e["statusMessage"] = json!("working");
    }
    let _ = slot_matcher;
    e
}

fn group(rng: &mut Rng) -> Value {
    let matcher = *rng.pick(&["Bash", "AskUserQuestion", "Artifact", "*", "Edit|Write"]);
    let mut entries = Vec::new();
    for _ in 0..1 + rng.below(3) {
        entries.push(entry(rng, matcher));
    }
    let mut g = json!({"matcher": matcher, "hooks": entries});
    if rng.chance(20) {
        g["comment"] = json!("mine");
    }
    if rng.chance(10) {
        g.as_object_mut().map(|o| o.remove("matcher"));
    }
    g
}

fn settings(rng: &mut Rng) -> Value {
    let mut root = json!({"model": "opus", "n": [1, 2.50, 1e3]});
    if rng.chance(30) {
        root["permissions"] = json!({"allow": ["Bash(ls)"], "deny": []});
    }
    if rng.chance(85) {
        let mut hooks = serde_json::Map::new();
        for event in [
            "PreToolUse",
            "PostToolUse",
            "PermissionRequest",
            "PostToolUseFailure",
            "Stop",
        ] {
            if rng.chance(55) {
                let groups: Vec<Value> = (0..rng.below(4)).map(|_| group(rng)).collect();
                hooks.insert(event.to_string(), Value::Array(groups));
            }
        }
        root["hooks"] = Value::Object(hooks);
    }
    if rng.chance(20) {
        root["zzz"] = json!({"k": [true, null]});
    }
    root
}

fn styles() -> Vec<Style> {
    let mk = |unit, colon, nl, inline, jsonc| Style {
        unit,
        colon,
        nl,
        inline,
        jsonc,
    };
    vec![
        mk("  ", ": ", "\n", 0, false),
        mk("    ", ": ", "\n", 35, false),
        mk("\t", ": ", "\n", 20, false),
        mk("  ", " : ", "\n", 10, false),
        mk("  ", ": ", "\r\n", 15, false),
        mk("  ", ": ", "\n", 100, false),
        mk("  ", ": ", "\n", 20, true),
        mk("\t", ":", "\n", 40, true),
    ]
}

fn without_ours(mut v: Value) -> Value {
    let Some(hooks) = v.get_mut("hooks").and_then(Value::as_object_mut) else {
        return v;
    };
    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        for g in groups.iter_mut() {
            if let Some(list) = g.get_mut("hooks").and_then(Value::as_array_mut) {
                list.retain(|h| {
                    let c = h["command"].as_str().unwrap_or("");
                    classify(c) != Some(Origin::Notchling)
                });
            }
        }
    }
    v
}

/// Empty groups and events are not compared: a group of only ours goes, others stay.
fn normalised(v: Value) -> Value {
    let mut v = without_ours(v);
    if let Some(hooks) = v.get_mut("hooks").and_then(Value::as_object_mut) {
        for groups in hooks.values_mut() {
            if let Some(list) = groups.as_array_mut() {
                list.retain(|g| {
                    let own = g["hooks"].as_array().is_some_and(Vec::is_empty);
                    !(own
                        && g.as_object()
                            .is_some_and(|o| o.keys().all(|k| k == "matcher" || k == "hooks")))
                });
            }
        }
        hooks.retain(|_, g| g.as_array().is_none_or(|l| !l.is_empty()));
        if hooks.is_empty() {
            v.as_object_mut().map(|o| o.remove("hooks"));
        }
    }
    v
}

#[test]
fn two_thousand_generated_files_through_every_choice() {
    let styles = styles();
    let choices = [Want::Leave, Want::Install, Want::Remove];
    let mut rng = Rng(0x5eed);
    let mut edits = 0;
    for case in 0..2000 {
        let data = settings(&mut rng);
        let style = rng.pick(&styles);
        let mut text = String::new();
        layout(&data, style, &mut rng, 0, &mut text);
        if rng.chance(70) {
            text.push_str(style.nl);
        }
        if style.jsonc && rng.chance(50) {
            text = format!("// header{}{text}", style.nl);
        }
        let start = value(&text);
        assert_eq!(start, data, "case {case}: the generator's own text\n{text}");
        let w = wanted(*rng.pick(&choices), *rng.pick(&choices));
        let plan = match plan_hooks(&text, &w) {
            Ok(p) => p,
            Err(e) => panic!("case {case} {w:?}: {e}\n{text}"),
        };
        if plan.changed {
            edits += 1;
        }
        let after = value(&plan.text);
        assert_eq!(
            normalised(after.clone()),
            normalised(start.clone()),
            "case {case}\n{text}\n----\n{}",
            plan.text
        );
        // Nothing of the user's is lost, in the text either: every non-ours command is still there.
        for command in [
            "~/bin/guard.sh",
            "echo hi",
            "prettier --write f",
            "notchling-lint hook",
        ] {
            assert_eq!(
                text.matches(command).count(),
                plan.text.matches(command).count(),
                "case {case} {command}"
            );
        }
        assert_eq!(
            text.matches("--hook").count(),
            plan.text.matches("--hook").count(),
            "case {case}: the Swift app's entries"
        );
        let again = plan_hooks(&plan.text, &w).unwrap_or_else(|e| panic!("case {case} again: {e}"));
        assert!(!again.changed, "case {case} {w:?}\n{}", plan.text);
        // Taking ours out of the result and of the start gives the same file for the same layout
        // (comments and trailing commas may move, the data may not).
        let removed = plan_hooks(&plan.text, &remove_all()).unwrap();
        assert_eq!(
            normalised(value(&removed.text)),
            normalised(start),
            "case {case}"
        );
    }
    assert!(edits > 500, "{edits} of 2000 cases changed something");
}
