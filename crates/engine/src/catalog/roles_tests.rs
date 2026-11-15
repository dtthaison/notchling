use super::*;
use serde_json::json;

#[test]
fn the_built_in_catalog_marks_the_review_models() {
    let r = Roles::built_in();
    let agy = r.tool("antigravity");
    assert_eq!(agy.picks_for("review")[0].model, "gemini-4-hydrogen");
    assert_eq!(agy.picks_for("review")[0].effort, "high");
    assert_eq!(agy.picks_for("spec-check")[0].model, "gemini-4-argon");
    assert_eq!(agy.picks_for("review-secrets")[0].model, "gemini-4-argon");
    let codex = r.tool("codex");
    assert_eq!(codex.picks_for("review-final")[0].model, "gpt-6-astra");
    assert_eq!(codex.picks_for("review-final")[0].effort, "xhigh");
    assert_eq!(codex.picks_for("review-secrets")[0].model, "gpt-6-astra");
    assert!(codex.picks_for("nothing").is_empty());
    assert_eq!(
        r.all_roles(),
        ["review-final", "review-secrets", "review", "spec-check"]
    );
}

#[test]
fn the_built_in_catalog_marks_what_is_not_for_review() {
    let r = Roles::built_in();
    let agy = r.tool("antigravity");
    for model in [
        "gemini-4-orchestrator",
        "gemini-4-flash",
        "claude-sonnet-5-5",
    ] {
        assert_eq!(agy.not_for(model), ["review"], "{model}");
    }
    assert!(agy.not_for("gemini-4-argon").is_empty());
    assert_eq!(r.tool("codex").not_for("gpt-6-terra"), ["review"]);
    assert_eq!(r.tool("codex").not_for("gpt-6-luna"), ["review"]);
    assert_eq!(
        agy.not_for_efforts(),
        [(None, "ultra".to_string(), vec!["review".to_string()])]
    );
}

#[test]
fn parsing_skips_bad_entries_and_old_files_have_no_roles() {
    let r = Roles::parse(&json!({
        "codex": {"prices": [], "picks": [
            {"model": "m", "roles": ["Review"]},
            {"model": "ok", "effort": "low", "roles": ["Review"], "use": "x"},
            {"model": "none", "effort": "low", "roles": []}
        ], "not_for": [{"roles": ["review"]}]}
    }));
    assert_eq!(r.tool("codex").picks.len(), 1);
    assert_eq!(r.tool("codex").picks[0].roles, ["review"]);
    assert!(r.tool("codex").avoids.is_empty());
    assert_eq!(
        Roles::parse(&json!({"codex": {"prices": []}})),
        Roles::default()
    );
}

fn temp(tag: &str, body: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-roles-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("catalog.json");
    std::fs::write(&path, body).unwrap();
    path
}

#[test]
fn an_override_adds_a_role_and_replaces_one_by_model_id() {
    let path = temp(
        "over",
        r#"{"antigravity":{"prices":[{"match":"x","input":1,"output":1,"cache_read":1}],"picks":[
            {"model":"gemini-4-argon","effort":"low","roles":["spec-check","triage"],"use":"cheap"},
            {"model":"gemini-9-new","effort":"high","roles":["review"]},
            {"effort":"high","roles":["x"]}],
          "not_for":[{"match":"flash","roles":[]},{"match":"terra","roles":["review"]}]}}"#,
    );
    let r = Roles::load(Some(&path));
    let agy = r.tool("antigravity");
    let argon = agy.picks_of("gemini-4-argon");
    assert_eq!(argon.len(), 1, "replaced, not appended to");
    assert_eq!(
        (argon[0].effort.as_str(), argon[0].note.as_str()),
        ("low", "cheap")
    );
    assert_eq!(argon[0].roles, ["spec-check", "triage"]);
    assert!(
        agy.picks_for("review-secrets").is_empty(),
        "argon lost its old roles"
    );
    assert_eq!(
        agy.picks_for("review").len(),
        2,
        "hydrogen and the new model"
    );
    assert!(
        agy.not_for("gemini-4-flash").is_empty(),
        "an empty not_for removes it"
    );
    assert_eq!(agy.not_for("gemini-4-terra"), ["review"]);
    assert_eq!(r.tool("codex"), Roles::built_in().tool("codex"));
}

#[test]
fn a_missing_or_broken_override_leaves_the_data_file() {
    let built = Roles::built_in();
    assert_eq!(Roles::load(None), built);
    assert_eq!(
        Roles::load(Some(std::path::Path::new("/no/such/catalog.json"))),
        built
    );
    assert_eq!(Roles::load(Some(&temp("bad", "{nope"))), built);
    assert_eq!(
        Roles::load(Some(&temp("odd", r#"{"codex":{"picks":"x","not_for":3}}"#))),
        built
    );
}

#[test]
fn an_entry_with_a_match_and_an_effort_excludes_only_that_pair() {
    let r = Roles::parse(&json!({"antigravity": {"not_for": [
        {"match": "hydrogen", "effort": "high", "roles": ["review"]},
        {"effort": "ultra", "roles": ["review"]},
        {"match": "flash", "roles": ["review"]}]}}));
    let agy = r.tool("antigravity");
    assert!(agy.excluded("gemini-4-hydrogen", "high", "review"));
    assert!(!agy.excluded("gemini-4-hydrogen", "low", "review"));
    assert!(!agy.excluded("gemini-4-argon", "high", "review"));
    assert!(!agy.excluded("gemini-4-hydrogen", "high", "spec-check"));
    assert!(
        agy.excluded("gemini-4-argon", "ULTRA", "review"),
        "any model at that effort"
    );
    assert!(
        agy.excluded("gemini-4-flash", "low", "review"),
        "any effort for that model"
    );
    assert_eq!(agy.not_for_efforts().len(), 2);
    assert!(
        agy.not_for("gemini-4-hydrogen").is_empty(),
        "not for it at every effort"
    );
}

#[test]
fn an_override_removes_a_pick_by_naming_only_the_model_and_empty_not_for_entries_are_dropped() {
    let path = temp(
        "remove",
        r#"{"antigravity":{"picks":[{"model":"gemini-4-hydrogen","roles":[]}]}}"#,
    );
    let r = Roles::load(Some(&path));
    assert!(r.tool("antigravity").picks_for("review").is_empty());
    assert_eq!(r.tool("antigravity").picks_for("spec-check").len(), 1);
    let parsed = Roles::parse(&json!({"codex": {"not_for": [{"effort": "ultra", "roles": []}]}}));
    assert!(parsed.tool("codex").avoids.is_empty());
}
