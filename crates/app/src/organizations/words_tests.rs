use notchling_locale as t;

#[test]
fn the_words_the_page_shows_come_from_messages_and_keep_their_english() {
    assert_eq!(t::organizations::page(), "Organizations");
    assert_eq!(t::organizations::new(), "New Organization…");
    assert_eq!(t::organizations::menu_rename(), "Rename…");
    assert_eq!(t::organizations::menu_colour(), "Colour");
    assert_eq!(t::organizations::menu_folders(), "Folders…");
    assert_eq!(t::organizations::menu_delete(), "Delete Organization");
    assert_eq!(t::organizations::delete_title("Acme"), "Delete Acme?");
    assert_eq!(t::organizations::folders_title("Acme"), "Folders of Acme");
    assert_eq!(
        t::organizations::folders_line("~/a, ~/b"),
        "Folders: ~/a, ~/b"
    );
}

#[test]
fn counts_use_the_plural_each_language_needs() {
    assert_eq!(t::organizations::count_accounts(1), "1 account");
    assert_eq!(t::organizations::count_accounts(2), "2 accounts");
    assert_eq!(t::organizations::count_secrets(0), "0 secrets");
    assert_eq!(t::organizations::count_machines(1), "1 machine");
    assert_eq!(
        t::organizations::counts("1 account", "2 secrets", "0 machines"),
        "1 account · 2 secrets · 0 machines"
    );
    assert_eq!(
        t::organizations::folders_moved(1),
        "One open session would change organization:"
    );
    assert_eq!(
        t::organizations::folders_moved(4),
        "4 open sessions would change organization:"
    );
}

#[test]
fn menu_and_button_titles_are_in_title_case() {
    for title in [
        t::organizations::new(),
        t::organizations::menu_delete(),
        t::organizations::page(),
    ] {
        for word in title.trim_end_matches('…').split(' ') {
            assert!(
                word.chars().next().is_some_and(char::is_uppercase),
                "{title}"
            );
        }
    }
}
