use super::command::{classify, Origin};
use super::hooks::*;
use super::testutil::*;
use serde_json::{json, Value};

fn plan(text: &str, wanted: &Wanted) -> HookPlan {
    plan_hooks(text, wanted).unwrap()
}

/// The data with our hook entries taken out the plain way (groups left empty go too).
fn without_ours(mut v: Value) -> Value {
    let Some(hooks) = v.get_mut("hooks").and_then(Value::as_object_mut) else {
        return v;
    };
    let events: Vec<String> = hooks.keys().cloned().collect();
    for event in events {
        let Some(groups) = hooks.get_mut(&event).and_then(Value::as_array_mut) else {
            continue;
        };
        for group in groups.iter_mut() {
            if let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                list.retain(|h| {
                    let c = h.get("command").and_then(Value::as_str).unwrap_or("");
                    classify(c) != Some(Origin::Notchling)
                });
            }
        }
        groups.retain(|g| {
            g.get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|l| !l.is_empty())
        });
        if groups.is_empty() {
            hooks.remove(&event);
        }
    }
    if hooks.is_empty() {
        v.as_object_mut().map(|o| o.remove("hooks"));
    }
    v
}

fn ours(v: &Value, event: &str) -> Vec<(String, String, u64)> {
    let mut out = Vec::new();
    for group in v["hooks"][event].as_array().into_iter().flatten() {
        for h in group["hooks"].as_array().into_iter().flatten() {
            let c = h["command"].as_str().unwrap_or("");
            if classify(c) == Some(Origin::Notchling) {
                out.push((
                    group["matcher"].as_str().unwrap_or("").to_string(),
                    c.to_string(),
                    h["timeout"].as_u64().unwrap_or(0),
                ));
            }
        }
    }
    out
}

const EVERY_FIXTURE: [(&str, &str); 7] = [
    ("user", USER_HOOKS),
    ("none", NO_HOOKS),
    ("empty-hooks", EMPTY_HOOKS),
    ("swift", SWIFT_INSTALLED),
    ("odd", ODD),
    ("compact", COMPACT),
    ("unknown", UNKNOWN_KEYS),
];

#[test]
fn installing_adds_the_five_entries_to_a_users_file_and_changes_nothing_else() {
    let out = plan(USER_HOOKS, &install_all());
    assert!(out.changed);
    assert_eq!(out.changes.len(), 5, "{:?}", out.changes);
    assert!(
        out.changes.iter().all(|c| c.starts_with("add ")),
        "{:?}",
        out.changes
    );
    let v = value(&out.text);
    assert_eq!(without_ours(v.clone()), value(USER_HOOKS));
    // The user's own hooks are where they were, in the same order.
    assert_eq!(
        v["hooks"]["PreToolUse"][0],
        value(USER_HOOKS)["hooks"]["PreToolUse"][0]
    );
    assert_eq!(v["hooks"]["Stop"], value(USER_HOOKS)["hooks"]["Stop"]);
    // And the rest of the file is byte for byte what it was: taking ours out gives it back.
    let back = plan(&out.text, &remove_all());
    assert!(back.changed);
    assert_eq!(back.text, USER_HOOKS);
}

#[test]
fn the_entries_are_the_swift_apps_slots_and_timeouts() {
    let v = value(&plan(NO_HOOKS, &install_all()).text);
    let pre = ours(&v, "PreToolUse");
    assert_eq!(
        pre,
        vec![
            ("AskUserQuestion".into(), CMD.into(), 300),
            ("Artifact".into(), CMD.into(), 60),
        ]
    );
    assert_eq!(
        ours(&v, "PermissionRequest"),
        vec![("*".into(), CMD.into(), 300)]
    );
    assert_eq!(
        ours(&v, "PostToolUse"),
        vec![("Artifact".into(), CMD.into(), 10)]
    );
    assert_eq!(
        ours(&v, "PostToolUseFailure"),
        vec![("Artifact".into(), CMD.into(), 10)]
    );
    let entry = &v["hooks"]["PermissionRequest"][0]["hooks"][0];
    assert_eq!(entry["type"], "command");
    assert_eq!(without_ours(v), value(NO_HOOKS));
}

