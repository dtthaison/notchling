use super::*;

fn v2_cache(prices: &str) -> String {
    let day = day_key(((NOW - 40.0 * DAY) / DAY).floor() as i64);
    serde_json::json!({
        "version": 2,
        "prices": prices,
        "files": {},
        "models": {&day: {"acct\tclaude-opus-5-5": 20.0}},
        "projects": {&day: {"acct\tdemo": 15.0, "acct\tother": 5.0}},
        "channels": {&day: {"acct\tClaude Code": 20.0}},
        "tokens": {&day: {"acct\tclaude-opus-5-5": [0.0, 1_000_000.0, 0.0, 0.0]}},
        "channel_tokens": {&day: {"acct\tClaude Code": 1_000_000.0}},
        "unpriced": {},
        "seen": {&day: ""}
    })
    .to_string()
}

#[test]
fn a_version_2_cache_moves_over_with_the_days_it_counted() {
    let tmp = TempDir::new("cost-v2");
    let data = tmp.path("data");
    store::put_doc(&data, CACHE_DOC, &v2_cache(&catalog().prices_hash())).unwrap();
    let mut t = claude_tracker(&tmp);
    t.refresh(NOW);
    let r = t.ranged(NOW);
    let s = &r.overall[&Window::All];
    near(s.month, 20.0);
    let projects: Vec<_> = s
        .projects
        .iter()
        .map(|p| (p.name.as_str(), p.cost))
        .collect();
    assert_eq!(projects, [("demo", 15.0), ("other", 5.0)]);
    assert_eq!(s.channels[0].name, CLAUDE_CODE);
    near(s.model_detail[0].output, 1_000_000.0);
    t.save_now(NOW);
    assert_eq!(Cache::load(&data, &catalog()).version, CURRENT);

    let tmp = TempDir::new("cost-v2-other-prices");
    store::put_doc(&tmp.path("data"), CACHE_DOC, &v2_cache("0000000000000000")).unwrap();
    let doubled = overridden(
        &tmp,
        r#"{"claude": {"prices": [{"match": "opus-5-5", "input": 4.0, "output": 40.0, "cache_read": 0.2}]}}"#,
    );
    let mut t = tracker_with(&tmp, doubled);
    t.refresh(NOW);
    near(month(&t.ranged(NOW), Window::All), 40.0);
}

fn doubled(tmp: &TempDir) -> Catalog {
    overridden(
        tmp,
        r#"{"claude": {"prices": [{"match": "opus-5-5", "input": 4.0, "output": 40.0, "cache_read": 0.2}]}}"#,
    )
}

#[test]
fn a_version_2_day_still_on_disk_is_repriced_once_whatever_its_project_split() {
    let tmp = TempDir::new("cost-v2-split");
    append(&tmp, "projects/-p/a.jsonl", &usage_line("m1", 20.0, NOW));
    let mut first = claude_tracker(&tmp);
    first.refresh(NOW);
    first.save_now(NOW);
    let text = store::doc(&tmp.path("data"), CACHE_DOC).unwrap().unwrap();
    let v3: serde_json::Value = serde_json::from_str(&text).unwrap();
    let today = day_key((NOW / DAY).floor() as i64);
    let key = v3["cells"][&today]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let parts: Vec<&str> = key.split('\t').collect();
    let (account, project) = (parts[0], parts[3]);
    let old = day_key(((NOW - 40.0 * DAY) / DAY).floor() as i64);
    let at = |name: &str| format!("{account}\t{name}");
    let model = at("claude-opus-5-5");
    let out = [0.0, 1_000_000.0, 0.0, 0.0];
    let mut seen = v3["seen"].clone();
    seen[&old] = "".into();
    let v2 = serde_json::json!({
        "version": 2,
        "prices": "0000000000000000",
        "files": v3["files"],
        "models": {&today: {&model: 20.0}, &old: {&model: 20.0}},
        "projects": {&today: {at(project): 10.0, at("other"): 10.0}, &old: {at("other"): 20.0}},
        "channels": {&today: {at(CLAUDE_CODE): 20.0}, &old: {at(CLAUDE_CODE): 20.0}},
        "tokens": {&today: {&model: out}, &old: {&model: out}},
        "channel_tokens": {&today: {at(CLAUDE_CODE): 1e6}, &old: {at(CLAUDE_CODE): 1e6}},
        "unpriced": {},
        "seen": seen
    });
    store::put_doc(&tmp.path("data"), CACHE_DOC, &v2.to_string()).unwrap();
    let mut t = tracker_with(&tmp, doubled(&tmp));
    t.refresh(NOW + 1.0);
    let r = t.ranged(NOW);
    let s = &r.overall[&Window::All];
    near(s.today, 40.0);
    let today_projects: Vec<_> = r.overall[&Window::Days(7)]
        .projects
        .iter()
        .map(|p| (p.name.as_str(), p.cost))
        .collect();
    assert_eq!(today_projects, [(project, 40.0)]);
    near(s.month, 80.0);
    near(s.model_detail[0].output, 2_000_000.0);

    t.save_now(NOW + 1.0);
    let mut again = tracker_with(&tmp, doubled(&tmp));
    again.refresh(NOW + 2.0);
    near(month(&again.ranged(NOW), Window::All), 80.0);
}
