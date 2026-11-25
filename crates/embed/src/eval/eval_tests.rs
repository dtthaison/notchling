use super::*;
use notchling_engine::search::query::fuse::{rrf, List, K};
use notchling_engine::search::query::shape::text_only;
use std::collections::HashSet;

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn fuse(lists: &[&[String]]) -> Vec<String> {
    let lists: Vec<List> = lists.iter().map(|ids| List { ids, weight: 1.0 }).collect();
    rrf(&lists, K).into_iter().map(|(id, _)| id).collect()
}

#[test]
fn recall_at_one_and_five() {
    let rankings = vec![
        ids(&["a", "b", "c", "d", "e", "f"]),
        ids(&["x", "a", "c", "d", "e", "f"]),
        ids(&["x", "y", "z", "w", "v", "a"]),
        ids(&[]),
    ];
    let wants = vec![ids(&["a"]), ids(&["a"]), ids(&["a"]), ids(&["a"])];
    assert_eq!(recall(&rankings, &wants, 1), 0.25);
    assert_eq!(recall(&rankings, &wants, 5), 0.5);
    assert_eq!(recall(&rankings, &wants, 6), 0.75);
    let several = vec![ids(&["q", "b"])];
    assert_eq!(recall(&several, &[ids(&["a", "b"])], 5), 1.0);
    assert_eq!(recall(&several, &[ids(&["a", "b"])], 1), 0.0);
    assert_eq!(recall(&[], &[], 5), 0.0);
}

#[test]
fn bad_lines_are_reported_not_scored() {
    let known: HashSet<&str> = ["claude:one", "claude:two", "codex:three"].into();
    let text = [
        r#"{"q": "login", "want": ["claude:one"], "lang": "en"}"#,
        "",
        "not json at all",
        r#"{"q": "", "want": ["claude:one"]}"#,
        r#"{"q": "no want", "want": []}"#,
        r#"{"q": "ghost", "want": ["claude:one", "claude:gone"]}"#,
        r#"{"want": ["claude:two"]}"#,
        r#"{"q": "đăng nhập", "want": ["codex:three"]}"#,
    ]
    .join("\n");
    let (queries, bad) = parse_queries(&text, &|id| known.contains(id));
    assert_eq!(queries.len(), 2);
    assert_eq!(queries[0].q, "login");
    assert_eq!(queries[0].lang.as_deref(), Some("en"));
    assert_eq!(queries[1].lang, None);
    let lines: Vec<usize> = bad.iter().map(|b| b.line).collect();
    assert_eq!(lines, vec![3, 4, 5, 6, 7]);
    assert!(bad[3].why.contains("claude:gone"), "{}", bad[3].why);

    let exact = vec![false; queries.len()];
    let systems = vec![("text".to_string(), vec![ids(&["claude:one"]), ids(&["x"])])];
    let report = report(&queries, &exact, &systems);
    let all = report
        .rows
        .iter()
        .find(|(label, _)| label == "all/text")
        .expect("all row");
    assert_eq!(all.1.n, 2);
    assert_eq!(all.1.r1, 0.5);
}

#[test]
fn exact_group_uses_text_only() {
    let queries = vec![
        Query {
            q: "foo/bar.rs".to_string(),
            want: ids(&["d1"]),
            lang: Some("en".to_string()),
        },
        Query {
            q: "why does scrolling stall".to_string(),
            want: ids(&["d2"]),
            lang: Some("en".to_string()),
        },
    ];
    let exact: Vec<bool> = queries.iter().map(|q| text_only(&q.q)).collect();
    assert_eq!(exact, vec![true, false]);

    let text = ids(&["d1", "d3"]);
    let dense = ids(&["d9", "d2", "d1"]);
    assert_eq!(
        hybrid(true, &text, std::slice::from_ref(&dense), &fuse),
        text
    );
    let mixed = hybrid(false, &text, &[dense], &fuse);
    assert_ne!(mixed, text);
    assert!(mixed.contains(&"d2".to_string()));

    let grouped = groups(&queries, &exact);
    let names: Vec<&str> = grouped.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["all", "en", "exact"]);
    assert_eq!(grouped[2].1, vec![0]);

    let systems = vec![
        ("text".to_string(), vec![ids(&["d1"]), ids(&["d3"])]),
        (
            "text+m".to_string(),
            vec![
                hybrid(true, &ids(&["d1"]), &[ids(&["d5"])], &fuse),
                hybrid(false, &ids(&["d3"]), &[ids(&["d2"])], &fuse),
            ],
        ),
    ];
    let report = report(&queries, &exact, &systems);
    let at = |label: &str| {
        report
            .rows
            .iter()
            .find(|(l, _)| l == label)
            .expect(label)
            .1
            .clone()
    };
    assert_eq!(at("exact/text"), at("exact/text+m"));
    assert_eq!(at("exact/text").r1, 1.0);
}

#[test]
fn a_floor_of_zero_changes_nothing() {
    let list = vec![
        ("a".to_string(), 0.8f32),
        ("b".to_string(), 0.1),
        ("c".to_string(), -0.2),
    ];
    assert_eq!(above_floor(&list, 0.0), ids(&["a", "b", "c"]));
    assert_eq!(above_floor(&list, 0.5), ids(&["a"]));

    let wants = vec![ids(&["b"]), ids(&["a"])];
    let text = vec![ids(&["z"]), ids(&["a"])];
    let dense = vec![list.clone(), list];
    let exact = vec![false, false];
    let rows = sweep(&wants, &text, &dense, &exact, &fuse);
    assert_eq!(rows.len(), 19);
    assert_eq!(rows[0].floor, 0.0);
    let plain: Vec<Vec<String>> = text
        .iter()
        .zip(&dense)
        .map(|(t, d)| hybrid(false, t, &[above_floor(d, 0.0)], &fuse))
        .collect();
    assert_eq!(rows[0].r5, recall(&plain, &wants, 5));
    assert!((rows[18].floor - 0.9).abs() < 1e-6);
    assert_eq!(rows[0].meaning_only, 1);
    assert_eq!(rows[0].lost, 0.0);
    let last = &rows[rows.len() - 1];
    assert_eq!(last.lost, 1.0);
}