#[test]
fn installing_twice_changes_nothing() {
    for (name, text) in EVERY_FIXTURE {
        let once = plan(text, &install_all());
        let twice = plan(&once.text, &install_all());
        assert!(!twice.changed, "{name}");
        assert_eq!(twice.text, once.text, "{name}");
        assert!(twice.changes.is_empty(), "{name}");
        assert_eq!(
            twice
                .notes
                .iter()
                .filter(|n| n.ends_with("already installed"))
                .count(),
            5,
            "{name}"
        );
    }
}

#[test]
fn removing_takes_exactly_ours_out_of_every_fixture() {
    for (name, text) in EVERY_FIXTURE {
        let installed = plan(text, &install_all());
        let back = plan(&installed.text, &remove_all());
        assert!(back.changed, "{name}");
        assert_eq!(back.changes.len(), 5, "{name}");
        assert!(ours(&value(&back.text), "PreToolUse").is_empty(), "{name}");
        match name {
            // Comments may sit where an entry went; an empty `hooks` is not worth keeping.
            "odd" | "empty-hooks" => assert_eq!(
                without_ours(value(&back.text)),
                without_ours(value(text)),
                "{name}"
            ),
            _ => assert_eq!(back.text, text, "{name}"),
        }
    }
}

#[test]
fn the_swift_apps_entries_are_reported_and_never_touched() {
    let found = detect(SWIFT_INSTALLED).unwrap();
    assert_eq!(found.len(), 5);
    assert!(found
        .iter()
        .all(|f| f.origin == Origin::Swift && !f.managed));
    assert!(found
        .iter()
        .any(|f| f.event == "PermissionRequest" && f.matcher.as_deref() == Some("*")));
    assert!(found
        .iter()
        .all(|f| f.command.ends_with("Notchling' --hook")));

    // Removing ours finds nothing to do, and the file is not rewritten.
    let nothing = plan(SWIFT_INSTALLED, &remove_all());
    assert!(!nothing.changed);
    assert_eq!(nothing.text, SWIFT_INSTALLED);
    assert_eq!(
        nothing
            .notes
            .iter()
            .filter(|n| n.contains("Swift app's hook"))
            .count(),
        5
    );

    // Installing adds ours beside theirs; theirs stay, in place.
    let out = plan(SWIFT_INSTALLED, &install_all());
    let v = value(&out.text);
    assert_eq!(without_ours(v.clone()), value(SWIFT_INSTALLED));
    assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 5);
    let now = detect(&out.text).unwrap();
    assert_eq!(now.iter().filter(|f| f.origin == Origin::Swift).count(), 5);
    assert_eq!(now.iter().filter(|f| f.managed).count(), 5);
    // Taking ours out again leaves the Swift app's file as it was.
    assert_eq!(plan(&out.text, &remove_all()).text, SWIFT_INSTALLED);
}

#[test]
fn prompt_hooks_and_page_hooks_are_chosen_apart() {
    let prompt = plan(NO_HOOKS, &wanted(Want::Install, Want::Leave));
    let v = value(&prompt.text);
    assert_eq!(prompt.changes.len(), 2);
    assert_eq!(ours(&v, "PreToolUse").len(), 1);
    assert_eq!(ours(&v, "PermissionRequest").len(), 1);
    assert!(ours(&v, "PostToolUse").is_empty());

    let both = plan(&prompt.text, &wanted(Want::Leave, Want::Install));
    assert_eq!(both.changes.len(), 3);
    let v = value(&both.text);
    assert_eq!(ours(&v, "PreToolUse").len(), 2);
    assert_eq!(ours(&v, "PostToolUseFailure").len(), 1);

    // Removing the page hooks leaves the prompt hooks, in the same event.
    let only_prompt = plan(&both.text, &wanted(Want::Leave, Want::Remove));
    let v = value(&only_prompt.text);
    assert_eq!(
        ours(&v, "PreToolUse"),
        vec![("AskUserQuestion".into(), CMD.into(), 300)]
    );
    assert!(ours(&v, "PostToolUse").is_empty());
    assert_eq!(only_prompt.text, prompt.text);
}

