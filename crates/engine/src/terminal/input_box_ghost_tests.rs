use super::input_box::{read, read_shaded, typed_text_shaded, Fill};
use super::Shade;

const RULE_LINE: &str = "────────────────────────────────────────";

fn lines(v: &[&str]) -> Vec<String> {
    v.iter().map(|l| l.to_string()).collect()
}

type Cells = (String, Vec<Shade>);

fn cells(plain: &str, cursor: &str, faint: &str) -> Cells {
    let mut shades = vec![Shade::Plain; plain.chars().count()];
    shades.extend(vec![Shade::Cursor; cursor.chars().count()]);
    shades.extend(vec![Shade::Faint; faint.chars().count()]);
    (format!("{plain}{cursor}{faint}"), shades)
}

fn boxed(rows: &[Cells]) -> (Vec<String>, Vec<Vec<Shade>>) {
    let mut screen = vec![RULE_LINE.to_string()];
    let mut shades = vec![Vec::new()];
    for (row, cells) in rows {
        screen.push(row.clone());
        shades.push(cells.clone());
    }
    screen.push(RULE_LINE.to_string());
    shades.push(Vec::new());
    (screen, shades)
}

fn ghosted(typed: &str, ghost: &str) -> (Vec<String>, Vec<Vec<Shade>>) {
    let (first, rest) = ghost.split_at(ghost.chars().next().map_or(0, char::len_utf8));
    boxed(&[cells(&format!("! {typed}"), first, rest)])
}

#[test]
fn a_ghost_suggestion_after_the_typed_text_is_not_part_of_what_the_box_holds() {
    let (screen, shades) = ghosted("git push", " -u origin main");
    let input = read_shaded(&screen, None, &shades).unwrap();
    assert_eq!(input.held, lines(&["! git push"]));
    assert_eq!(input.fill("!git push"), Fill::Whole);
    assert!(input.leftover_of(&["!git push".to_string()]));
    assert_eq!(
        typed_text_shaded(&screen, None, &shades),
        Some("git push".to_string())
    );
    let unshaded = read(&screen, None).unwrap();
    assert_eq!(unshaded.fill("!git push"), Fill::Foreign);
}

#[test]
fn a_ghost_that_starts_with_a_letter_is_cut_from_its_first_cell_too() {
    let (screen, shades) = ghosted("git pu", "sh origin main");
    let input = read_shaded(&screen, None, &shades).unwrap();
    assert_eq!(input.held, lines(&["! git pu"]));
}

#[test]
fn text_the_cursor_or_a_chip_highlights_is_kept_and_only_faint_text_is_cut() {
    let screen = lines(&[RULE_LINE, "❯ fix this", RULE_LINE]);
    let mut plain = vec![Shade::Plain; 10];
    plain[9] = Shade::Cursor;
    let shades = vec![Vec::new(), plain, Vec::new()];
    assert_eq!(
        read_shaded(&screen, None, &shades).unwrap().held,
        lines(&["❯ fix this"])
    );
    let mut faint = vec![Shade::Plain; 10];
    faint[2..].fill(Shade::Faint);
    let hint = vec![Vec::new(), faint, Vec::new()];
    assert!(read_shaded(&screen, Some((1, 2)), &hint).unwrap().empty);
}

#[test]
fn a_faint_chip_after_typed_text_is_text_and_never_cut_as_a_ghost() {
    for chip in [
        "[Image #1]",
        "[Image #12]",
        "[Pasted text #1 +30 lines]",
        "[Pasted text #2]",
        "[...Truncated text #2 +9 lines...]",
    ] {
        let faint = boxed(&[cells("❯ check ", "", chip)]);
        let inverted = boxed(&[cells("❯ check ", &chip[..1], &chip[1..])]);
        for (screen, shades) in [faint, inverted] {
            let input = read_shaded(&screen, None, &shades).unwrap();
            assert_eq!(input.held, lines(&[&format!("❯ check {chip}")]), "{chip}");
            assert_eq!(input.fill("check"), Fill::Foreign, "{chip}");
            assert!(!input.empty, "{chip}");
            assert_eq!(
                typed_text_shaded(&screen, None, &shades),
                Some(format!("check {chip}")),
                "{chip}"
            );
        }
    }
}

