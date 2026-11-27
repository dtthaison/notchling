use super::find;
use super::quota_tests::{card, codex, pick, put, room, set, setup, AGY};
use crate::catalog::roles::Roles;
use crate::delegate::test_support::Rig;
use crate::providers::usage::Reached;
use std::sync::Arc;

const REVIEW_ROLES: [&str; 3] = ["review", "review-final", "review-secrets"];

#[test]
fn a_model_marked_not_for_review_is_not_for_the_review_roles_either() {
    let roles = Roles::built_in();
    let agy = roles.tool("antigravity");
    for role in REVIEW_ROLES {
        assert!(agy.excluded("claude-opus-5-5", "high", role), "{role}");
        assert!(agy.excluded("gemini-4-hydrogen", "ultra", role), "{role}");
        assert!(!agy.excluded("gemini-4-hydrogen", "high", role), "{role}");
    }
    assert!(!agy.excluded("claude-opus-5-5", "high", "spec-check"));
    assert!(!agy.excluded("claude-opus-5-5", "high", "reviewer"));
    assert!(roles
        .tool("codex")
        .excluded("gpt-6-terra", "high", "review-final"));
}

#[test]
fn no_ladder_in_the_built_in_catalog_names_a_model_its_own_not_for_list_refuses() {
    let roles = Roles::built_in();
    for name in ["codex", "antigravity"] {
        for pick in &roles.tool(name).picks {
            for ladder in &pick.then {
                for step in &ladder.steps {
                    let tool = if step.tool == "codex" {
                        "codex"
                    } else {
                        "antigravity"
                    };
                    assert!(
                        !roles
                            .tool(tool)
                            .excluded(&step.model, &step.effort, &ladder.role),
                        "{} then {} for {}",
                        pick.model,
                        step.model,
                        ladder.role
                    );
                }
            }
        }
    }
}

#[test]
fn the_second_secrets_reviewer_is_never_a_model_marked_not_for_review() {
    let rig = setup();
    put(&rig, "antigravity", &room());
    put(&rig, "codex", &codex(Some(Reached::Credits)));
    assert_eq!(
        pick(&rig, "review-secrets"),
        "agy --model gemini-4-hydrogen --effort high\nagy --model gemini-4-argon --effort high\nnote: gpt-6-astra: out of credits\n"
    );
}

#[test]
fn with_no_other_model_that_has_room_the_secrets_review_has_one_pick_and_says_why() {
    let rig = setup();
    put(&rig, "antigravity", &card());
    put(&rig, "codex", &codex(Some(Reached::Credits)));
    assert_eq!(
        pick(&rig, "review-secrets"),
        "agy --model gemini-4-hydrogen --effort high\nnote: gpt-6-astra: out of credits\nnote: gemini-4-argon: week quota used up, resets in 7d 0h\nnote: no second model for review-secrets: no other model for it has room\n"
    );
}

#[test]
fn the_second_secrets_reviewer_is_not_the_first() {
    let rig = setup();
    let mut q = room();
    set(&mut q, "Gemini 4 Argon", "Week", 100.0);
    put(&rig, "antigravity", &q);
    put(&rig, "codex", &codex(Some(Reached::Credits)));
    let text = pick(&rig, "review-secrets");
    let picks: Vec<&str> = text.lines().filter(|l| !l.starts_with("note:")).collect();
    assert_eq!(
        picks,
        vec!["agy --model gemini-4-hydrogen --effort high"],
        "{text}"
    );
}

#[test]
fn a_review_model_of_the_same_tool_that_the_user_marked_stands_in_for_a_second_reviewer() {
    let mut rig = setup();
    rig.deps.capture = Arc::new(|_, _| Some(format!("{AGY}gemini-4-neon-high\tN\n")));
    std::fs::write(
        rig.deps.catalog_file.clone().unwrap(),
        r#"{"antigravity":{"picks":[{"model":"gemini-4-neon","effort":"high","roles":["review"]}]}}"#,
    )
    .unwrap();
    let mut q = room();
    set(&mut q, "Gemini 4 Argon", "Week", 100.0);
    put(&rig, "antigravity", &q);
    put(&rig, "codex", &codex(Some(Reached::Credits)));
    assert_eq!(
        pick(&rig, "review-secrets"),
        "agy --model gemini-4-hydrogen --effort high\nagy --model gemini-4-neon --effort high\nnote: gpt-6-astra: out of credits\nnote: gemini-4-argon: week quota used up, resets in 7d 0h\n"
    );
}

