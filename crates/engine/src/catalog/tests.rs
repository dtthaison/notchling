use super::*;
use std::path::PathBuf;

fn temp_file(tag: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-catalog-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("catalog.json");
    std::fs::write(&path, body).unwrap();
    path
}

fn live(id: &str) -> LiveModel {
    LiveModel {
        id: id.into(),
        ..Default::default()
    }
}

#[test]
fn every_current_and_older_claude_model_has_a_price() {
    let c = Catalog::built_in();
    let m = c.models(Tool::Claude, &[]);
    assert_eq!(m.len(), 13);
    for x in &m {
        assert!(
            c.price(Tool::Claude, &x.id).is_some(),
            "{} has no price",
            x.id
        );
    }
}

#[test]
fn prices_match_todays_tables() {
    let c = Catalog::built_in();
    assert_eq!(
        c.price(Tool::Claude, "claude-fable-5-2").unwrap().input,
        8.0
    );
    assert_eq!(
        c.price(Tool::Claude, "claude-fable-5-1").unwrap().output,
        50.0
    );
    assert_eq!(
        c.price(Tool::Claude, "claude-fable-5-3").unwrap().input,
        8.0
    );
    assert!(c.price_borrowed(Tool::Claude, "claude-fable-5-3"));
    assert!(!c.price_borrowed(Tool::Claude, "claude-fable-5-2"));
    assert_eq!(
        c.price(Tool::Claude, "claude-fable-4").unwrap().cache_read,
        1.0
    );
    assert_eq!(
        c.price(Tool::Codex, "gpt-5.1-2026-01-01").unwrap().input,
        1.25
    );
    assert!(c.price(Tool::Codex, "gpt-5.6-sol").is_none());
    assert!(c.price(Tool::Claude, "claude-new-9").is_none());
    assert!(c.price(Tool::Antigravity, "gemini-4").is_none());
    let haiku = c.price(Tool::Claude, "claude-haiku-5-5").unwrap();
    let over = haiku.over.unwrap();
    assert_eq!((over.tokens, over.input, over.output), (100000.0, 0.5, 2.5));
}

#[test]
fn suffixed_ids_are_the_bare_model() {
    let c = Catalog::built_in();
    for (suffixed, bare) in [
        ("claude-opus-5-5[1m]", "claude-opus-5-5"),
        ("claude-fable-5-2-20261101", "claude-fable-5-2"),
        ("claude-fable-5-2-2026-11-01", "claude-fable-5-2"),
    ] {
        assert_eq!(c.price(Tool::Claude, suffixed), c.price(Tool::Claude, bare));
        assert_eq!(
            c.context_window(Tool::Claude, suffixed),
            c.context_window(Tool::Claude, bare)
        );
    }
    assert!(c.is_allowed(Tool::Claude, &[], "claude-opus-5-5[1m]"));
    assert!(c.is_allowed(Tool::Claude, &[], "claude-fable-5-2-20261101"));
    assert!(!c.is_allowed(Tool::Claude, &[], "claude-evil-1"));
    assert!(!c.is_allowed(Tool::Claude, &[], "claude-evil-1[1m]"));
    assert_eq!(
        c.context_window(Tool::Claude, "claude-opus-5-5[1m]"),
        Some(400000)
    );
    let a = c.allowed(Tool::Claude, &[live("claude-opus-9[1m]")]);
    assert!(a.contains(&"claude-opus-9".to_string()));
    assert!(a.contains(&"claude-opus-9[1m]".to_string()));
    let l = [live("claude-opus-9[1m]")];
    assert!(c.is_allowed(Tool::Claude, &l, "claude-opus-9"));
    assert!(c.is_allowed(Tool::Claude, &l, "claude-opus-9-20261101"));
    assert!(!c.is_allowed(Tool::Claude, &[], "claude-opus-9"));
}

