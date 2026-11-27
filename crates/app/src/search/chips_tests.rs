use super::chips::*;
use notchling_engine::search::facets::FacetValue;
use notchling_engine::search::query::filter::Filter;
use notchling_engine::search::query::parse::parse;

fn value(value: &str, label: &str, count: usize) -> FacetValue {
    FacetValue {
        value: value.into(),
        label: label.into(),
        count,
    }
}

fn known() -> Known {
    let mut known = Known::default();
    known.set(
        "project",
        vec![
            value("/w/notchling", "notchling", 5),
            value("/w/notchling-site", "notchling-site", 3),
            value("/w/other", "other", 1),
        ],
    );
    known.set(
        "tool",
        vec![
            value("claude", "claude", 4),
            value("codex", "codex", 2),
            value("agy", "agy", 1),
        ],
    );
    known
}

fn labels(text: &str, known: &Known) -> Vec<(String, bool)> {
    chips(&parse(text), known)
        .into_iter()
        .map(|c| (c.label, c.known))
        .collect()
}

#[test]
fn an_unknown_value_sends_no_filter() {
    let known = known();
    let parsed = parse("fix project:nothing");
    assert_eq!(
        sent_filter(&parsed, &known, 1_800_000_000.0, 0),
        Filter::default()
    );
    assert_eq!(
        labels("project:nothing", &known),
        [("No Such Project".to_owned(), false)]
    );
    let parsed = parse("project:Notchling tool:codex");
    let filter = sent_filter(&parsed, &known, 1_800_000_000.0, 0);
    assert_eq!(filter.project, Some(vec!["/w/notchling".to_owned()]));
    assert_eq!(filter.tool, Some(vec!["codex".to_owned()]));
}

#[test]
fn a_value_not_loaded_yet_is_asked_for() {
    let parsed = parse("owner:bob project:other when:7d");
    assert_eq!(wanted(&parsed, &known()), ["owner"]);
}

#[test]
fn tab_completes_the_longest_common_prefix() {
    let known = known();
    assert_eq!(
        complete("fix project:no", &known).as_deref(),
        Some("fix project:notchling")
    );
    assert_eq!(
        complete("project:oth", &known).as_deref(),
        Some("project:other ")
    );
    assert_eq!(complete("project:zzz", &known), None);
    assert_eq!(complete("project:other", &known), None);
    assert_eq!(complete("fix the thing", &known), None);
}

#[test]
fn when_chips_are_labeled() {
    let known = Known::default();
    assert_eq!(
        labels("when:today when:7d when:2026-11 when:1d", &known),
        [
            ("Today".to_owned(), true),
            ("Last 7 Days".to_owned(), true),
            ("November 2026".to_owned(), true),
            ("Last Day".to_owned(), true),
        ]
    );
}

#[test]
fn a_tool_chip_reads_its_name_and_the_alias() {
    let known = known();
    assert_eq!(
        labels("tool:codex tool:antigravity", &known),
        [
            ("Tool: Codex".to_owned(), true),
            ("Tool: Antigravity".to_owned(), true)
        ]
    );
    let filter = sent_filter(&parse("tool:antigravity"), &known, 1_800_000_000.0, 0);
    assert_eq!(filter.tool, Some(vec!["agy".to_owned()]));
}

#[test]
fn a_category_chip_narrows_the_corpora() {
    let all = vec!["sessions", "pages"];
    assert_eq!(corpora(&parse("hello"), all.clone()), ["sessions", "pages"]);
    assert_eq!(corpora(&parse("p: hello"), all.clone()), ["pages"]);
    assert_eq!(corpora(&parse("c: hello"), all.clone()), ["sessions"]);
    assert_eq!(
        corpora(&parse("p: hello"), vec!["sessions"]),
        Vec::<&str>::new()
    );
}

#[test]
fn facet_values_come_from_the_corpora_that_have_them() {
    assert_eq!(corpora_for("tool"), ["sessions"]);
    assert_eq!(corpora_for("account"), ["sessions"]);
    assert_eq!(corpora_for("owner"), ["pages"]);
    assert_eq!(corpora_for("project"), ["sessions", "pages"]);
    assert!(corpora_for("state").is_empty());
}