#[test]
fn a_stale_entry_is_replaced_and_a_second_copy_is_removed() {
    let old = "'/old/place/notchling' hook";
    let text = json!({
        "hooks": {
            "PermissionRequest": [
                {"matcher": "*", "hooks": [{"type": "command", "command": old, "timeout": 120}]},
                {"matcher": "Bash", "hooks": [{"type": "command", "command": "mine.sh"}]},
            ],
            "PreToolUse": [
                {"matcher": "AskUserQuestion", "hooks": [
                    {"type": "command", "command": CMD, "timeout": 300},
                    {"type": "command", "command": CMD, "timeout": 300},
                ]},
            ],
        }
    });
    let text = serde_json::to_string_pretty(&text).unwrap();
    let out = plan(&text, &wanted(Want::Install, Want::Leave));
    assert!(out.changed);
    assert!(out
        .changes
        .iter()
        .any(|c| c.starts_with("replace PermissionRequest (*)") && c.contains(old)));
    assert!(out.changes.iter().any(|c| c.contains("a second copy")));
    let v = value(&out.text);
    assert_eq!(
        ours(&v, "PermissionRequest"),
        vec![("*".into(), CMD.into(), 300)]
    );
    assert_eq!(
        ours(&v, "PreToolUse"),
        vec![("AskUserQuestion".into(), CMD.into(), 300)]
    );
    // The user's own group is untouched.
    assert_eq!(v["hooks"]["PermissionRequest"][0]["matcher"], "Bash");
    assert!(!out.text.contains(old));
}

#[test]
fn an_entry_that_shares_a_group_leaves_the_users_hook_and_the_groups_other_keys() {
    let text = r#"{
  "hooks": {
    "PermissionRequest": [
      {
        "matcher": "*",
        "note": "keep me",
        "hooks": [
          { "type": "command", "command": "mine.sh", "timeout": 5, "async": true },
          { "type": "command", "command": "'/old/notchling' hook", "timeout": 300 }
        ]
      },
      {
        "matcher": "*",
        "extra": 1,
        "hooks": [{ "type": "command", "command": "'/old/notchling' hook", "timeout": 300 }]
      }
    ]
  }
}
"#;
    let out = plan(text, &wanted(Want::Remove, Want::Leave));
    let v = value(&out.text);
    let groups = v["hooks"]["PermissionRequest"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["note"], "keep me");
    assert_eq!(groups[0]["hooks"].as_array().unwrap().len(), 1);
    assert_eq!(groups[0]["hooks"][0]["async"], true);
    // A group with a key of its own is kept even when it has no hook left.
    assert_eq!(groups[1]["extra"], 1);
    assert_eq!(groups[1]["hooks"], json!([]));
}

#[test]
fn a_group_of_only_ours_goes_and_an_event_and_hooks_left_empty_go_with_it() {
    let installed = plan(NO_HOOKS, &install_all()).text;
    let back = plan(&installed, &remove_all());
    assert_eq!(back.text, NO_HOOKS);
    assert!(value(&back.text).get("hooks").is_none());
}

#[test]
fn ours_in_another_place_is_reported_and_left_alone() {
    let text = r#"{
  "hooks": {
    "PostToolUse": [
      { "matcher": "Bash", "hooks": [{ "type": "command", "command": "'/x/notchling' hook" }] }
    ]
  }
}
"#;
    let out = plan(text, &remove_all());
    assert!(!out.changed);
    assert!(out
        .notes
        .iter()
        .any(|n| n.contains("PostToolUse (Bash)") && n.contains("left alone")));
    let found = detect(text).unwrap();
    assert_eq!(found.len(), 1);
    assert!(!found[0].managed);
}