#[test]
fn an_id_is_allowed_only_in_the_shape_of_a_model_id() {
    let c = Catalog::built_in();
    for id in [
        "claude-opus-5-5",
        "claude-opus-5-5[1m]",
        "claude-opus-5-5-20261101",
    ] {
        assert!(c.is_allowed(Tool::Claude, &[], id), "{id}");
    }
    for id in [
        "claude-opus-5-5[x]",
        "claude-opus-5-5[1m] --x",
        "claude-opus-5-5[1m]\n",
        "claude-opus-5-5[1m]\n/permissions",
        "claude-opus-5-5 --x",
        "claude-opus-5-5[1m][1m]",
        "Claude-Opus-5-5",
        "-claude-opus-5-5",
        "--settings",
        "",
        "[1m]",
    ] {
        assert!(!c.is_allowed(Tool::Claude, &[], id), "{id:?}");
    }
    let l = [live("gpt-test")];
    assert!(c.is_allowed(Tool::Codex, &l, "gpt-test"));
    assert!(!c.is_allowed(Tool::Codex, &l, "gpt-test[1m]"));
    assert!(!c.is_allowed(Tool::Codex, &l, "gpt-test-20261101"));
}

#[test]
fn a_two_digit_version_is_not_another_models_entry() {
    let c = Catalog::built_in();
    for id in ["claude-fable-5-10", "claude-fable-5-19"] {
        let p = c.price(Tool::Claude, id).unwrap();
        assert_eq!((p.input, p.output), (8.0, 40.0), "{id}");
    }
    assert_eq!(
        c.price(Tool::Claude, "claude-mythos-5-10")
            .unwrap()
            .cache_read,
        1.0
    );
    assert!(c.price_borrowed(Tool::Claude, "claude-fable-5-10"));
    let path = temp_file(
        "tendigit",
        r#"{"claude": {"prices": [{"match": "fable-5-10", "input": 1.0, "output": 2.0, "cache_read": 0.1}]}}"#,
    );
    let c = Catalog::load(&path);
    assert_eq!(
        c.price(Tool::Claude, "claude-fable-5-10").unwrap().input,
        1.0
    );
    assert!(!c.price_borrowed(Tool::Claude, "claude-fable-5-10"));
}

#[test]
fn the_override_changes_a_price_but_cannot_allow_a_model() {
    let path = temp_file(
        "price",
        r#"{"claude": {"current": ["claude-evil-1"], "older": ["claude-evil-2"],
            "prices": [{"match": "fable-5-2", "input": 7.0, "output": 40.0, "cache_read": 0.2}]}}"#,
    );
    let c = Catalog::load(&path);
    assert_eq!(
        c.price(Tool::Claude, "claude-fable-5-2").unwrap().input,
        7.0
    );
    let allowed = c.allowed(Tool::Claude, &[]);
    assert!(!allowed.contains(&"claude-evil-1".to_string()));
    assert!(!allowed.contains(&"claude-evil-2".to_string()));
    assert!(c
        .models(Tool::Claude, &[])
        .iter()
        .all(|m| !m.id.contains("evil")));
}

#[test]
fn the_override_adds_prices_windows_and_names() {
    let path = temp_file(
        "add",
        r#"{"claude": {"windows": {"claude-fable-5-2": 1000000},
                       "names": {"claude-fable-5-2": "Fable Five Two"},
                       "prices": [{"match": "fable-5-3", "input": 1.0, "output": 2.0, "cache_read": 0.1}]},
            "codex": {"prices": [{"id": "GPT-7", "input": 3.0, "output": 6.0, "cache_read": 0.3}]}}"#,
    );
    let c = Catalog::load(&path);
    assert_eq!(
        c.price(Tool::Claude, "claude-fable-5-3-x").unwrap().input,
        1.0
    );
    assert_eq!(c.price(Tool::Codex, "gpt-7").unwrap().input, 3.0);
    assert_eq!(
        c.context_window(Tool::Claude, "claude-fable-5-2"),
        Some(1000000)
    );
    assert_eq!(
        c.context_window(Tool::Claude, "claude-opus-5-5"),
        Some(400000)
    );
    let m = c.models(Tool::Claude, &[]);
    assert_eq!(
        m.iter().find(|x| x.id == "claude-fable-5-2").unwrap().name,
        "Fable Five Two"
    );
    assert_eq!(
        m.iter().find(|x| x.id == "claude-opus-5-5").unwrap().name,
        "Opus 5.5"
    );
}