#[test]
fn the_vault_is_skipped_for_a_filter_or_another_category() {
    let known = known();
    let allowed = |text: &str| {
        let parsed = parse(text);
        let sent = sent_filter(&parsed, &known, 1_800_000_000.0, 0);
        vault_allowed(&parsed, &sent)
    };
    assert!(allowed("token"));
    assert!(allowed("v: token"));
    assert!(allowed("token project:nothing"));
    assert!(!allowed("token tool:codex"));
    assert!(!allowed("token when:7d"));
    assert!(!allowed("p: token"));
}

fn shared_names() -> Known {
    let mut known = Known::default();
    known.set(
        "project",
        vec![
            value("/w/notchling", "notchling", 5),
            value("notchling", "notchling", 3),
            value("/x/notchling", "notchling", 1),
            value("/w/other", "other", 1),
        ],
    );
    known
}

#[test]
fn a_typed_name_sends_every_value_that_shares_it() {
    let known = shared_names();
    let sent = |text: &str| sent_filter(&parse(text), &known, 1_800_000_000.0, 0).project;
    assert_eq!(
        sent("project:notchling"),
        Some(vec![
            "/w/notchling".to_owned(),
            "notchling".to_owned(),
            "/x/notchling".to_owned()
        ])
    );
    assert_eq!(
        sent("project:/x/notchling"),
        Some(vec!["/x/notchling".to_owned()])
    );
    assert_eq!(
        sent("project:notchling project:other"),
        Some(vec![
            "/w/notchling".to_owned(),
            "notchling".to_owned(),
            "/x/notchling".to_owned(),
            "/w/other".to_owned()
        ])
    );
    assert_eq!(
        labels("project:notchling", &known),
        [("Project: notchling".to_owned(), true)]
    );
}

#[test]
fn a_category_nothing_searches_here_is_a_red_chip() {
    let known = Known::default();
    for (text, title) in [
        ("s: dark", "Settings"),
        ("i: ask", "Inbox"),
        ("a: run", "Actions"),
        ("m: box", "Machines"),
        ("activity: x", "Activity"),
    ] {
        assert_eq!(
            labels(text, &known),
            [(format!("Not Searched: {title}"), false)],
            "{text}"
        );
    }
    assert_eq!(labels("p: x", &known), [("Pages".to_owned(), true)]);
    assert_eq!(labels("v: x", &known), [("Vault".to_owned(), true)]);
    assert_eq!(labels("c: x", &known), [("Sessions".to_owned(), true)]);
}

#[test]
fn a_capped_list_still_asks_for_what_was_typed() {
    let mut known = known();
    let parsed = parse("project:notchling");
    assert!(unresolved(&parsed, &known).is_empty());
    known.mark_capped("project");
    assert_eq!(
        unresolved(&parsed, &known),
        [("project", "notchling".to_owned())]
    );
    known.mark_asked("project", "Notchling");
    assert!(unresolved(&parsed, &known).is_empty());
    let other = parse("project:other");
    assert_eq!(
        unresolved(&other, &known),
        [("project", "other".to_owned())]
    );
    assert!(unresolved(&parse("tool:codex"), &known).is_empty());
}

fn twins() -> Known {
    let mut known = Known::default();
    known.set(
        "project",
        vec![
            value("/w/a/app", "app", 4),
            value("/w/b/app", "app", 2),
            value("/w/other", "other", 1),
        ],
    );
    known
}

fn twins_sent(text: &str) -> Option<Vec<String>> {
    sent_filter(&parse(text), &twins(), 1_800_000_000.0, 0).project
}

#[test]
fn a_bare_name_two_projects_share_still_sends_both() {
    assert_eq!(
        twins_sent("project:app"),
        Some(vec!["/w/a/app".to_owned(), "/w/b/app".to_owned()])
    );
    assert_eq!(
        twins_sent("project:APP"),
        Some(vec!["/w/a/app".to_owned(), "/w/b/app".to_owned()])
    );
    assert_eq!(
        labels("project:app", &twins()),
        [("Project: app".to_owned(), true)]
    );
}