#[test]
fn the_users_look_alike_commands_are_never_ours() {
    let text = r#"{"hooks":{"PreToolUse":[{"matcher":"Artifact","hooks":[
      {"type":"command","command":"echo hook"},
      {"type":"command","command":"notchling-lint hook"},
      {"type":"command","command":"my-notchling hook"},
      {"type":"command","command":"/x/notchling hooks"},
      {"type":"command","command":"/x/notchling hook extra-word"},
      {"type":"command"}, {"command": 5}, "text", 7, null
    ]}]}}"#;
    assert!(detect(text).unwrap().is_empty());
    let out = plan(text, &remove_all());
    assert!(!out.changed);
    let out = plan(text, &install_all());
    let v = value(&out.text);
    assert_eq!(
        v["hooks"]["PreToolUse"][0]["hooks"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
    assert_eq!(without_ours(v), value(text));
}

#[test]
fn a_wrapper_around_our_program_in_our_slot_is_the_users_and_is_never_removed() {
    let wrapped = [
        "tee /tmp/x | '/opt/notchling/bin/notchling' hook",
        "sh -c '/opt/notchling/bin/notchling hook'",
        "'/opt/notchling/bin/notchling' hook; rm -rf /tmp/x",
    ];
    for command in wrapped {
        let text = format!(
            r#"{{"hooks":{{"PermissionRequest":[{{"matcher":"*","hooks":[{{"type":"command","command":{}}}]}}]}}}}"#,
            serde_json::to_string(command).unwrap()
        );
        assert!(detect(&text).unwrap().is_empty(), "{command}");
        assert!(!plan(&text, &remove_all()).changed, "{command}");
        let out = plan(&text, &install_all());
        let v = value(&out.text);
        // Theirs stays where it was; ours goes in beside it, not over it.
        assert_eq!(
            v["hooks"]["PermissionRequest"][0],
            value(&text)["hooks"]["PermissionRequest"][0]
        );
        assert_eq!(ours(&v, "PermissionRequest").len(), 1, "{command}");
        assert_eq!(without_ours(v), value(&text), "{command}");
    }
}

#[test]
fn an_empty_file_becomes_an_object_with_our_entries_and_a_missing_one_is_the_same() {
    for blank in ["", "  \n", "\u{feff}"] {
        let out = plan(blank, &install_all());
        assert!(out.changed, "{blank:?}");
        let v = value(&out.text);
        assert_eq!(ours(&v, "PermissionRequest").len(), 1);
        assert!(out.text.starts_with("{\n  \"hooks\": {\n"), "{}", out.text);
        assert!(out.text.ends_with("}\n"));
        assert_eq!(without_ours(v), json!({}));
        let none = plan(blank, &remove_all());
        assert!(!none.changed);
        assert_eq!(none.text, blank);
    }
}

#[test]
fn odd_formatting_is_kept() {
    // Comments, a byte-order mark and trailing commas survive.
    let with_bom = format!("\u{feff}{ODD}");
    let out = plan(&with_bom, &install_all());
    assert!(out.text.starts_with('\u{feff}'));
    for kept in [
        "// my settings",
        "// the default",
        "/* hooks I wrote",
        "// lint first",
        "// trailing",
    ] {
        assert!(out.text.contains(kept), "{kept}\n{}", out.text);
    }
    let v = value(&out.text);
    assert_eq!(
        v["hooks"]["Notification"][0]["hooks"][0]["command"],
        "say done"
    );
    assert_eq!(
        v["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "lint.sh"
    );
    assert_eq!(ours(&v, "PreToolUse").len(), 2);
    let back = plan(&out.text, &remove_all());
    for kept in [
        "// my settings",
        "// the default",
        "/* hooks I wrote",
        "// lint first",
        "// trailing",
    ] {
        assert!(back.text.contains(kept), "{kept}\n{}", back.text);
    }

    // Tabs and CRLF.
    let tabs: String = NO_HOOKS.replace("  ", "\t");
    let out = plan(&tabs, &install_all());
    assert!(
        out.text
            .contains("\n\t\"hooks\": {\n\t\t\"PreToolUse\": [\n\t\t\t{\n"),
        "{}",
        out.text
    );
    assert_eq!(plan(&out.text, &remove_all()).text, tabs);
    let crlf = NO_HOOKS.replace('\n', "\r\n");
    let out = plan(&crlf, &install_all());
    assert!(!out.text.replace("\r\n", "").contains('\n'));
    assert_eq!(plan(&out.text, &remove_all()).text, crlf);

    // One line, no final newline.
    let out = plan(COMPACT, &install_all());
    assert!(!out.text.ends_with('\n'));
    assert_eq!(value(&out.text)["env"], json!({"A": "1"}));
    assert_eq!(plan(&out.text, &remove_all()).text, COMPACT);
}

#[test]
fn unknown_keys_and_odd_values_survive_every_change() {
    let out = plan(UNKNOWN_KEYS, &install_all());
    let v = value(&out.text);
    assert_eq!(v["hooks"]["FutureEvent"], json!({"not": "a list"}));
    assert_eq!(v["extraTopLevel"], json!([true, false, null]));
    assert_eq!(v["hooks"]["PermissionRequest"][0]["comment"], "mine");
    assert_eq!(
        v["hooks"]["PermissionRequest"][0]["hooks"][0]["statusMessage"],
        "asking"
    );
    assert_eq!(v["hooks"]["SessionStart"][0]["hooks"][0]["async"], true);

    // Numbers keep their spelling: the text outside the entries is untouched.
    let out = plan(USER_HOOKS, &install_all());
    for kept in [
        "\"RATIO\": 1.50",
        "12345678901234567890123",
        "\"SMALL\": 1e-7",
        "café",
        "naïve / ok",
    ] {
        assert!(out.text.contains(kept), "{kept}");
    }
}

#[test]
fn a_file_that_is_not_safe_to_edit_is_refused_and_the_message_holds_none_of_it() {
    let secret = "sk-ant-api03-DONTLEAK";
    let cases = [
        format!("{{\"env\": {{\"K\": \"{secret}\"}}, \"hooks\": {{"),
        format!("{{\"env\": {{\"K\": \"{secret}\"}} \"x\": 1}}"),
        format!("[\"{secret}\"]"),
        format!("\"{secret}\""),
        format!("{{\"k\": \"{secret}\"}} trailing"),
        format!("{{\"k\": \"{secret}\", \"hooks\": []}}"),
        format!("{{\"k\": \"{secret}\", \"hooks\": null}}"),
        format!("{{\"k\": \"{secret}\", \"hooks\": {{\"PreToolUse\": {{}}}}}}"),
        format!("{{\"k\": \"{secret}\", \"hooks\": {{\"PermissionRequest\": \"x\"}}}}"),
        format!("{{\"k\": \"{secret}\", \"hooks\": {{}}, \"hooks\": {{}}}}"),
        format!("{{\"k\": \"{secret}\", \"hooks\": {{\"PreToolUse\": [], \"PreToolUse\": []}}}}"),
    ];
    for text in &cases {
        for wanted in [install_all(), remove_all()] {
            let err = match plan_hooks(text, &wanted) {
                Err(e) => e,
                // A file we need not edit may still read: only the ones that need an edit fail.
                Ok(p) => {
                    assert!(!p.changed, "{text}");
                    continue;
                }
            };
            assert!(!err.0.contains(secret), "{err}");
        }
        // Installing always needs an edit, so it must be refused.
        assert!(plan_hooks(text, &install_all()).is_err(), "{text}");
        assert!(
            detect(text).is_err() || text.contains("\"hooks\""),
            "{text}"
        );
    }
}

#[test]
fn every_start_and_every_choice_ends_where_it_should() {
    // The edit is checked against the same change made to the data inside `plan_hooks`; this
    // runs it over every combination and checks the ends too.
    let choices = [Want::Leave, Want::Install, Want::Remove];
    for (name, text) in EVERY_FIXTURE {
        let starts = [
            text.to_string(),
            plan(text, &install_all()).text,
            plan(text, &wanted(Want::Install, Want::Leave)).text,
            plan(text, &wanted(Want::Leave, Want::Install)).text,
        ];
        for start in &starts {
            for prompt in choices {
                for pages in choices {
                    let w = wanted(prompt, pages);
                    let first = plan_hooks(start, &w)
                        .unwrap_or_else(|e| panic!("{name} {prompt:?} {pages:?}: {e}"));
                    assert_eq!(
                        without_ours(value(&first.text)),
                        without_ours(value(start)),
                        "{name} {prompt:?} {pages:?}"
                    );
                    let again = plan(&first.text, &w);
                    assert!(!again.changed, "{name} {prompt:?} {pages:?}");
                    let v = value(&first.text);
                    let count = |e: &str| ours(&v, e).len();
                    let state = |w: Want, had: usize, now: usize, what: &str| match w {
                        Want::Install => assert_eq!(now, had.max(1), "{what}"),
                        Want::Remove => assert_eq!(now, 0, "{what}"),
                        Want::Leave => assert_eq!(now, had, "{what}"),
                    };
                    let b = value(start);
                    let had = |e: &str| ours(&b, e).len();
                    state(pages, had("PostToolUse"), count("PostToolUse"), name);
                    state(
                        pages,
                        had("PostToolUseFailure"),
                        count("PostToolUseFailure"),
                        name,
                    );
                    state(
                        prompt,
                        had("PermissionRequest"),
                        count("PermissionRequest"),
                        name,
                    );
                }
            }
        }
    }
}
