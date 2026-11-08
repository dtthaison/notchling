use super::edit::*;
use super::hooks::plan_hooks;
use super::jsonc;
use super::render::Json;
use super::testutil::*;

fn item() -> Json {
    Json::obj(vec![
        ("matcher", Json::Str("Artifact".into())),
        ("n", Json::Num(7)),
    ])
}

fn arr(text: &str, path: &[Seg]) -> usize {
    let root = jsonc::parse(text).unwrap();
    let mut node = &root;
    for seg in path {
        node = match seg {
            Seg::Key(k) => node.get(k).unwrap(),
            Seg::At(i) => &node.items().unwrap()[*i],
        };
    }
    node.items()
        .map_or_else(|| node.members().unwrap().len(), <[_]>::len)
}

#[test]
fn an_element_is_added_on_its_own_lines_in_the_files_indent() {
    let text = "{\n  \"list\": [\n    1,\n    2\n  ]\n}\n";
    let out = append(text, &[key("list")], None, &item()).unwrap();
    assert_eq!(
        out,
        "{\n  \"list\": [\n    1,\n    2,\n    {\n      \"matcher\": \"Artifact\",\n      \"n\": 7\n    }\n  ]\n}\n"
    );
    assert_eq!(remove(&out, &[key("list")], 2).unwrap(), text);
}

#[test]
fn a_member_is_added_after_the_last_and_removed_again() {
    let text = "{\n  \"a\": 1,\n  \"b\": {\n    \"c\": 2\n  }\n}\n";
    let out = append(text, &[], Some("hooks"), &Json::Arr(vec![item()])).unwrap();
    assert!(out.contains("  },\n  \"hooks\": [\n    {\n"), "{out}");
    assert_eq!(remove(&out, &[], 2).unwrap(), text);
    let inner = append(text, &[key("b")], Some("d"), &Json::Num(3)).unwrap();
    assert_eq!(
        inner,
        "{\n  \"a\": 1,\n  \"b\": {\n    \"c\": 2,\n    \"d\": 3\n  }\n}\n"
    );
    assert_eq!(remove(&inner, &[key("b")], 1).unwrap(), text);
}

#[test]
fn an_empty_container_opens_up_in_a_pretty_file_and_stays_inline_in_a_compact_one() {
    let pretty = "{\n  \"hooks\": {}\n}\n";
    let out = append(pretty, &[key("hooks")], Some("Stop"), &Json::Arr(vec![])).unwrap();
    assert_eq!(out, "{\n  \"hooks\": {\n    \"Stop\": []\n  }\n}\n");
    assert_eq!(remove(&out, &[key("hooks")], 0).unwrap(), pretty);
    let compact = "{\"a\":[],\"b\":{}}";
    let out = append(compact, &[key("a")], None, &Json::Num(1)).unwrap();
    assert_eq!(out, "{\"a\":[1],\"b\":{}}");
    let out = append(&out, &[], Some("c"), &Json::Num(2)).unwrap();
    assert_eq!(out, "{\"a\":[1],\"b\":{},\"c\": 2}");
}

#[test]
fn an_empty_top_object_gets_its_first_member() {
    let out = append("{\n}\n", &[], Some("hooks"), &Json::Obj(vec![])).unwrap();
    assert_eq!(out, "{\n  \"hooks\": {}\n}\n");
    assert_eq!(remove(&out, &[], 0).unwrap(), "{}\n");
    // On one line it stays on one line.
    let out = append("{}\n", &[], Some("hooks"), &Json::Obj(vec![])).unwrap();
    assert_eq!(out, "{\"hooks\": {}}\n");
}

#[test]
fn inline_lists_keep_their_spacing() {
    assert_eq!(
        append("[1, 2]", &[], None, &Json::Num(3)).unwrap(),
        "[1, 2, 3]"
    );
    assert_eq!(
        append("[1,2]", &[], None, &Json::Num(3)).unwrap(),
        "[1,2,3]"
    );
    assert_eq!(remove("[1, 2, 3]", &[], 1).unwrap(), "[1, 3]");
    assert_eq!(remove("[1, 2, 3]", &[], 2).unwrap(), "[1, 2]");
    assert_eq!(remove("[1, 2, 3]", &[], 0).unwrap(), "[2, 3]");
    assert_eq!(remove("[ 1 ]", &[], 0).unwrap(), "[]");
}

