use notchling_engine::machines::step::{CheckResult, StepId, SubRow};
use notchling_engine::versions::{parse_probe, Tool, WorkerTools};
use serde_json::json;

use super::versions_feed::mac_side;
use super::versions_model::{drift_lines, place_rows, replace_label, PlaceFlag};

fn worker(codex: &str) -> WorkerTools {
    parse_probe(
        &format!(
            "codex.path=/opt/homebrew/bin/codex\ncodex.link=../Cellar/codex/{codex}/bin/codex\ncodex.version=codex-cli {codex}\nclaude.path=\n"
        ),
        1,
    )
}

fn mac(reference: &str, usable: bool) -> serde_json::Value {
    json!({"tools": [{
        "tool": "codex",
        "installed": {"version": reference, "sha256": "ab"},
        "method": "npm",
        "reference": {"version": reference, "sha256": "ab", "result": "partly", "partly": true, "at": 1, "unchecked": []},
        "usable": usable,
        "failed": [],
    }]})
}

#[test]
fn selfupdate_rows_per_place() {
    let result = CheckResult::fail("Self-update is on in ~/.codex/config.toml").rows(vec![
        SubRow::new("~/.claude/settings.json", true, "Self-update is off"),
        SubRow::new(
            "~/.codex/config.toml",
            false,
            "Will add: check_for_update_on_startup = false",
        ),
    ]);
    let rows = place_rows(&result);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].name, "~/.claude/settings.json");
    assert_eq!(rows[0].flag, PlaceFlag::Off);
    assert_eq!(rows[1].name, "~/.codex/config.toml");
    assert_eq!(rows[1].flag, PlaceFlag::On);
    assert_eq!(
        rows[1].detail,
        "Will add: check_for_update_on_startup = false"
    );
}

#[test]
fn cant_read_place_is_flagged() {
    let result = CheckResult::fail("Can't read ~/.codex/config.toml: too_big").rows(vec![
        SubRow::new("Can't read ~/.codex/config.toml", false, "too_big"),
        SubRow::new(
            "CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE",
            false,
            "on in the login shell: the package manager may update Claude Code",
        ),
    ]);
    let rows = place_rows(&result);
    assert_eq!(rows[0].flag, PlaceFlag::CantRead);
    assert_eq!(rows[1].flag, PlaceFlag::On);
}

#[test]
fn drift_row_names_both_versions() {
    let side = mac_side(&mac("0.166.0", true));
    let lines = drift_lines(&side, &worker("0.165.0"));
    assert_eq!(
        lines,
        vec![(Tool::Codex, "Drifted: 0.165.0, the Mac 0.166.0".to_string())]
    );
    assert!(drift_lines(&side, &worker("0.166.0")).is_empty());
}

#[test]
fn ahead_says_never_moved_down_automatically() {
    let side = mac_side(&mac("0.166.0", true));
    let lines = drift_lines(&side, &worker("0.167.0"));
    assert_eq!(
        lines,
        vec![(
            Tool::Codex,
            "Drifted: 0.167.0, the Mac 0.166.0. Moved down only by its own click".to_string()
        )]
    );
}

#[test]
fn no_usable_reference_shows_no_drift() {
    let stale = mac_side(&mac("0.166.0", false));
    assert!(drift_lines(&stale, &worker("0.167.0")).is_empty());
    let none: Result<_, _> = Err(super::versions_feed::MacVersionsError::NotRunning);
    assert!(drift_lines(&none, &worker("0.167.0")).is_empty());
}

#[test]
fn the_remove_alert_names_what_is_still_off() {
    use super::versions_model::remove_note;
    assert_eq!(remove_note(&[]), "The machine itself is not changed.");
    let note = remove_note(&[
        "Self-Update On: Claude Code".to_string(),
        "Self-Update On: Codex".to_string(),
    ]);
    assert!(
        note.starts_with("The machine itself is not changed."),
        "{note}"
    );
    assert!(note.contains("for Claude Code, Codex;"), "{note}");
    assert!(note.contains("Self-Update On…"), "{note}");
}

#[test]
fn the_undo_button_is_named_for_its_step() {
    assert_eq!(
        replace_label(&StepId::self_update(Tool::Claude)),
        "Self-Update On…"
    );
    assert_eq!(replace_label(&StepId::deploy_key("me/app")), "Replace key…");
}