#[test]
fn a_bad_override_entry_is_skipped_alone() {
    let path = temp_file(
        "bad",
        r#"{"claude": {"windows": {"claude-opus-5-5": "big", "claude-sonnet-5-5": 500000},
            "prices": [{"match": "sonnet-5-5", "output": 9.0, "cache_read": 0.1},
                       {"match": "opus-5-5", "input": 3.0, "output": 20.0, "cache_read": 0.2},
                       "nonsense", 7]},
            "codex": 5}"#,
    );
    let c = Catalog::load(&path);
    assert_eq!(c.price(Tool::Claude, "claude-opus-5-5").unwrap().input, 3.0);
    assert_eq!(
        c.price(Tool::Claude, "claude-sonnet-5-5").unwrap().input,
        2.0
    );
    assert_eq!(
        c.context_window(Tool::Claude, "claude-opus-5-5"),
        Some(400000)
    );
    assert_eq!(
        c.context_window(Tool::Claude, "claude-sonnet-5-5"),
        Some(500000)
    );
    assert_eq!(c.price(Tool::Codex, "o3").unwrap().input, 2.0);
}

#[test]
fn a_missing_or_unreadable_override_is_the_built_in() {
    let built_in = Catalog::built_in().prices_hash();
    let missing = std::env::temp_dir()
        .join("nl-catalog-none")
        .join("catalog.json");
    assert_eq!(Catalog::load(&missing).prices_hash(), built_in);
    for body in ["", "not json", "[1, 2]", "{\"claude\": []}"] {
        let path = temp_file("junk", body);
        let c = Catalog::load(&path);
        assert_eq!(c.prices_hash(), built_in, "{body}");
        assert_eq!(c.models(Tool::Claude, &[]).len(), 13);
    }
}

#[test]
fn a_newer_family_model_from_the_live_list_becomes_current_and_borrows() {
    let live = [LiveModel {
        id: "claude-fable-5-3".into(),
        name: Some("Claude Fable 5.3".into()),
        ..Default::default()
    }];
    let m = Catalog::built_in().models(Tool::Claude, &live);
    let cur: Vec<&str> = m
        .iter()
        .filter(|x| x.tier == Tier::Current)
        .map(|x| x.id.as_str())
        .collect();
    assert_eq!(
        cur,
        [
            "claude-opus-5-5",
            "claude-fable-5-3",
            "claude-sonnet-5-5",
            "claude-haiku-5-5"
        ]
    );
    let f = m.iter().find(|x| x.id == "claude-fable-5-3").unwrap();
    assert!(f.borrowed);
    assert_eq!(f.borrowed_from.as_deref(), Some("claude-fable-5-2"));
    assert!(m
        .iter()
        .filter(|x| x.id != "claude-fable-5-3")
        .all(|x| x.borrowed_from.is_none()));
    assert_eq!(f.price.as_ref().unwrap().input, 8.0);
    assert_eq!(f.name, "Fable 5.3", "Anthropic's \"Claude \" is left off");
    assert_eq!(
        m.iter().find(|x| x.tier == Tier::Older).unwrap().id,
        "claude-fable-5-2"
    );
    assert_eq!(m.len(), 14);
}

#[test]
fn a_newer_model_borrows_the_window_of_its_place() {
    let m = Catalog::built_in().models(Tool::Claude, &[live("claude-opus-5-6")]);
    let o = m.iter().find(|x| x.id == "claude-opus-5-6").unwrap();
    assert_eq!(o.tier, Tier::Current);
    assert_eq!(o.context_window, Some(400000));
    assert_eq!(o.price.as_ref().unwrap().input, 4.0);
}

#[test]
fn an_older_live_model_stays_older() {
    let c = Catalog::built_in();
    let before = c.models(Tool::Claude, &[]).len();
    let m = c.models(
        Tool::Claude,
        &[live("claude-fable-5"), live("claude-new-9")],
    );
    let cur: Vec<&str> = m
        .iter()
        .filter(|x| x.tier == Tier::Current)
        .map(|x| x.id.as_str())
        .collect();
    assert_eq!(
        cur,
        [
            "claude-opus-5-5",
            "claude-fable-5-2",
            "claude-sonnet-5-5",
            "claude-haiku-5-5"
        ]
    );
    assert_eq!(m.len(), before + 1);
    let last = m.last().unwrap();
    assert_eq!((last.id.as_str(), last.tier), ("claude-new-9", Tier::Older));
    assert!(last.price.is_none());
    assert!(!last.borrowed);
}