#[test]
fn removal_in_a_pretty_list_takes_whole_lines_and_the_right_comma() {
    let text = "[\n  1,\n  2,\n  3\n]";
    assert_eq!(remove(text, &[], 0).unwrap(), "[\n  2,\n  3\n]");
    assert_eq!(remove(text, &[], 1).unwrap(), "[\n  1,\n  3\n]");
    assert_eq!(remove(text, &[], 2).unwrap(), "[\n  1,\n  2\n]");
    assert_eq!(remove("[\n  1\n]", &[], 0).unwrap(), "[]");
}

#[test]
fn trailing_commas_and_comments_stay_where_they_were() {
    let text = "[\n  1, // one\n  2, // two\n]";
    let out = append(text, &[], None, &Json::Num(3)).unwrap();
    assert_eq!(out, "[\n  1, // one\n  2, // two\n  3\n]");
    let out = remove(text, &[], 1).unwrap();
    assert_eq!(out, "[\n  1, // one\n]");
    let text = "[\n  1,\n  2 // two\n]";
    let out = append(text, &[], None, &Json::Num(3)).unwrap();
    assert_eq!(out, "[\n  1,\n  2, // two\n  3\n]");
    assert!(jsonc::parse(&out).is_ok());
    assert_eq!(remove(&out, &[], 2).unwrap(), "[\n  1,\n  2 // two\n]");
}

/// The numbers in the list at the top of `text`.
fn numbers(text: &str) -> Vec<u64> {
    let root = jsonc::parse(text).unwrap();
    let items = root.items().unwrap();
    items
        .iter()
        .map(|n| text[n.start..n.end].parse().unwrap())
        .collect()
}

#[test]
fn removing_an_item_keeps_every_comment_before_between_and_after_the_items() {
    let lists = [
        // Inline.
        "[/* before */ 1, /* between */ 2, /* more */ 3 /* after */]",
        "[ 1 , /* b1 */ 2 /* x */ , /* b2 */ 3 ]",
        "[1,/* b1 */2,/* b2 */3]",
        "[/* before */ 1, /* b1 */ 2, /* b2 */ 3, /* after */]",
        // The first item on the bracket's line, a comment ending that line.
        "[1, // one\n  // between\n  2,\n  /* b2 */ 3\n  // after\n]",
        // Every item on its own line, comments on lines of their own.
        "[\n  // before\n  1,\n  // between\n  2,\n  /* more\n     lines */\n  3\n  // after\n]",
        // The last item shares its line with the closing bracket.
        "[\n  1,\n  // between\n  2,\n  /* b2 */ 3 ]",
    ];
    for text in lists {
        let comments: Vec<&str> = [
            "/* before */",
            "/* between */",
            "/* more */",
            "/* after */",
            "// between",
            "// before",
            "// after",
            "/* b1 */",
            "/* b2 */",
            "/* x */",
            "/* more\n     lines */",
            "// one",
        ]
        .into_iter()
        .filter(|c| text.contains(c))
        .collect();
        assert!(!comments.is_empty(), "{text}");
        for index in 0..3 {
            let out = remove(text, &[], index).unwrap();
            let mut want = numbers(text);
            want.remove(index);
            assert_eq!(numbers(&out), want, "remove {index} of {text:?}: {out:?}");
            for c in &comments {
                assert!(
                    out.contains(c),
                    "remove {index} of {text:?} lost {c:?}: {out:?}"
                );
            }
        }
    }
}

