use crate::choose::{choose_among, match_tag, Choice, Inputs};
use crate::format::{Body, Table};
use crate::state::Active;
use crate::Text;

const SHIPPED: [&str; 5] = ["en", "de", "pt", "pt-BR", "zh-Hans"];

type Row = (
    Option<&'static str>,
    Option<&'static str>,
    &'static [&'static str],
    &'static str,
);

fn system(tags: &[&str]) -> Vec<String> {
    tags.iter().map(|t| t.to_string()).collect()
}

fn pick(env: Option<&str>, setting: Option<&str>, tags: &[&str]) -> Choice {
    let list = system(tags);
    choose_among(
        &Inputs {
            env,
            setting,
            system: &list,
            region: None,
            mark_fallback: false,
        },
        &SHIPPED,
    )
}

#[test]
fn chooses_over_preferred_lists() {
    let rows: [Row; 13] = [
        (Some("de"), Some("pt-BR"), &["fr"], "de"),
        (None, Some("pt-BR"), &["de"], "pt-BR"),
        (None, None, &["fr-FR", "de-AT"], "de"),
        (None, None, &["zh-Hans-CN"], "zh-Hans"),
        (None, None, &["pt_BR"], "pt-BR"),
        (None, None, &["pt-PT"], "pt"),
        (None, None, &["fr"], "en"),
        (None, None, &[], "en"),
        (None, Some("system"), &["de"], "de"),
        (Some("xx"), None, &["de"], "de"),
        (None, None, &["de-u-nu-latn"], "de"),
        (None, None, &["PT-br"], "pt-BR"),
        (Some(""), Some(""), &["zh-Hant"], "en"),
    ];
    for (env, setting, tags, want) in rows {
        assert_eq!(
            pick(env, setting, tags).tag,
            want,
            "{env:?} {setting:?} {tags:?}"
        );
    }
}

#[test]
fn the_pseudo_language_is_english_marked() {
    let choice = pick(Some("en-XA"), None, &["de"]);
    assert_eq!(choice.tag, "en");
    assert!(choice.pseudo);
    assert!(!pick(Some("de"), None, &[]).pseudo);
}

#[test]
fn a_region_comes_from_the_setting_or_the_tag() {
    assert_eq!(pick(None, None, &["en-DE"]).region.as_deref(), Some("DE"));
    assert_eq!(pick(None, None, &["en-DE"]).tag, "en");
    assert_eq!(pick(None, None, &["de"]).region, None);
    let list = system(&["de-AT"]);
    let choice = choose_among(
        &Inputs {
            env: None,
            setting: None,
            system: &list,
            region: Some("us"),
            mark_fallback: true,
        },
        &SHIPPED,
    );
    assert_eq!(choice.region.as_deref(), Some("US"));
    assert!(choice.mark_fallback);
}

#[test]
fn matching_tries_exact_then_script_then_language() {
    assert_eq!(match_tag("zh-Hans-CN", &SHIPPED), Some("zh-Hans"));
    assert_eq!(match_tag("zh-Hant", &SHIPPED), None);
    assert_eq!(match_tag("pt-BR", &SHIPPED), Some("pt-BR"));
    assert_eq!(match_tag("de_DE.UTF-8", &SHIPPED), Some("de"));
    assert_eq!(match_tag("", &SHIPPED), None);
}

static EN_BODIES: [Body; 2] = [Body::Plain("one"), Body::Plain("two")];
static PT_BODIES: [Option<Body>; 2] = [Some(Body::Plain("um-pt")), None];
static PT_BR_BODIES: [Option<Body>; 2] = [None, None];
static PT_BR_OWN: [Option<Body>; 2] = [Some(Body::Plain("um-br")), None];
static TABLES: [(&str, Table); 3] = [
    ("en", Table::Full(&EN_BODIES)),
    ("pt", Table::Part(&PT_BODIES)),
    ("pt-BR", Table::Part(&PT_BR_BODIES)),
];
static TABLES_OWN: [(&str, Table); 3] = [
    ("en", Table::Full(&EN_BODIES)),
    ("pt", Table::Part(&PT_BODIES)),
    ("pt-BR", Table::Part(&PT_BR_OWN)),
];

fn choice(tag: &'static str, mark_fallback: bool) -> Choice {
    Choice {
        tag,
        pseudo: false,
        region: None,
        mark_fallback,
    }
}

#[test]
fn fallback_chain_parent_then_english() {
    let active = Active::with_tables(&choice("pt-BR", false), &TABLES);
    assert_eq!(active.get(Text(0)), "um-pt");
    assert_eq!(active.get(Text(1)), "two");
}

#[test]
fn a_message_that_fell_back_can_be_marked() {
    let active = Active::with_tables(&choice("pt-BR", true), &TABLES);
    assert_eq!(active.get(Text(0)), "\u{2039}um-pt\u{203a}");
    assert_eq!(active.get(Text(1)), "\u{2039}two\u{203a}");
    let a = active.get(Text(1));
    let b = active.get(Text(1));
    assert!(std::ptr::eq(a, b));
    let own = Active::with_tables(&choice("pt-BR", true), &TABLES_OWN);
    assert_eq!(own.get(Text(0)), "um-br");
}

#[test]
fn english_never_marks() {
    let active = Active::with_tables(&choice("en", true), &TABLES);
    assert_eq!(active.get(Text(0)), "one");
}

#[test]
fn a_message_out_of_range_is_empty() {
    let active = Active::with_tables(&choice("en", false), &TABLES);
    assert_eq!(active.get(Text(40)), "");
}