#[test]
fn a_chip_beside_a_ghost_keeps_the_whole_box_as_it_is() {
    let (screen, shades) = boxed(&[cells("! echo [Image #1] ", "-", "n hi")]);
    let input = read_shaded(&screen, None, &shades).unwrap();
    assert_eq!(input.held, lines(&["! echo [Image #1] -n hi"]));
}

#[test]
fn a_ghost_that_wraps_onto_the_next_row_is_cut_with_its_continuation() {
    let (screen, shades) = boxed(&[
        cells("! git push -u origin main", " ", "--network"),
        cells("  ", "", "host"),
    ]);
    let input = read_shaded(&screen, None, &shades).unwrap();
    assert_eq!(input.held, lines(&["! git push -u origin main"]));
    assert_eq!(input.fill("!git push -u origin main"), Fill::Whole);
}

#[test]
fn a_ghost_that_wraps_over_three_rows_is_cut_whole() {
    let (screen, shades) = boxed(&[
        cells("! git push", " ", "-u origin main --force"),
        cells("  ", "", "--no-verify --quiet --"),
        cells("  ", "", "verbose"),
    ]);
    let input = read_shaded(&screen, None, &shades).unwrap();
    assert_eq!(input.held, lines(&["! git push"]));
}

#[test]
fn a_ghost_that_starts_on_the_next_row_is_cut_with_that_row() {
    let (screen, shades) = boxed(&[
        cells("! git push -u origin main --network-host", "", ""),
        cells("  ", "-", "-force"),
    ]);
    let input = read_shaded(&screen, None, &shades).unwrap();
    assert_eq!(
        input.held,
        lines(&["! git push -u origin main --network-host"])
    );
}

#[test]
fn a_blank_row_under_the_ghost_does_not_hide_it() {
    let (mut screen, mut shades) = ghosted("git push", " -u origin main");
    screen.insert(2, String::new());
    shades.insert(2, Vec::new());
    let input = read_shaded(&screen, None, &shades).unwrap();
    assert_eq!(input.held[0], "! git push");
    assert_eq!(input.fill("!git push"), Fill::Whole);
    assert_eq!(
        typed_text_shaded(&screen, None, &shades),
        Some("git push".to_string())
    );
}

#[test]
fn a_blank_row_under_a_wrapped_ghost_does_not_hide_it_either() {
    let (screen, shades) = boxed(&[
        cells("! git push -u origin main", " ", "--network"),
        cells("  ", "", "host"),
        cells("", "", ""),
    ]);
    let input = read_shaded(&screen, None, &shades).unwrap();
    assert_eq!(input.held[0], "! git push -u origin main");
    assert_eq!(input.fill("!git push -u origin main"), Fill::Whole);
}

#[test]
fn a_lone_highlighted_cell_is_left_alone() {
    let (screen, shades) = boxed(&[cells("! git statu", "s", "")]);
    let input = read_shaded(&screen, None, &shades).unwrap();
    assert_eq!(input.held, lines(&["! git status"]));
    let (screen, shades) = boxed(&[cells("! git push", " ", "-u"), cells("  ", "x", "")]);
    let input = read_shaded(&screen, None, &shades).unwrap();
    assert_eq!(input.held, lines(&["! git push -u", "  x"]));
}

#[test]
fn faint_rows_that_are_not_clearly_the_ghost_continuing_are_kept() {
    let (screen, shades) = boxed(&[cells("! git push", "", ""), cells("  ", "", "faint")]);
    let input = read_shaded(&screen, None, &shades).unwrap();
    assert_eq!(input.held, lines(&["! git push", "  faint"]));
    let (screen, shades) = boxed(&[
        cells("! git push", "", ""),
        cells("", "", ""),
        cells("  ", "-", "faint"),
    ]);
    let input = read_shaded(&screen, None, &shades).unwrap();
    assert_eq!(input.held, lines(&["! git push", "", "  -faint"]));
    let (screen, shades) = boxed(&[
        cells("! git push", " ", "-u"),
        cells("  second line", "", ""),
    ]);
    let input = read_shaded(&screen, None, &shades).unwrap();
    assert_eq!(input.held, lines(&["! git push -u", "  second line"]));
}
