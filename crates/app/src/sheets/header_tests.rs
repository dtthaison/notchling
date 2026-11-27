const HEADER: &str = include_str!("header.rs");

const SHEETS: [(&str, &str, &str); 22] = [
    ("new session", include_str!("view.rs"), "sheet_header("),
    (
        "settings",
        include_str!("settings_view.rs"),
        "sheet_header(",
    ),
    ("chrome", include_str!("chrome_view.rs"), "sheet_header("),
    (
        "second opinion",
        include_str!("second_opinion_view.rs"),
        "sheet_header(",
    ),
    (
        "accounts",
        include_str!("../accounts/view_tabs.rs"),
        "close_button(",
    ),
    (
        "vault",
        include_str!("../vault/view.rs"),
        "sheet_header_closing(",
    ),
    (
        "vault request",
        include_str!("../vault/prompt_view.rs"),
        "close_button(",
    ),
    (
        "machines",
        include_str!("../cluster/view.rs"),
        "sheet_header_closing(",
    ),
    (
        "add machine",
        include_str!("../cluster/add_view.rs"),
        "close_button(",
    ),
    (
        "update machines",
        include_str!("../cluster/updates_view.rs"),
        "close_button(",
    ),
    (
        "edit addresses",
        include_str!("../cluster/edit_addresses_view.rs"),
        "close_button(",
    ),
    (
        "add from tailscale",
        include_str!("../cluster/from_tailscale_view.rs"),
        "close_button(",
    ),
    (
        "move",
        include_str!("../cluster/move_view.rs"),
        "close_button(",
    ),
    (
        "sign in",
        include_str!("../cluster/tool_setup_view.rs"),
        "close_button(",
    ),
    (
        "fleet sheets",
        include_str!("../machines/sheets_view.rs"),
        "close_button(",
    ),
    (
        "fleet diff",
        include_str!("../machines/diff_view.rs"),
        "close_button(",
    ),
    (
        "simulator new",
        include_str!("../chat/view/simulator/parts.rs"),
        "close_button(",
    ),
    (
        "tasks bar",
        include_str!("../chat/view/tasks.rs"),
        "icon_button_small(",
    ),
    (
        "opinions",
        include_str!("../chat/view/opinions.rs"),
        "icon_button_small(",
    ),
    (
        "changes",
        include_str!("../chat/view/changes_panel.rs"),
        "icon_button_small(",
    ),
    (
        "branches",
        include_str!("../chat/view/branches_panel.rs"),
        "icon_button(",
    ),
    (
        "page panel",
        include_str!("../chat/view/page_panel.rs"),
        "icon_button(",
    ),
];

const TEXT_CLOSERS: [&str; 16] = [
    "\"settings-done\"",
    "\"accounts-done\"",
    "\"vault-done\"",
    "\"vault-cancel\"",
    "\"cluster-done\"",
    "\"secrets-cancel\"",
    "\"trust-cancel\"",
    "\"fix-cancel\"",
    "\"job-cancel\"",
    "\"diff-done\"",
    "\"move-cancel\"",
    "\"node-login-cancel\"",
    "\"edit-addresses-cancel\"",
    "\"sim-new-cancel\"",
    "t::chat::sheet_cancel()",
    "t::cluster::login_close()",
];

#[test]
fn every_sheet_closes_with_the_shared_x() {
    for (name, source, closer) in SHEETS {
        assert!(source.contains(closer), "{name} has no {closer}");
    }
}

#[test]
fn no_sheet_closes_with_a_text_button() {
    for (name, source, _) in SHEETS {
        for word in TEXT_CLOSERS {
            assert!(!source.contains(word), "{name} still has {word}");
        }
    }
}

#[test]
fn the_x_is_the_kits_icon_button_with_a_close_tooltip() {
    assert!(HEADER.contains("icon_button(id, Icon::Close, ButtonKind::Ghost"));
    assert!(HEADER.contains("t::chat::sheet_close()"));
    assert!(HEADER.contains("tip_text("));
}
