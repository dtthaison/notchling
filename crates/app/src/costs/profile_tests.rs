use super::profile::{Profile, CUSTOM};
use super::renders::profile_reply;

fn balanced() -> Profile {
    Profile::from_reply(&profile_reply(false)).unwrap()
}

fn edited() -> Profile {
    Profile::from_reply(&profile_reply(true)).unwrap()
}

#[test]
fn a_preset_shows_its_three_choices_with_one_picked() {
    let segments = balanced().segments();
    let keys: Vec<&str> = segments.iter().map(|s| s.key).collect();
    assert_eq!(keys, ["saver", "balanced", "quality"]);
    let picked: Vec<&str> = segments
        .iter()
        .filter(|s| s.selected)
        .map(|s| s.key)
        .collect();
    assert_eq!(picked, ["balanced"]);
}

#[test]
fn an_edited_profile_turns_into_custom_and_no_preset_stays_picked() {
    let segments = edited().segments();
    let keys: Vec<&str> = segments.iter().map(|s| s.key).collect();
    assert_eq!(keys, ["saver", "balanced", "quality", CUSTOM]);
    let picked: Vec<&str> = segments
        .iter()
        .filter(|s| s.selected)
        .map(|s| s.key)
        .collect();
    assert_eq!(picked, [CUSTOM]);
}

#[test]
fn the_table_reads_every_role_and_tool() {
    let p = balanced();
    assert_eq!(p.rows.len(), 7);
    assert_eq!(p.cell("code", 0).unwrap().text(), "sonnet high");
    assert_eq!(p.cell("judge", 1).unwrap().text(), "gpt-6-astra xhigh");
    assert_eq!(p.cell("image", 1).unwrap().text(), "- low");
    assert!(p.cell("image", 0).is_none());
    assert!(p.cell("review", 1).is_none());
}

#[test]
fn floors_are_read_per_role() {
    let p = balanced();
    assert_eq!(
        p.row("judge").unwrap().floor.as_ref().unwrap().text(),
        "opus high"
    );
    assert_eq!(
        p.row("hardest").unwrap().floor.as_ref().unwrap().text(),
        "opus xhigh"
    );
    assert!(p.row("read").unwrap().floor.is_none());
}

#[test]
fn edited_cells_are_counted_and_marked() {
    let p = edited();
    assert_eq!(p.edited_cells(), 2);
    assert!(p.cell("code", 0).unwrap().own);
    assert!(!balanced().cell("code", 0).unwrap().own);
}

#[test]
fn habits_read_as_lines_with_grouped_numbers() {
    let lines = balanced().habit_lines();
    assert_eq!(lines[0], "Builder hands back after 80 turns");
    assert_eq!(lines[1], "5 builders at once");
    assert_eq!(lines[4], "Bash output inline up to 12,000 characters");
}

#[test]
fn the_builder_limit_steps_by_one_inside_its_bounds() {
    use super::profile::habit_steps;
    assert_eq!(habit_steps("parallelCap", 5), (Some(4), Some(6)));
    assert_eq!(habit_steps("parallelCap", 1), (None, Some(2)));
    assert_eq!(habit_steps("parallelCap", 12), (Some(11), None));
    assert_eq!(habit_steps("parallelCap", 14), (Some(12), None));
    assert_eq!(habit_steps("parallelCap", 0), (None, Some(1)));
}

#[test]
fn a_reply_without_a_table_is_no_profile() {
    assert!(Profile::from_reply(&serde_json::json!({"profile": {"preset": "saver"}})).is_none());
}