#[test]
fn claude_efforts_are_the_list_except_for_haiku_4_5() {
    let m = Catalog::built_in().models(Tool::Claude, &[]);
    let efforts = |id: &str| m.iter().find(|x| x.id == id).unwrap().efforts.clone();
    assert_eq!(
        efforts("claude-opus-5-5"),
        ["low", "medium", "high", "xhigh", "max"]
    );
    assert!(efforts("claude-haiku-4-5").is_empty());
}

#[test]
fn other_tools_list_the_live_models_as_given() {
    let c = Catalog::built_in();
    assert!(c.models(Tool::Codex, &[]).is_empty());
    assert!(c.allowed(Tool::Codex, &[]).is_empty());
    let l = [
        LiveModel {
            id: "gpt-5.1".into(),
            name: Some("GPT-5.1".into()),
            context_window: Some(272000),
            efforts: vec!["low".into(), "high".into()],
            default_effort: Some("low".into()),
        },
        live("gpt-9"),
    ];
    let m = c.models(Tool::Codex, &l);
    assert_eq!(m.len(), 2);
    assert_eq!(m[0].name, "GPT-5.1");
    assert_eq!(m[0].tier, Tier::Current);
    assert_eq!(m[0].context_window, Some(272000));
    assert_eq!(m[0].efforts, ["low", "high"]);
    assert_eq!(m[0].default_effort.as_deref(), Some("low"));
    assert_eq!(m[0].price.as_ref().unwrap().input, 1.25);
    assert!(m[1].price.is_none());
    assert_eq!(c.allowed(Tool::Codex, &l), ["gpt-5.1", "gpt-9"]);
    assert_eq!(c.models(Tool::Antigravity, &l).len(), 2);
}

#[test]
fn the_prices_hash_changes_with_a_price_and_only_then() {
    let built_in = Catalog::built_in().prices_hash();
    assert_eq!(built_in.len(), 16);
    assert_eq!(built_in, Catalog::built_in().prices_hash());
    let same = temp_file(
        "hash-same",
        r#"{"claude": {"windows": {"claude-opus-5-5": 1}, "names": {"claude-opus-5-5": "X"},
            "prices": [{"match": "fable-5-2", "input": 8.0, "output": 40.0, "cache_read": 0.2}]}}"#,
    );
    assert_eq!(Catalog::load(&same).prices_hash(), built_in);
    let changed = temp_file(
        "hash-changed",
        r#"{"claude": {"prices": [{"match": "fable-5-2", "input": 8.5, "output": 40.0, "cache_read": 0.2}]}}"#,
    );
    assert_ne!(Catalog::load(&changed).prices_hash(), built_in);
    let added = temp_file(
        "hash-added",
        r#"{"antigravity": {"prices": [{"id": "gemini-4", "input": 1.0, "output": 2.0, "cache_read": 0.1}]}}"#,
    );
    assert_ne!(Catalog::load(&added).prices_hash(), built_in);
}

#[test]
fn model_key_strips_the_window_and_a_date() {
    assert_eq!(model_key("claude-opus-5-5[1m]"), "claude-opus-5-5");
    assert_eq!(model_key("claude-haiku-4-5-20251001"), "claude-haiku-4-5");
    assert_eq!(model_key("gpt-5.1-2026-01-01"), "gpt-5.1");
    assert_eq!(model_key("claude-opus-5-5"), "claude-opus-5-5");
    assert_eq!(model_key("o4-mini"), "o4-mini");
    assert_eq!(model_key("gpt-5-2026"), "gpt-5-2026");
}

#[test]
fn tool_parses_its_names() {
    assert_eq!(Tool::parse("claude"), Some(Tool::Claude));
    assert_eq!(Tool::parse("codex"), Some(Tool::Codex));
    assert_eq!(Tool::parse("antigravity"), Some(Tool::Antigravity));
    assert_eq!(Tool::parse("gemini"), None);
}