#[test]
fn removing_our_group_keeps_the_users_comments_between_the_groups() {
    let ours = r#"{"matcher": "AskUserQuestion", "hooks": [{"type": "command", "command": "'/opt/notchling/bin/notchling' hook", "timeout": 300}]}"#;
    let theirs = r#"{"matcher": "Bash", "hooks": [{"type": "command", "command": "~/bin/x.sh"}]}"#;
    for text in [
        format!(r#"{{"hooks": {{"PreToolUse": [{ours}, /* mine */ {theirs}]}}}}"#),
        format!(r#"{{"hooks": {{"PreToolUse": [{theirs}, /* mine */ {ours}]}}}}"#),
        format!(r#"{{"hooks": {{"PreToolUse": [{theirs}, /* mine */ {ours}, /* too */]}}}}"#),
    ] {
        let out = plan_hooks(&text, &remove_all()).unwrap();
        assert!(out.changed, "{text}");
        assert!(out.text.contains("/* mine */"), "{}", out.text);
        assert_eq!(
            value(&out.text)["hooks"]["PreToolUse"],
            serde_json::json!([value(theirs)]),
            "{}",
            out.text
        );
    }
}

#[test]
fn removing_an_item_inline_takes_its_comma_and_the_space_after_it_only() {
    assert_eq!(
        remove("[1, /* a */ 2, 3]", &[], 0).unwrap(),
        "[/* a */ 2, 3]"
    );
    assert_eq!(
        remove("[1, /* a */ 2, 3]", &[], 1).unwrap(),
        "[1, /* a */ 3]"
    );
    assert_eq!(
        remove("[1, 2, /* z */ 3]", &[], 2).unwrap(),
        "[1, 2 /* z */ ]"
    );
    assert_eq!(remove("[1, 2, 3]", &[], 2).unwrap(), "[1, 2]");
}

#[test]
fn crlf_files_get_crlf_and_tabs_are_kept() {
    let text = "{\r\n\t\"list\": [\r\n\t\t1\r\n\t]\r\n}\r\n";
    let out = append(text, &[key("list")], None, &Json::Num(2)).unwrap();
    assert_eq!(out, "{\r\n\t\"list\": [\r\n\t\t1,\r\n\t\t2\r\n\t]\r\n}\r\n");
    assert_eq!(remove(&out, &[key("list")], 1).unwrap(), text);
    let out = append(text, &[], Some("k"), &Json::Arr(vec![Json::Num(1)])).unwrap();
    assert!(
        out.contains(",\r\n\t\"k\": [\r\n\t\t1\r\n\t]\r\n}"),
        "{out:?}"
    );
    assert!(!out.replace("\r\n", "").contains('\n'));
}

#[test]
fn removing_the_only_item_of_a_container_with_a_comment_keeps_the_comment() {
    let text = "{\n  \"a\": [\n    // keep me\n    1\n  ]\n}";
    let out = remove(text, &[key("a")], 0).unwrap();
    assert!(out.contains("// keep me"), "{out}");
    assert!(jsonc::parse(&out).is_ok(), "{out}");
}

#[test]
fn appending_beside_a_comment_only_container() {
    let text = "{\n  \"a\": [\n    // none yet\n  ]\n}";
    let out = append(text, &[key("a")], None, &Json::Num(1)).unwrap();
    assert!(out.contains("// none yet"), "{out}");
    assert_eq!(arr(&out, &[key("a")]), 1);
}

#[test]
fn bad_paths_and_shapes_are_errors() {
    let text = "{\"a\": [1], \"b\": 2}";
    assert!(append(text, &[key("zzz")], None, &Json::Num(1)).is_err());
    assert!(append(text, &[key("b")], None, &Json::Num(1)).is_err());
    assert!(append(text, &[key("a")], Some("k"), &Json::Num(1)).is_err());
    assert!(append(text, &[], None, &Json::Num(1)).is_err());
    assert!(remove(text, &[key("a")], 5).is_err());
    assert!(append("{", &[], None, &Json::Num(1)).is_err());
}

#[test]
fn every_fixture_takes_an_element_in_every_list_and_gives_it_back() {
    for (name, text) in [
        ("user", USER_HOOKS),
        ("none", NO_HOOKS),
        ("swift", SWIFT_INSTALLED),
        ("odd", ODD),
        ("compact", COMPACT),
        ("unknown", UNKNOWN_KEYS),
    ] {
        let root = jsonc::parse(text).unwrap();
        let mut paths = Vec::new();
        collect_lists(&root, &mut Vec::new(), &mut paths);
        assert!(!paths.is_empty() || name == "none", "{name}");
        for path in paths {
            let before = arr(text, &path);
            let out = append(text, &path, None, &item()).unwrap();
            assert_eq!(arr(&out, &path), before + 1, "{name} {path:?}");
            let back = remove(&out, &path, before).unwrap();
            if name == "odd" {
                // Comments may sit where the new item went; the data must come back.
                assert_eq!(value(&back), value(text), "{name} {path:?}");
            } else {
                assert_eq!(back, text, "{name} {path:?}");
            }
        }
    }
}

fn collect_lists(node: &jsonc::Node, at: &mut Vec<Seg>, out: &mut Vec<Vec<Seg>>) {
    match &node.kind {
        jsonc::Kind::Object(members) => {
            for m in members {
                at.push(key(&m.key));
                collect_lists(&m.value, at, out);
                at.pop();
            }
        }
        jsonc::Kind::Array(items) => {
            out.push(at.clone());
            for (i, n) in items.iter().enumerate() {
                at.push(Seg::At(i));
                collect_lists(n, at, out);
                at.pop();
            }
        }
        _ => {}
    }
}
