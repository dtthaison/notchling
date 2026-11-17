use super::*;
use crate::delegate::command::{models_listing, models_listing_json};
use crate::delegate::test_support::{rig, Rig};
use crate::tools::acp::fake::FakeSpawner as AcpFake;
use std::sync::Arc;

const CODEX: &str = r#"{"models":[
  {"slug":"gpt-6-astra","priority":1,"visibility":"list","default_reasoning_level":"medium",
   "supported_reasoning_levels":[{"effort":"medium"},{"effort":"xhigh"}]},
  {"slug":"gpt-6-terra","priority":2,"visibility":"list"}]}"#;

const AGY: &str = "gemini-4-hydrogen-high\tH\ngemini-4-argon-high\tA\ngemini-4-flash-low\tF\n";

fn with(codex: Option<&str>, agy: Option<&'static str>) -> Rig {
    let mut rig = rig(AcpFake::new());
    if let Some(cache) = codex {
        std::fs::create_dir_all(&rig.deps.codex_home).unwrap();
        std::fs::write(rig.deps.codex_home.join("models_cache.json"), cache).unwrap();
    }
    rig.deps.capture = Arc::new(move |_, _| agy.map(String::from));
    rig
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn pick(rig: &Rig, role: &str) -> (i32, String) {
    let mut out = Vec::new();
    let code = models_listing(&words(&["--for", role]), &rig.deps, &mut out);
    (code, String::from_utf8(out).unwrap())
}

#[test]
fn for_prints_each_marked_model_that_is_listed_now() {
    let rig = with(Some(CODEX), Some(AGY));
    assert_eq!(
        pick(&rig, "review"),
        (0, "agy --model gemini-4-hydrogen --effort high\n".into())
    );
    assert_eq!(
        pick(&rig, "review-final"),
        (0, "codex --model gpt-6-astra --effort xhigh\n".into())
    );
    assert_eq!(
        pick(&rig, "REVIEW-SECRETS"),
        (
            0,
            "codex --model gpt-6-astra --effort xhigh\nagy --model gemini-4-argon --effort high\n"
                .into()
        )
    );
    assert_eq!(
        pick(&rig, "spec-check"),
        (0, "agy --model gemini-4-argon --effort high\n".into())
    );
}

#[test]
fn nothing_live_in_the_family_gives_the_clear_message() {
    let rig = with(Some(CODEX), None);
    assert_eq!(
        pick(&rig, "review"),
        (1, "no model for review; run delegate-models\n".into())
    );
    let (code, text) = pick(&with(None, Some(AGY)), "review-secrets");
    assert_eq!(code, 0);
    assert_eq!(
        text,
        "agy --model gemini-4-hydrogen --effort high\nagy --model gemini-4-argon --effort high\n"
    );
    let other = "gemini-4-argon-high\tA\n";
    assert_eq!(
        pick(&with(Some(CODEX), Some(other)), "review").0,
        1,
        "argon is not hydrogen's family"
    );
}

#[test]
fn a_family_successor_is_used_and_named_when_the_marked_model_is_gone() {
    let agy = "gemini-4.1-hydrogen-high\tH\ngemini-4-argon-high\tA\n";
    let rig = with(Some(CODEX), Some(agy));
    assert_eq!(
        pick(&rig, "review"),
        (
            0,
            "agy --model gemini-4.1-hydrogen --effort high\nnote: gemini-4-hydrogen is gone; using gemini-4.1-hydrogen (same family, newer)\n"
                .into()
        )
    );
}

#[test]
fn a_newer_family_model_is_a_note_while_the_marked_one_is_live() {
    let agy = "gemini-4-hydrogen-high\tH\ngemini-4.1-hydrogen-high\tH\n";
    let rig = with(Some(CODEX), Some(agy));
    assert_eq!(
        pick(&rig, "review"),
        (
            0,
            "agy --model gemini-4-hydrogen --effort high\nnote: newer in family: gemini-4.1-hydrogen\n"
                .into()
        )
    );
}

#[test]
fn a_model_not_for_the_role_is_never_the_successor() {
    let agy = "gemini-4.1-hydrogen-high\tH\n";
    let mut rig = with(Some(CODEX), Some(agy));
    let file = rig.deps.catalog_file.clone().unwrap();
    std::fs::write(
        &file,
        r#"{"antigravity":{"not_for":[{"match":"4.1-hydrogen","roles":["review"]}]}}"#,
    )
    .unwrap();
    rig.deps.catalog_file = Some(file);
    assert_eq!(pick(&rig, "review").0, 1);
}

#[test]
fn the_providers_own_replacement_comes_first() {
    let cache = r#"{"models":[
      {"slug":"gpt-6.2-astra","priority":1,"visibility":"list"},
      {"slug":"gpt-6.1-astra","priority":2,"visibility":"list"},
      {"slug":"gpt-6.3-astra","priority":3,"visibility":"list"}]}"#;
    let rig = with(Some(cache), None);
    std::fs::write(
        rig.deps.codex_home.join("models_cache.json"),
        cache.replace(
            r#"{"slug":"gpt-6.1-astra","#,
            r#"{"slug":"gpt-6.1-astra","upgrade":{"model":"gpt-6.2-astra"},"#,
        ),
    )
    .unwrap();
    std::fs::write(
        rig.deps.catalog_file.clone().unwrap(),
        r#"{"codex":{"picks":[{"model":"gpt-6-astra","effort":"xhigh","roles":[]},{"model":"gpt-6.1-astra","effort":"xhigh","roles":["review-final"]}]}}"#,
    )
    .unwrap();
    assert_eq!(
        pick(&rig, "review-final"),
        (
            0,
            "codex --model gpt-6.1-astra --effort xhigh\nnote: gpt-6.2-astra replaces gpt-6.1-astra (the provider's hint)\n"
                .into()
        ),
        "marked and live: the hint is a note"
    );
    std::fs::write(
        rig.deps.codex_home.join("models_cache.json"),
        r#"{"models":[
          {"slug":"gpt-6.2-astra","visibility":"list"},
          {"slug":"gpt-6.3-astra","visibility":"list"},
          {"slug":"gpt-5-astra","visibility":"list","upgrade":"gpt-6.2-astra"}]}"#,
    )
    .unwrap();
    std::fs::write(
        rig.deps.catalog_file.clone().unwrap(),
        r#"{"codex":{"picks":[{"model":"gpt-6-astra","effort":"xhigh","roles":[]},{"model":"gpt-5-astra","effort":"xhigh","roles":["review-final"]}]}}"#,
    )
    .unwrap();
    let (code, text) = pick(&rig, "review-final");
    assert_eq!(code, 0);
    assert_eq!(
        text,
        "codex --model gpt-5-astra --effort xhigh\nnote: gpt-6.2-astra replaces gpt-5-astra (the provider's hint)\n"
    );
    std::fs::write(
        rig.deps.codex_home.join("models_cache.json"),
        r#"{"models":[
          {"slug":"gpt-6.2-astra","visibility":"list"},
          {"slug":"gpt-6.3-astra","visibility":"list"},
          {"slug":"gpt-5-astra","visibility":"hide","upgrade":"gpt-6.2-astra"}]}"#,
    )
    .unwrap();
    let (code, text) = pick(&rig, "review-final");
    assert_eq!(code, 0);
    assert_eq!(
        text,
        "codex --model gpt-6.2-astra --effort xhigh\nnote: gpt-5-astra is gone; using gpt-6.2-astra (the provider's replacement)\n",
        "the hint wins over the newer gpt-6.3-astra"
    );
}