#[test]
fn no_review_role_ever_names_a_not_for_review_model_or_the_same_model_twice() {
    let rig = setup();
    let mut states = Vec::new();
    for credits in [false, true] {
        for card_state in 0..4 {
            let mut q = room();
            match card_state {
                1 => set(&mut q, "Gemini 4 Argon", "Week", 100.0),
                2 => set(&mut q, "Gemini Models", "Week", 100.0),
                3 => set(&mut q, "Claude and GPT models", "5h", 100.0),
                _ => {}
            }
            states.push((credits, q));
        }
    }
    states.push((false, card()));
    states.push((true, card()));
    for (credits, q) in states {
        put(&rig, "antigravity", &q);
        put(&rig, "codex", &codex(credits.then_some(Reached::Credits)));
        for role in REVIEW_ROLES {
            let text = pick(&rig, role);
            let picks: Vec<&str> = text.lines().filter(|l| !l.starts_with("note:")).collect();
            assert!(
                picks.iter().all(|l| !l.contains("claude")),
                "{role}: {text}"
            );
            let mut seen = picks.clone();
            seen.sort();
            seen.dedup();
            assert_eq!(seen.len(), picks.len(), "{role}: {text}");
        }
    }
}

const USER_FILE: &str = r#"{"antigravity":{"picks":[
  {"model":"gemini-4-carbon","effort":"high","roles":["review-secrets","review-final","spec-check"]},
  {"model":"gemini-4-argon","effort":"high","roles":["review-secrets"]}]}}"#;

fn with_user_file(rig: &mut Rig, file: &str) {
    rig.deps.capture = Arc::new(|_, _| {
        Some(format!(
            "{AGY}gemini-4-carbon-high\tK\ngemini-4-neon-high\tN\n"
        ))
    });
    std::fs::write(rig.deps.catalog_file.clone().unwrap(), file).unwrap();
}

fn lines_of(text: &str) -> (Vec<&str>, Vec<&str>) {
    text.lines().partition(|l| !l.starts_with("note:"))
}