#[test]
fn an_override_price_keyed_by_the_full_claude_id_is_that_models() {
    let file = temp_file(
        "full-id",
        r#"{"claude": {"prices": [
            {"id": "claude-fable-5-3", "input": 9.0, "output": 45.0, "cache_read": 0.3},
            {"match": "Claude-Opus-5-5", "input": 6.0, "output": 30.0, "cache_read": 0.6}]}}"#,
    );
    let c = Catalog::load(&file);
    let fable = c.price(Tool::Claude, "claude-fable-5-3").unwrap();
    assert_eq!((fable.input, fable.output), (9.0, 45.0));
    assert!(!c.price_borrowed(Tool::Claude, "claude-fable-5-3"));
    assert_eq!(c.price(Tool::Claude, "claude-opus-5-5").unwrap().input, 6.0);
}

#[test]
fn a_negative_override_price_is_skipped_and_zero_is_a_price() {
    let file = temp_file(
        "negative",
        r#"{"claude": {"prices": [
            {"match": "opus-5-5", "input": -4.0, "output": 20.0, "cache_read": 0.2},
            {"match": "sonnet-5", "input": 2.0, "output": 10.0, "cache_read": 0.2,
             "over": {"tokens": 1000, "input": -1.0, "output": 1.0, "cache_read": 1.0}},
            {"match": "haiku-5-5", "input": 0.0, "output": 0.0, "cache_read": 0.0}]}}"#,
    );
    let c = Catalog::load(&file);
    let built_in = Catalog::built_in();
    assert_eq!(
        c.price(Tool::Claude, "claude-opus-5-5"),
        built_in.price(Tool::Claude, "claude-opus-5-5")
    );
    assert_eq!(
        c.price(Tool::Claude, "claude-sonnet-5-5"),
        built_in.price(Tool::Claude, "claude-sonnet-5-5")
    );
    let free = c.price(Tool::Claude, "claude-haiku-5-5").unwrap();
    assert_eq!((free.input, free.output, free.cache_read), (0.0, 0.0, 0.0));
}

#[test]
fn only_a_plain_release_takes_its_familys_place() {
    let c = Catalog::built_in();
    let models = c.models(
        Tool::Claude,
        &[live("claude-opus-6-preview"), live("claude-opus-5-6")],
    );
    let current: Vec<&str> = models
        .iter()
        .filter(|m| m.tier == Tier::Current)
        .map(|m| m.id.as_str())
        .collect();
    assert!(current.contains(&"claude-opus-5-6"), "{current:?}");
    assert!(!current.contains(&"claude-opus-6-preview"), "{current:?}");
    let preview = models
        .iter()
        .find(|m| m.id == "claude-opus-6-preview")
        .expect("listed");
    assert_eq!(preview.tier, Tier::Older);
    let alone = c.models(Tool::Claude, &[live("claude-opus-6-preview")]);
    assert!(alone
        .iter()
        .any(|m| m.id == "claude-opus-5-5" && m.tier == Tier::Current));
}

#[test]
fn anthropics_claude_prefix_is_left_off_names() {
    let c = Catalog::built_in();
    let mut fable = live("claude-fable-5-3");
    fable.name = Some("Claude Fable 5.3".into());
    let mut opus = live("claude-opus-5-5");
    opus.name = Some("Claude Opus 5.5".into());
    let models = c.models(Tool::Claude, &[fable, opus]);
    let name = |id: &str| {
        models
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.name.clone())
            .unwrap()
    };
    assert_eq!(name("claude-fable-5-3"), "Fable 5.3");
    assert_eq!(name("claude-opus-5-5"), "Opus 5.5");
}

#[test]
fn a_live_claude_id_that_is_empty_or_misshapen_is_dropped() {
    let c = Catalog::built_in();
    let live = [
        live(""),
        live("claude-evil-1 --flag"),
        live("Claude-Upper-1"),
        live("claude-fable-5-3"),
    ];
    let allowed = c.allowed(Tool::Claude, &live);
    assert!(!allowed.iter().any(|a| a.is_empty()), "{allowed:?}");
    assert!(!allowed.iter().any(|a| a.contains(' ') || a.contains('U')));
    assert!(allowed.contains(&"claude-fable-5-3".to_string()));
    let ids: Vec<String> = c
        .models(Tool::Claude, &live)
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert!(
        !ids.iter().any(|id| id.is_empty() || id.contains(' ')),
        "{ids:?}"
    );
    assert!(!c.is_allowed(Tool::Claude, &live, ""));
}