#[test]
fn hints_read_a_slug_or_an_object_and_skip_nulls() {
    let hints = hints_of(
        br#"{"models":[
          {"slug":"a","upgrade":null},{"slug":"b","upgrade":"c"},
          {"slug":"d","upgrade":{"model":"e"}},{"slug":"f","upgrade":{"id":"f"}},{"slug":"g"}]}"#,
    );
    assert_eq!(
        hints,
        [
            ("b".to_string(), "c".to_string()),
            ("d".to_string(), "e".to_string())
        ]
    );
}

#[test]
fn the_override_file_adds_replaces_and_marks_a_model_only_the_live_list_reports() {
    let rig = with(Some(CODEX), Some("gemini-5-neon-high\tN\n"));
    let file = rig.deps.catalog_file.clone().unwrap();
    std::fs::write(
        &file,
        r#"{"antigravity":{"picks":[
            {"model":"gemini-5-neon","effort":"medium","roles":["review","spec-check"],"use":"mine"},
            {"model":"gemini-4-hydrogen","effort":"high","roles":[]}]}}"#,
    )
    .unwrap();
    assert_eq!(
        pick(&rig, "review"),
        (0, "agy --model gemini-5-neon --effort medium\n".into()),
        "added, and the compiled-in hydrogen role removed"
    );
    assert_eq!(
        pick(&rig, "spec-check").1,
        "agy --model gemini-5-neon --effort medium\n"
    );
    let mut out = Vec::new();
    models_listing(&[], &rig.deps, &mut out);
    assert!(String::from_utf8(out)
        .unwrap()
        .contains("gemini-5-neon  efforts: high  [review, spec-check at medium]\n    use: mine"));
    assert_eq!(
        pick(&rig, "review-final").1,
        "codex --model gpt-6-astra --effort xhigh\n",
        "codex entries not in the override stay"
    );
    std::fs::write(&file, "not json").unwrap();
    assert_eq!(
        pick(&rig, "review").0,
        1,
        "a bad file leaves the compiled-in data (hydrogen is not live)"
    );
}

