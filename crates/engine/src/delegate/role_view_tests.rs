use super::*;
use serde_json::json;

fn live(models: &[&str]) -> Live {
    Live {
        models: models.iter().map(|m| (*m).to_owned()).collect(),
        hints: Vec::new(),
        efforts: Vec::new(),
    }
}

fn user(tag: &str, text: &str) -> Roles {
    let dir = std::env::temp_dir().join(format!("nl-roleview-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("catalog.json");
    std::fs::write(&file, text).unwrap();
    let roles = Roles::load(Some(&file));
    let _ = std::fs::remove_dir_all(&dir);
    roles
}

fn of<'a>(views: &'a [RoleView], role: &str) -> &'a RoleView {
    views.iter().find(|v| v.role == role).unwrap()
}

#[test]
fn the_four_roles_come_first_in_order_with_the_built_in_picks() {
    let built = Roles::built_in();
    let v = views(
        &built,
        &built,
        &live(&["gpt-6-astra"]),
        &live(&["gemini-4-hydrogen", "gemini-4-argon"]),
    );
    let names: Vec<&str> = v.iter().map(|r| r.role.as_str()).collect();
    assert_eq!(names, KNOWN_ROLES);
    let review = &of(&v, "review").choices;
    assert_eq!(review.len(), 1);
    assert_eq!(review[0].tool, "antigravity");
    assert_eq!(review[0].model.as_deref(), Some("gemini-4-hydrogen"));
    assert_eq!(review[0].effort, "high");
    assert_eq!(review[0].source, Source::BuiltIn);
    assert!(review[0].checked && review[0].note.is_none());
    assert!(review[0]
        .advice
        .as_deref()
        .unwrap()
        .contains("Review after each task"));
    let secrets = &of(&v, "review-secrets").choices;
    assert_eq!(
        secrets.iter().map(|c| c.tool).collect::<Vec<_>>(),
        ["codex", "antigravity"]
    );
}

#[test]
fn a_gone_model_gives_way_to_its_family_and_says_live() {
    let built = Roles::built_in();
    let v = views(
        &built,
        &built,
        &live(&["gpt-6-astra"]),
        &live(&["gemini-4.1-hydrogen", "gemini-4-argon"]),
    );
    let c = &of(&v, "review").choices[0];
    assert_eq!(c.marked, "gemini-4-hydrogen");
    assert_eq!(c.model.as_deref(), Some("gemini-4.1-hydrogen"));
    assert_eq!(c.source, Source::Live);
    assert!(c.note.as_deref().unwrap().contains("is gone"));
}

#[test]
fn a_newer_family_model_is_a_note_only() {
    let built = Roles::built_in();
    let v = views(
        &built,
        &built,
        &live(&["gpt-6-astra"]),
        &live(&["gemini-4-hydrogen", "gemini-4.1-hydrogen", "gemini-4-argon"]),
    );
    let c = &of(&v, "review").choices[0];
    assert_eq!(c.model.as_deref(), Some("gemini-4-hydrogen"));
    assert_eq!(c.source, Source::BuiltIn);
    assert_eq!(
        c.note.as_deref(),
        Some("newer in family: gemini-4.1-hydrogen")
    );
}

#[test]
fn nothing_live_in_the_family_leaves_no_model() {
    let built = Roles::built_in();
    let v = views(
        &built,
        &built,
        &live(&["gpt-6-astra"]),
        &live(&["gemini-4-flash"]),
    );
    let c = &of(&v, "review").choices[0];
    assert_eq!(c.model, None);
    assert!(c.checked);
}

#[test]
fn an_unknown_live_list_shows_the_marked_model_unchecked() {
    let built = Roles::built_in();
    let v = views(&built, &built, &live(&[]), &live(&[]));
    let c = &of(&v, "review-final").choices[0];
    assert_eq!(c.model.as_deref(), Some("gpt-6-astra"));
    assert!(!c.checked && c.note.is_none());
    assert_eq!(c.source, Source::BuiltIn);
}

