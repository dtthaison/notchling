use notchling_locale as t;

#[test]
fn the_page_and_its_tabs_keep_their_english() {
    assert_eq!(t::settings::page_costs(), "Models & Costs");
    assert_eq!(t::costs::tab_roles(), "Roles");
    assert_eq!(t::costs::tab_check(), "Check");
    assert_eq!(t::costs::tab_cache(), "Cache");
    assert_eq!(t::costs::setup_sheet_title(), "Set Up Cost Profile");
    assert_eq!(
        t::costs::setup_nothing_written(),
        "Nothing is written until Apply."
    );
}

#[test]
fn counts_use_the_plural_each_language_needs() {
    assert_eq!(t::costs::habit_parallel(1), "1 builder at once");
    assert_eq!(t::costs::habit_parallel(5), "5 builders at once");
    assert_eq!(
        t::costs::setup_done(1),
        "1 file written. Undo puts it back."
    );
    assert_eq!(
        t::costs::setup_done(4),
        "4 files written. Undo puts them back."
    );
    assert_eq!(
        t::costs::undone_conflicts(1),
        "Undone, 1 file left as it is"
    );
    assert_eq!(t::costs::check_drift(2, 6), "2 of 6 differ from the table.");
}

#[test]
fn menu_and_button_titles_are_in_title_case() {
    for title in [
        t::costs::apply(),
        t::costs::undo(),
        t::costs::check_keep(),
        t::costs::check_restore(),
        t::costs::check_again(),
        t::costs::picker_reset(),
        t::costs::coming_later(),
        t::costs::diff_show(),
        t::costs::diff_hide(),
        t::costs::report_title(),
        t::costs::imported_title(),
        t::costs::check_title(),
        t::settings::page_costs(),
    ] {
        for word in title.split(' ') {
            let small = matches!(word, "&" | "to" | "of" | "the");
            assert!(
                small || word.chars().next().is_some_and(char::is_uppercase),
                "{title}"
            );
        }
    }
}

#[test]
fn a_tip_names_the_role_and_its_floor() {
    assert_eq!(
        t::costs::below_floor("Judge", "opus high"),
        "Below the floor for Judge: opus high or better."
    );
}