#[test]
fn for_says_so_when_no_model_is_marked() {
    let rig = with(Some(CODEX), Some(AGY));
    let (code, text) = pick(&rig, "poetry");
    assert_eq!(code, 1);
    assert!(
        text.starts_with("no model marked for poetry; run delegate-models"),
        "{text}"
    );
    let mut out = Vec::new();
    let code = models_listing(&words(&["--for"]), &rig.deps, &mut out);
    assert_eq!(code, 2);
    assert!(String::from_utf8(out)
        .unwrap()
        .starts_with("--for needs a role: review-final"));
}

#[test]
fn the_text_listing_shows_roles_use_and_what_is_not_for_review() {
    let rig = with(Some(CODEX), Some(AGY));
    let mut out = Vec::new();
    assert_eq!(models_listing(&[], &rig.deps, &mut out), 0);
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("  gpt-6-astra  efforts: medium, xhigh (default medium)  [review-final, review-secrets at xhigh]\n    use: Final review of a branch"),
        "{text}"
    );
    assert!(text.contains("  gpt-6-terra  efforts: ") && text.contains("[not for review]"));
    assert!(text.contains(
        "  gemini-4-hydrogen  efforts: high  [review at high]\n    use: Review after each task"
    ));
    assert!(
        text.contains("  gemini-4-flash  efforts: low  [not for review]"),
        "{text}"
    );
    assert!(text.ends_with("  not for review: effort ultra\n"), "{text}");
}

#[test]
fn the_json_listing_carries_roles_use_and_not_for() {
    let rig = with(Some(CODEX), Some(AGY));
    let mut out = Vec::new();
    assert_eq!(models_listing_json(&[], &rig.deps, &mut out), 0);
    let v: Value = serde_json::from_slice(&out).unwrap();
    let codex = v["codex"]["models"].as_array().unwrap();
    assert_eq!(
        codex[0]["roles"][0],
        json!({"role": "review-final", "effort": "xhigh"})
    );
    assert_eq!(codex[0]["roles"][1]["role"], "review-secrets");
    assert!(codex[0]["use"]
        .as_str()
        .unwrap()
        .starts_with("Final review"));
    assert!(codex[0].get("notFor").is_none());
    assert_eq!(codex[1]["notFor"], json!(["review"]));
    let agy = v["agy"]["models"].as_array().unwrap();
    assert_eq!(
        agy[0]["roles"][0],
        json!({"role": "review", "effort": "high"})
    );
    assert_eq!(agy[2]["notFor"], json!(["review"]));
    assert_eq!(
        v["agy"]["notForEfforts"],
        json!([{"effort": "ultra", "notFor": ["review"]}])
    );
}

fn catalog(rig: &Rig, body: &str) {
    std::fs::write(rig.deps.catalog_file.clone().unwrap(), body).unwrap();
}