fn trio(models: &[&str]) -> Roles {
    let picks: Vec<String> = models
        .iter()
        .map(|m| format!(r#"{{"model":"{m}","effort":"high","roles":["trio"]}}"#))
        .collect();
    let text = format!(r#"{{"antigravity":{{"picks":[{}]}}}}"#, picks.join(","));
    Roles::parse(&serde_json::from_str(&text).unwrap())
}

fn trio_rig(week: &[&str]) -> Rig {
    let mut rig = setup();
    with_user_file(&mut rig, "{}");
    let mut q = room();
    for pool in week {
        set(&mut q, pool, "Week", 100.0);
    }
    put(&rig, "antigravity", &q);
    put(&rig, "codex", &codex(None));
    rig
}

#[test]
fn a_seat_that_fails_for_room_before_a_later_seat_is_filled_leaves_no_second_model_note() {
    let rig = trio_rig(&["Gemini Models"]);
    let found = find(
        "trio",
        &trio(&["gemini-4-argon", "gemini-4-hydrogen", "claude-opus-5-5"]),
        &rig.deps,
    );
    let models: Vec<&str> = found.picks.iter().map(|p| p.1.as_str()).collect();
    assert_eq!(models, vec!["gemini-4-argon", "claude-opus-5-5"]);
    assert!(
        !found.notes.iter().any(|n| n.contains("no second model")),
        "{:?}",
        found.notes
    );
}

#[test]
fn a_third_seat_that_fails_for_room_with_two_picks_chosen_says_nothing_of_a_second_model() {
    let rig = trio_rig(&["Gemini Models"]);
    let found = find(
        "trio",
        &trio(&["gemini-4-argon", "claude-opus-5-5", "gemini-4-hydrogen"]),
        &rig.deps,
    );
    assert_eq!(found.picks.len(), 2);
    assert!(
        !found.notes.iter().any(|n| n.contains("no second model")),
        "{:?}",
        found.notes
    );
}

#[test]
fn the_no_second_model_note_comes_once_when_only_one_of_three_seats_is_filled() {
    let rig = trio_rig(&["Gemini Models"]);
    let found = find(
        "trio",
        &trio(&["gemini-4-argon", "gemini-4-hydrogen", "gemini-4-neon"]),
        &rig.deps,
    );
    assert_eq!(found.picks.len(), 1);
    let said = found
        .notes
        .iter()
        .filter(|n| n.contains("no second model"))
        .count();
    assert_eq!(said, 1, "{:?}", found.notes);
    assert!(found.notes.last().unwrap().contains("no second model"));
}

#[test]
fn a_role_with_one_seat_never_says_there_is_no_second_model() {
    let rig = trio_rig(&["Gemini Models"]);
    let found = find("trio", &trio(&["gemini-4-hydrogen"]), &rig.deps);
    assert_eq!(found.picks.len(), 1);
    assert!(
        !found.notes.iter().any(|n| n.contains("no second model")),
        "{:?}",
        found.notes
    );
}

#[test]
fn the_second_seat_takes_a_model_of_another_family_that_has_room_over_one_of_the_same_family() {
    let mut rig = setup();
    with_user_file(
        &mut rig,
        r#"{"antigravity":{"picks":[
          {"model":"gemini-4-hydrogen","effort":"high","roles":["review","fam"]},
          {"model":"gemini-4-argon","effort":"high","roles":["fam"],
           "then":{"fam":[{"tool":"agy","model":"claude-opus-5-5","effort":"high"}]}}]}}"#,
    );
    put(&rig, "antigravity", &room());
    put(&rig, "codex", &codex(None));
    let text = pick(&rig, "fam");
    assert_eq!(
        lines_of(&text).0,
        vec![
            "agy --model gemini-4-hydrogen --effort high",
            "agy --model claude-opus-5-5 --effort high"
        ],
        "{text}"
    );
    let mut q = room();
    set(&mut q, "Claude and GPT models", "5h", 100.0);
    put(&rig, "antigravity", &q);
    let text = pick(&rig, "fam");
    assert_eq!(
        lines_of(&text).0,
        vec![
            "agy --model gemini-4-hydrogen --effort high",
            "agy --model gemini-4-argon --effort high"
        ],
        "{text}"
    );
}

#[test]
fn with_codex_out_a_user_marked_model_fills_the_codex_seat_and_no_ladder_step_comes_in() {
    let mut rig = setup();
    with_user_file(&mut rig, USER_FILE);
    put(&rig, "antigravity", &room());
    put(&rig, "codex", &codex(Some(Reached::Credits)));
    let note = "note: gpt-6-astra: out of credits\n";
    assert_eq!(
        pick(&rig, "review-secrets"),
        format!("agy --model gemini-4-carbon --effort high\nagy --model gemini-4-argon --effort high\n{note}")
    );
    assert_eq!(
        pick(&rig, "review-final"),
        format!("agy --model gemini-4-carbon --effort high\n{note}")
    );
    assert_eq!(
        pick(&rig, "spec-check"),
        "agy --model gemini-4-carbon --effort high\n"
    );
    assert_eq!(
        pick(&rig, "review"),
        "agy --model gemini-4-hydrogen --effort high\n"
    );
}

#[test]
fn with_codex_in_the_codex_pick_comes_first_and_the_users_picks_follow() {
    let mut rig = setup();
    with_user_file(&mut rig, USER_FILE);
    put(&rig, "antigravity", &room());
    put(&rig, "codex", &codex(None));
    let astra = "codex --model gpt-6-astra --effort xhigh\n";
    assert_eq!(
        pick(&rig, "review-secrets"),
        format!("{astra}agy --model gemini-4-carbon --effort high\nagy --model gemini-4-argon --effort high\n")
    );
    assert_eq!(
        pick(&rig, "review-final"),
        format!("{astra}agy --model gemini-4-carbon --effort high\n")
    );
    assert_eq!(
        pick(&rig, "spec-check"),
        "agy --model gemini-4-carbon --effort high\n"
    );
}