#[test]
fn the_users_pick_is_marked_as_theirs_and_an_unlisted_role_follows() {
    let built = Roles::built_in();
    let roles = user(
        "mine",
        r#"{"codex": {"picks": [
            {"model": "gpt-6-astra", "effort": "high", "roles": ["review-final", "triage"]}]}}"#,
    );
    let v = views(&roles, &built, &live(&["gpt-6-astra"]), &live(&[]));
    let final_review = &of(&v, "review-final").choices[0];
    assert_eq!(final_review.effort, "high");
    assert_eq!(final_review.source, Source::Override);
    assert!(final_review.advice.is_none());
    assert_eq!(of(&v, "triage").choices.len(), 1);
    assert_eq!(v.last().unwrap().role, "triage");
    assert!(of(&v, "review-secrets")
        .choices
        .iter()
        .all(|c| c.tool == "antigravity"));
}

#[test]
fn a_role_the_user_emptied_stays_listed_with_no_choice() {
    let built = Roles::built_in();
    let roles = user(
        "empty",
        r#"{"antigravity": {"picks": [{"model": "gemini-4-hydrogen", "effort": "high", "roles": []}]}}"#,
    );
    let v = views(&roles, &built, &live(&[]), &live(&[]));
    assert!(of(&v, "review").choices.is_empty());
}

#[test]
fn json_carries_every_field() {
    let built = Roles::built_in();
    let v = views(
        &built,
        &built,
        &live(&["gpt-6-astra"]),
        &live(&["gemini-4-argon"]),
    );
    let j = to_json(&v);
    assert_eq!(j[0]["role"], "review");
    let spec = &j[3]["choices"][0];
    assert_eq!(spec["tool"], "antigravity");
    assert_eq!(spec["model"], "gemini-4-argon");
    assert_eq!(spec["source"], "built_in");
    assert_eq!(spec["checked"], true);
    assert_eq!(spec["note"], json!(null));
}

#[test]
fn a_model_the_catalog_says_is_not_for_the_job_is_never_shown_as_the_choice() {
    let built = Roles::built_in();
    let roles = user(
        "excluded",
        r#"{"antigravity": {"picks": [{"model": "gemini-4-flash", "effort": "high", "roles": ["review"]}]}}"#,
    );
    let v = views(
        &roles,
        &built,
        &live(&[]),
        &live(&["gemini-4-flash", "gemini-4-hydrogen"]),
    );
    let flash = of(&v, "review")
        .choices
        .iter()
        .find(|c| c.marked == "gemini-4-flash")
        .unwrap();
    assert_eq!(flash.model, None);
    assert_eq!(flash.source, Source::Override);
}

#[test]
fn an_excluded_pick_is_not_shown_even_when_the_live_list_is_unknown() {
    let built = Roles::built_in();
    let roles = user(
        "excluded-unchecked",
        r#"{"antigravity": {"picks": [{"model": "gemini-4-flash", "effort": "high", "roles": ["review"]},
            {"model": "gemini-4-argon", "effort": "ultra", "roles": ["review"]}]}}"#,
    );
    let v = views(&roles, &built, &live(&[]), &live(&[]));
    let choices = &of(&v, "review").choices;
    let flash = choices
        .iter()
        .find(|c| c.marked == "gemini-4-flash")
        .unwrap();
    assert_eq!(flash.model, None);
    assert!(!flash.checked);
    assert_eq!(
        flash.note.as_deref(),
        Some("gemini-4-flash is not for review")
    );
    let ultra = choices
        .iter()
        .find(|c| c.marked == "gemini-4-argon")
        .unwrap();
    assert_eq!(ultra.model, None, "an excluded effort counts too");
    let hydrogen = choices
        .iter()
        .find(|c| c.marked == "gemini-4-hydrogen")
        .unwrap();
    assert_eq!(hydrogen.model.as_deref(), Some("gemini-4-hydrogen"));
}

#[test]
fn no_model_is_never_tagged_live() {
    let built = Roles::built_in();
    let v = views(&built, &built, &live(&[]), &live(&["gemini-4-flash"]));
    let c = &of(&v, "review").choices[0];
    assert_eq!(c.model, None);
    assert_eq!(c.source, Source::BuiltIn);
    let roles = user(
        "excl-builtin",
        r#"{"antigravity": {"not_for": [{"match": "hydrogen", "roles": ["review"]}]}}"#,
    );
    let v = views(&roles, &built, &live(&[]), &live(&[]));
    let c = &of(&v, "review").choices[0];
    assert_eq!(c.model, None);
    assert_eq!(c.source, Source::BuiltIn);
}
