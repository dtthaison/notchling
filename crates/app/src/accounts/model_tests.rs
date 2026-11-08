//! The window's rows and pages over the render's made-up snapshot.

use super::*;
use crate::accounts::flows::AddPanel;
use crate::accounts::renders::{snapshot, Shown, NOW};
use crate::switches::ACCOUNTS_OFF;

fn on() -> Switches {
    Switches::all_on()
}

#[test]
fn the_tabs_are_claude_code_then_the_daemons_providers_in_order() {
    let s = snapshot(Shown::Claude);
    let tabs_of = tabs(&s);
    let titles: Vec<&str> = tabs_of.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Claude Code", "Claude Desktop", "Codex", "Antigravity"]
    );
    let alone = tabs(&Snapshot::default());
    assert_eq!(alone.len(), 1);
    assert_eq!(alone[0].id, CLAUDE_CODE);
    assert_eq!(Tab::TokenCost.title(), "Token Cost");
    assert_eq!(initial("ops@example.com"), "O");
    assert_eq!(initial(""), "?");
}

#[test]
fn the_claude_page_has_in_use_switch_signed_out_and_lane_rows() {
    let s = snapshot(Shown::Claude);
    let p = page(CLAUDE_CODE, &s, &on(), false, NOW, Zone::UTC);
    assert_eq!(p.title, "Claude Code");
    assert_eq!(p.empty, None);
    assert_eq!(p.off, None);
    let me = &p.rows[0];
    assert!(me.in_use && me.switch.is_none());
    assert_eq!(me.subtitle, "Max 20x");
    let RowContent::Limits(groups) = &me.content else {
        panic!("the account in use shows the watcher's windows");
    };
    assert_eq!(groups[0].lines[0].label, "5 hours");
    assert_eq!(groups[0].lines[1].label, "Week");
    assert_eq!(
        me.cost.as_deref(),
        Some("Today $5.30 · 7 days $17.40 · 30 days $61.90")
    );
    let ops = &p.rows[1];
    assert_eq!(ops.subtitle, "Max 5x · night lane");
    assert!(
        ops.switch.is_none(),
        "an account live in a lane isn't switched to"
    );
    let old = &p.rows[2];
    assert_eq!(
        old.content,
        RowContent::Text {
            text: "Signed out. Add this account again to sign in.".into(),
            warning: true
        }
    );
    assert!(old.switch.is_none());
    let team = &p.rows[3];
    assert!(team.switch.is_some_and(|c| c.enabled));
    assert_eq!(
        team.content,
        RowContent::Text {
            text: "Limits show once the saved login is checked.".into(),
            warning: false
        }
    );
    assert_eq!(team.menu[0].title, "Remove…");
    assert_eq!(
        p.add,
        AddPanel::Added {
            line: "Added team@example.com.".into()
        }
    );
    // Busy: Switch waits with no reason to give.
    let busy = page(CLAUDE_CODE, &s, &on(), true, NOW, Zone::UTC);
    let c = busy.rows[3].switch.unwrap();
    assert!(!c.enabled && c.tooltip.is_none());
}

#[test]
fn off_lists_the_names_and_every_control_says_why() {
    let s = snapshot(Shown::Off);
    let p = page(CLAUDE_CODE, &s, &Switches::all_off(), false, NOW, Zone::UTC);
    assert_eq!(p.off, Some(ACCOUNTS_OFF));
    assert_eq!(p.rows.len(), 4);
    for row in &p.rows {
        assert!(!row.in_use);
        if let Some(c) = row.switch {
            assert_eq!(c.tooltip, Some(ACCOUNTS_OFF));
        }
        // ⋯ Remove asks the daemon too: off, it is drawn disabled with the reason.
        assert_eq!(
            row.menu[0].control,
            Control {
                enabled: false,
                tooltip: Some(ACCOUNTS_OFF)
            }
        );
        assert!(matches!(
            row.content,
            RowContent::Text { .. } | RowContent::None
        ));
    }
    let AddPanel::Idle { add, .. } = p.add else {
        panic!()
    };
    assert_eq!(add.tooltip, Some(ACCOUNTS_OFF));
    let codex = page(CODEX, &s, &Switches::all_off(), false, NOW, Zone::UTC);
    assert_eq!(codex.rows.len(), 2, "saved Codex names list while off");
    assert!(codex.rows.iter().all(|r| !r.menu[0].control.enabled));
    let live = page(CLAUDE_CODE, &s, &on(), false, NOW, Zone::UTC);
    assert!(live.rows.iter().all(|r| r.menu[0].control.enabled));
    assert!(codex.rows.iter().all(|r| !r.in_use));
    // Detached: the daemon's absence is the reason.
    let d = page(
        CLAUDE_CODE,
        &s,
        &Switches::detached(),
        false,
        NOW,
        Zone::UTC,
    );
    assert_eq!(d.off, Some(crate::acting::DETACHED_MESSAGE));
}

#[test]
fn provider_pages_show_their_reports_defaults_and_remove_words() {
    let s = snapshot(Shown::Claude);
    let codex = page(CODEX, &s, &on(), false, NOW, Zone::UTC);
    assert_eq!(codex.title, "Codex");
    let me = &codex.rows[0];
    assert!(me.in_use);
    assert_eq!(me.subtitle, "Plus");
    let RowContent::Limits(groups) = &me.content else {
        panic!("the report");
    };
    assert_eq!(groups[0].name.as_deref(), Some("Codex"));
    assert_eq!(groups[0].lines[0].label, "5 hours");
    let work = &codex.rows[1];
    assert_eq!(work.subtitle, "ChatGPT");
    assert!(work.switch.is_some_and(|c| c.enabled));
    assert_eq!(
        work.content,
        RowContent::Text {
            text: "Limits show again once you switch to it.".into(),
            warning: false
        }
    );
    let agy = page(ANTIGRAVITY, &s, &on(), false, NOW, Zone::UTC);
    assert!(matches!(&agy.rows[0].content, RowContent::Limits(g) if g.len() == 2));
    assert_eq!(agy.rows[0].subtitle, "Google Account");
    let desktop = page(CLAUDE_DESKTOP, &s, &on(), false, NOW, Zone::UTC);
    assert_eq!(desktop.rows[0].content, RowContent::None);
    assert!(
        desktop.rows[1].switch.is_none(),
        "not saved in Desktop: no Switch"
    );
    assert_eq!(desktop.rows[1].menu[0].title, "Remove from List…");
    // A sign-in under way disables Switch and shows the waiting panel.
    let waiting = snapshot(Shown::Codex);
    let codex = page(CODEX, &waiting, &on(), false, NOW, Zone::UTC);
    assert!(codex.rows[1].switch.is_some_and(|c| !c.enabled));
    assert!(matches!(
        codex.add,
        AddPanel::Waiting {
            url: Some(_),
            copy_link: false,
            ..
        }
    ));
    // An unknown provider is an empty page.
    let none = page("gemini", &s, &on(), false, NOW, Zone::UTC);
    assert!(none.rows.is_empty() && none.empty.is_some());
}