#[test]
fn a_role_never_prints_more_picks_than_it_has_seats_with_the_users_file() {
    let mut rig = setup();
    with_user_file(&mut rig, USER_FILE);
    let roles = Roles::load(rig.deps.catalog_file.as_deref());
    let mut states = vec![
        (false, room()),
        (true, room()),
        (false, card()),
        (true, card()),
    ];
    let mut argon_out = room();
    set(&mut argon_out, "Gemini 4 Argon", "Week", 100.0);
    states.push((false, argon_out.clone()));
    states.push((true, argon_out));
    for (credits, q) in states {
        put(&rig, "antigravity", &q);
        put(&rig, "codex", &codex(credits.then_some(Reached::Credits)));
        for role in ["review", "review-final", "review-secrets", "spec-check"] {
            let seats = roles.tool("codex").picks_for(role).len()
                + roles.tool("antigravity").picks_for(role).len();
            let text = pick(&rig, role);
            let picks = lines_of(&text).0;
            assert!(picks.len() <= seats, "{role}: {text}");
        }
    }
}

#[test]
fn a_seat_whose_own_model_resolves_to_nothing_walks_past_a_held_stand_in_to_one_with_room() {
    let rig = setup();
    std::fs::write(
        rig.deps.catalog_file.clone().unwrap(),
        r#"{"antigravity":{"picks":[
          {"model":"gemini-4-hydrogen","effort":"high","roles":["review-duo"]},
          {"model":"claude-opus-5-5","effort":"high","roles":["review-duo"],
           "then":{"review-duo":[{"tool":"agy","model":"gemini-4-argon","effort":"high"}]}}]}}"#,
    )
    .unwrap();
    put(&rig, "antigravity", &room());
    put(&rig, "codex", &codex(None));
    let text = pick(&rig, "review-duo");
    assert_eq!(
        lines_of(&text).0,
        vec![
            "agy --model gemini-4-hydrogen --effort high",
            "agy --model gemini-4-argon --effort high"
        ],
        "{text}"
    );
}

fn duo() -> Roles {
    let text = r#"{"antigravity":{"picks":[
      {"model":"gemini-4-hydrogen","effort":"high","roles":["duo"],
       "then":{"duo":[{"tool":"agy","model":"gemini-4-argon","effort":"high"}]}},
      {"model":"gemini-4-argon","effort":"high","roles":["duo"],
       "then":{"duo":[{"tool":"agy","model":"claude-opus-5-5","effort":"high"}]}}]}}"#;
    Roles::parse(&serde_json::from_str(text).unwrap())
}

fn duo_rig(claude_out: bool) -> Rig {
    let rig = setup();
    let mut q = room();
    set(&mut q, "Gemini Models", "Week", 100.0);
    if claude_out {
        set(&mut q, "Claude and GPT models", "5h", 100.0);
    }
    put(&rig, "antigravity", &q);
    put(&rig, "codex", &codex(None));
    rig
}

#[test]
fn a_seat_whose_own_model_an_earlier_seat_reached_by_its_ladder_walks_its_own_ladder() {
    let rig = duo_rig(false);
    let found = find("duo", &duo(), &rig.deps);
    let models: Vec<&str> = found.picks.iter().map(|p| p.1.as_str()).collect();
    assert_eq!(
        models,
        vec!["gemini-4-argon", "claude-opus-5-5"],
        "{:?}",
        found.notes
    );
}

#[test]
fn a_seat_whose_own_model_an_earlier_seat_reached_by_its_ladder_with_nothing_else_says_there_is_no_second_model(
) {
    let rig = duo_rig(true);
    let found = find("duo", &duo(), &rig.deps);
    let models: Vec<&str> = found.picks.iter().map(|p| p.1.as_str()).collect();
    assert_eq!(models, vec!["gemini-4-argon"], "{:?}", found.notes);
    let said = found
        .notes
        .iter()
        .filter(|n| n.contains("no second model for duo"))
        .count();
    assert_eq!(said, 1, "{:?}", found.notes);
}