#[test]
fn a_path_suffix_narrows_to_one_project() {
    let only_b = Some(vec!["/w/b/app".to_owned()]);
    for typed in [
        "project:b/app",
        "project:B/App",
        "project:w/b/app",
        "project:/w/b/app",
        "project:/w/b/app/",
        "project:b/app/",
    ] {
        assert_eq!(twins_sent(typed), only_b, "{typed}");
    }
    assert_eq!(twins_sent("in:a/app"), Some(vec!["/w/a/app".to_owned()]));
    for typed in [
        "project:c/app",
        "project:b/ap",
        "project:/b/app",
        "project:w/app",
    ] {
        assert_eq!(twins_sent(typed), None, "{typed}");
    }
}

#[test]
fn a_chip_names_the_parent_when_one_project_of_a_shared_name_is_meant() {
    let known = twins();
    assert_eq!(
        labels("project:b/app", &known),
        [("Project: b/app".to_owned(), true)]
    );
    assert_eq!(
        labels("project:/w/a/app", &known),
        [("Project: a/app".to_owned(), true)]
    );
    assert_eq!(
        labels("project:other", &known),
        [("Project: other".to_owned(), true)]
    );
    assert_eq!(
        labels("project:c/app", &known),
        [("No Such Project".to_owned(), false)]
    );
}

#[test]
fn the_parent_grows_until_the_names_differ() {
    let mut known = Known::default();
    known.set(
        "project",
        vec![
            value("/w/x/lib/app", "app", 3),
            value("/w/y/lib/app", "app", 2),
            value("/w/z/app", "app", 1),
        ],
    );
    let sent = |text: &str| sent_filter(&parse(text), &known, 1_800_000_000.0, 0).project;
    assert_eq!(
        labels("project:x/lib/app", &known),
        [("Project: x/lib/app".to_owned(), true)]
    );
    assert_eq!(
        labels("project:z/app", &known),
        [("Project: z/app".to_owned(), true)]
    );
    assert_eq!(sent("project:lib/app").map(|v| v.len()), Some(2));
    assert_eq!(
        sent("project:y/lib/app"),
        Some(vec!["/w/y/lib/app".to_owned()])
    );
}

#[test]
fn tab_offers_the_parent_folder_of_a_shared_name() {
    let known = twins();
    let tab = |text: &str| complete(text, &known);
    assert_eq!(tab("project:b/").as_deref(), Some("project:b/app "));
    assert_eq!(tab("project:a/a").as_deref(), Some("project:a/app "));
    assert_eq!(tab("project:a").as_deref(), Some("project:a/app "));
    assert_eq!(tab("fix in:b/ap").as_deref(), Some("fix in:b/app "));
    assert_eq!(tab("project:ap").as_deref(), Some("project:app"));
    assert_eq!(tab("project:oth").as_deref(), Some("project:other "));
    assert_eq!(tab("project:c/"), None);
}

#[test]
fn tab_walks_through_the_projects_that_share_a_name() {
    let known = twins();
    let tab = |text: &str| complete(text, &known);
    assert_eq!(tab("project:app").as_deref(), Some("project:a/app"));
    assert_eq!(tab("project:a/app").as_deref(), Some("project:b/app"));
    assert_eq!(tab("project:b/app").as_deref(), Some("project:a/app"));
    assert_eq!(tab("project:other"), None);
}

#[test]
fn tab_never_offers_the_bare_name_when_other_projects_match_too() {
    let mut known = twins();
    known.add("project", vec![value("/w/apple", "apple", 1)]);
    assert_eq!(complete("project:ap", &known), None);
    assert_eq!(
        complete("project:appl", &known).as_deref(),
        Some("project:apple ")
    );
}

#[test]
fn a_typed_suffix_of_a_capped_list_is_asked_for_by_its_last_part() {
    let mut known = twins();
    known.mark_capped("project");
    assert_eq!(
        unresolved(&parse("project:b/app"), &known),
        [("project", "app".to_owned())]
    );
    known.mark_asked("project", "app");
    assert!(unresolved(&parse("project:b/app"), &known).is_empty());
    assert!(unresolved(&parse("project:app"), &known).is_empty());
}