#[test]
fn a_model_marked_not_for_the_role_is_never_printed() {
    let agy = "gemini-4-hydrogen-high\tH\ngemini-4.1-hydrogen-high\tH\ngemini-4-argon-high\tA\n";
    let rig = with(Some(CODEX), Some(agy));
    catalog(
        &rig,
        r#"{"antigravity":{"not_for":[{"match":"gemini-4-hydrogen","roles":["review"]}]}}"#,
    );
    assert_eq!(
        pick(&rig, "review"),
        (
            0,
            "agy --model gemini-4.1-hydrogen --effort high\nnote: gemini-4-hydrogen is not for review; using gemini-4.1-hydrogen (same family, newer)\n"
                .into()
        ),
        "the marked model is excluded, so a family model takes over"
    );
    catalog(
        &rig,
        r#"{"antigravity":{"not_for":[{"match":"hydrogen","roles":["review"]}]}}"#,
    );
    assert_eq!(
        pick(&rig, "review"),
        (1, "no model for review; run delegate-models\n".into())
    );
}

#[test]
fn an_effort_or_a_hint_target_that_is_not_for_the_role_is_refused() {
    let rig = with(Some(CODEX), Some(AGY));
    catalog(
        &rig,
        r#"{"antigravity":{"picks":[{"model":"gemini-4-argon","effort":"ultra","roles":["review"]},{"model":"gemini-4-hydrogen","roles":[]}]}}"#,
    );
    assert_eq!(pick(&rig, "review").0, 1, "ultra is not for review");
    catalog(
        &rig,
        r#"{"antigravity":{"not_for":[{"match":"hydrogen","effort":"high","roles":["review"]}]}}"#,
    );
    assert_eq!(pick(&rig, "review").0, 1, "hydrogen at high is excluded");
    let cache = r#"{"models":[
      {"slug":"gpt-6-terra","visibility":"list"},
      {"slug":"gpt-5-astra","visibility":"hide","upgrade":"gpt-6-terra"}]}"#;
    let rig = with(Some(cache), None);
    catalog(
        &rig,
        r#"{"codex":{"picks":[{"model":"gpt-6-astra","effort":"xhigh","roles":[]},{"model":"gpt-5-astra","effort":"xhigh","roles":["review"]}]}}"#,
    );
    assert_eq!(
        pick(&rig, "review"),
        (1, "no model for review; run delegate-models\n".into()),
        "terra is not for review, even as the provider's replacement"
    );
}

#[test]
fn for_checks_its_arguments_and_can_print_json() {
    let rig = with(Some(CODEX), Some(AGY));
    let run = |args: &[&str]| {
        let mut out = Vec::new();
        let code = models_listing(&words(args), &rig.deps, &mut out);
        (code, String::from_utf8(out).unwrap())
    };
    let ok = (
        0,
        "agy --model gemini-4-hydrogen --effort high\n".to_string(),
    );
    assert_eq!(run(&["--for=review"]), ok);
    assert_eq!(run(&["--for", " Review "]), ok);
    assert_eq!(run(&["--for", ""]).0, 2);
    assert_eq!(run(&["--for="]).0, 2);
    assert_eq!(run(&["--for"]).0, 2);
    let (code, text) = run(&["--for", "review", "bogus"]);
    assert_eq!(
        (code, text.as_str()),
        (2, "--for takes only a role, not bogus\n")
    );
    let mut out = Vec::new();
    assert_eq!(
        models_listing_json(&words(&["--for", "review"]), &rig.deps, &mut out),
        0
    );
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v,
        json!({"role": "review", "picks": [{"tool": "agy", "model": "gemini-4-hydrogen", "effort": "high"}], "notes": []})
    );
    let mut out = Vec::new();
    assert_eq!(
        models_listing_json(&words(&["--for", "poetry"]), &rig.deps, &mut out),
        1
    );
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["picks"], json!([]));
    assert!(v["error"]
        .as_str()
        .unwrap()
        .starts_with("no model marked for poetry"));
}

#[test]
fn a_listed_variant_that_a_run_would_refuse_is_never_printed() {
    let agy = "gemini-4-hydrogen-xhigh\tX\ngemini-4-argon-high\tA\nclaude-opus-5-5-high\tC\n";
    let rig = with(Some(CODEX), Some(agy));
    for role in ["review", "review-final", "review-secrets", "spec-check"] {
        let (_, text) = pick(&rig, role);
        assert!(!text.contains("hydrogen-xhigh"), "{role}: {text}");
    }
    let (code, text) = pick(&rig, "review");
    assert_eq!(code, 1);
    assert_eq!(text, "no model for review; run delegate-models\n");
}
