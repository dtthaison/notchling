use super::job_kinds_tests::row_of;
use super::*;
use serde_json::json;

#[test]
fn the_four_phase_8_kinds_are_for_the_mac_and_applied_by_the_app() {
    for name in [MACHINE, UPDATE, INSTALL, COST_PROFILE, MODEL] {
        let kind = spec(name).expect(name);
        assert_eq!(kind.answer_on, AnswerOn::Mac, "{name}");
        assert_eq!(kind.applied_by, AppliedBy::App, "{name}");
        assert_eq!(kind.result, Outcome::Stored, "{name}");
    }
}

#[test]
fn only_install_and_the_cost_profile_have_parts() {
    let with: Vec<&str> = KINDS.iter().filter(|k| k.parts).map(|k| k.name).collect();
    assert_eq!(with, vec![INSTALL, COST_PROFILE]);
}

#[test]
fn the_new_kinds_have_titles_that_name_no_machine_or_path() {
    for (kind, words) in [
        (MACHINE, "asks to change a machine"),
        (UPDATE, "asks to update a tool"),
        (INSTALL, "asks to set up Notchling"),
        (COST_PROFILE, "asks to change the Cost Profile"),
        (MODEL, "asks to download a model"),
    ] {
        let row = row_of(
            kind,
            AnswerOn::Mac,
            json!({"action": "add", "host": "10.0.0.9"}),
        );
        assert_eq!(title_of(&row), format!("Claude Code {words}"));
        assert_eq!(
            detail_of(&row),
            "Open the card on your Mac to read what it would do."
        );
    }
}
