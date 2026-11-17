use super::*;
use crate::testutil::Scratch;

fn widths(sidebar: f32, details: f32) -> Widths {
    Widths { sidebar, details }
}

#[test]
fn a_ratio_gives_the_width_back_inside_the_limits() {
    let r = ratio(300.0, 1500.0);
    assert!((r - 0.2).abs() < 1e-6);
    assert!((width_from(Side::Sidebar, r, 1500.0) - 300.0).abs() < 0.01);
    assert_eq!(width_from(Side::Sidebar, 0.9, 1500.0), SIDEBAR_MAX);
    assert_eq!(width_from(Side::Sidebar, 0.01, 1500.0), SIDEBAR_MIN);
    assert_eq!(width_from(Side::Details, 0.9, 1500.0), DETAILS_MAX);
    assert_eq!(width_from(Side::Details, 0.01, 1500.0), DETAILS_MIN);
    assert_eq!(width_from(Side::Details, f32::NAN, 1500.0), DETAILS_DEFAULT);
    assert_eq!(width_from(Side::Sidebar, 0.2, 0.0), SIDEBAR_DEFAULT);
    assert_eq!(ratio(300.0, 0.0), 0.0);
}

#[test]
fn a_bigger_window_restores_a_wider_panel_up_to_the_limit() {
    let saved = Saved::of(widths(300.0, 260.0), false, false, 1500.0);
    let at_small = saved.widths(1000.0).sidebar;
    let at_big = saved.widths(1800.0).sidebar;
    assert!(at_small < 300.0 && at_big > 300.0);
    assert_eq!(saved.widths(5000.0).sidebar, SIDEBAR_MAX);
    assert_eq!(saved.widths(300.0).sidebar, SIDEBAR_MIN);
    assert_eq!(saved.widths(1500.0).details, DETAILS_DEFAULT);
}

#[test]
fn a_panel_at_its_default_saves_no_ratio() {
    let saved = Saved::of(Widths::default(), true, false, 1400.0);
    assert_eq!(saved.sidebar, None);
    assert_eq!(saved.details, None);
    assert!(saved.sidebar_hidden && !saved.details_hidden);
}

#[test]
fn the_limits_hold_when_dragging() {
    assert_eq!(dragged(Side::Sidebar, 100.0, 1400.0, 260.0), SIDEBAR_MIN);
    assert_eq!(dragged(Side::Sidebar, 900.0, 1400.0, 260.0), SIDEBAR_MAX);
    assert_eq!(dragged(Side::Sidebar, 310.0, 1400.0, 260.0), 310.0);
    assert_eq!(dragged(Side::Details, 1300.0, 1400.0, 272.0), DETAILS_MIN);
    assert_eq!(dragged(Side::Details, 200.0, 1400.0, 272.0), DETAILS_MAX);
    assert_eq!(dragged(Side::Details, 1000.0, 1400.0, 272.0), 400.0);
}

#[test]
fn a_drag_leaves_the_column_its_minimum_when_the_window_allows() {
    let window = 1000.0;
    let other = 260.0;
    let most = window - COLUMN_MIN - other;
    assert_eq!(dragged(Side::Sidebar, 900.0, window, other), most);
    assert_eq!(dragged(Side::Sidebar, 100.0, 700.0, 400.0), SIDEBAR_MIN);
}

#[test]
fn a_window_with_room_keeps_the_wanted_widths() {
    let w = shown(widths(300.0, 300.0), true, true, 1600.0);
    assert_eq!(w, widths(300.0, 300.0));
}

#[test]
fn a_hidden_panel_takes_no_room() {
    let w = shown(widths(300.0, 300.0), false, true, 1600.0);
    assert_eq!(w, widths(0.0, 300.0));
    let w = shown(widths(300.0, 300.0), true, false, 1600.0);
    assert_eq!(w, widths(300.0, 0.0));
}

#[test]
fn a_squeeze_takes_from_both_panels_down_to_their_minimums() {
    let w = shown(widths(400.0, 400.0), true, true, 1000.0);
    assert!((w.sidebar + w.details + COLUMN_MIN - 1000.0).abs() < 0.01);
    assert!(w.sidebar >= SIDEBAR_MIN && w.details >= DETAILS_MIN);
    assert!(w.sidebar < 400.0 && w.details < 400.0);
    let w = shown(widths(400.0, 400.0), true, true, 700.0);
    assert_eq!(w, widths(SIDEBAR_MIN, DETAILS_MIN));
}

#[test]
fn a_squeeze_never_pushes_a_panel_under_its_minimum() {
    let w = shown(widths(SIDEBAR_MIN, 500.0), true, true, 900.0);
    assert_eq!(w.sidebar, SIDEBAR_MIN);
    assert_eq!(w.details, 900.0 - COLUMN_MIN - SIDEBAR_MIN);
}

#[test]
fn a_saved_value_reads_back_and_a_bad_one_is_ignored() {
    let saved = Saved {
        sidebar: Some(0.21),
        details: None,
        sidebar_hidden: false,
        details_hidden: true,
    };
    assert_eq!(Saved::from_value(Some(&saved.to_value())), Some(saved));
    assert_eq!(Saved::from_value(None), None);
    assert_eq!(Saved::from_value(Some(&json!("x"))), None);
    let odd = Saved::from_value(Some(&json!({"sidebar": -1, "details": 7})));
    assert_eq!(odd.map(|s| (s.sidebar, s.details)), Some((None, None)));
}

#[test]
fn the_panels_land_in_the_store_and_read_back() {
    let dir = Scratch::new("panels-save");
    let file = dir.path().join("settings.json");
    let saved = Saved::of(widths(320.0, 300.0), false, true, 1400.0);
    save(&file, &saved).expect("the panels are saved");
    assert_eq!(load(&file), Some(saved));
}

mod branches {
    use std::sync::Arc;

    use serde_json::{json, Value};

    use crate::chat::branch_list::{panel, PanelInput, PanelState, Tab};
    use crate::chat::branches_state::{
        hidden_switch, on_switch, Applied, Feed, Received, Shown, Switch,
    };
    use crate::chat::changes_target::{on_close, Target, TargetState};
    use crate::chat::git::Why;
    use crate::chat::git_gates::PENDING_AGAIN_MS;
    use crate::chat::panels::{after_close_changes, hide_step, HideStep};

    fn reply(pending: u64, names: &[&str]) -> Value {
        let rows: Vec<Value> = names
            .iter()
            .map(|name| json!({"name": name, "ref": format!("refs/heads/{name}"), "local": true}))
            .collect();
        json!({"repo": true, "name": "r", "pending": pending, "branches": rows})
    }

    fn input_of(feed: &Feed) -> Arc<PanelInput> {
        Arc::new(PanelInput {
            list: feed.list(),
            failed: feed.failed(),
            ..PanelInput::default()
        })
    }

    #[test]
    fn hide_step_order() {
        assert_eq!(hide_step(true, true, true), HideStep::ClosePage);
        assert_eq!(hide_step(false, true, true), HideStep::CloseChanges);
        assert_eq!(hide_step(false, false, true), HideStep::CloseBranches);
        assert_eq!(hide_step(false, false, false), HideStep::HideDetails);
        assert_eq!(hide_step(true, false, false), HideStep::ClosePage);
        assert_eq!(hide_step(false, true, false), HideStep::CloseChanges);
    }

    #[test]
    fn changes_from_branches_closes_back_to_branches() {
        let mut other = Some(TargetState::new(Target::Worktree("/r/lane".into())));
        let mut from_branches = true;
        let back = on_close(&mut other, &mut from_branches);
        assert!(back && other.is_none() && !from_branches);
        assert!(after_close_changes(true, back));
        let mut other: Option<TargetState> = None;
        let mut from_details = false;
        let back = on_close(&mut other, &mut from_details);
        assert!(!after_close_changes(true, back));
        assert!(!after_close_changes(false, true));
    }

    #[test]
    fn session_switch_reloads_or_closes() {
        assert_eq!(on_switch(Some("/r"), Some("/r")), Switch::Same);
        assert_eq!(on_switch(Some("/r"), Some("/other")), Switch::Reload);
        assert_eq!(on_switch(None, Some("/other")), Switch::Reload);
        assert_eq!(on_switch(Some("/r"), None), Switch::Close);
        let mut feed = Feed::default();
        assert_eq!(feed.apply("/r", Ok(reply(0, &["a"])), 0), Applied::Changed);
        let not_repo = json!({"repo": false});
        assert_eq!(feed.apply("/plain", Ok(not_repo), 1), Applied::NotRepo);
        assert!(feed.list().is_none());
    }

    #[test]
    fn open_row_and_filter_clear_on_switch() {
        let mut shown = Shown::default();
        shown.set_input(input_of(&{
            let mut feed = Feed::default();
            feed.apply("/r", Ok(reply(0, &["a"])), 0);
            feed
        }));
        assert!(shown.set_tab(Tab::Merged));
        shown.toggle("refs/heads/a");
        assert_eq!(shown.open(), Some("refs/heads/a"));
        assert!(shown.set_query("ask"));
        assert_eq!(shown.open(), None);
        shown.toggle("refs/heads/a");
        shown.toggle("refs/heads/a");
        assert_eq!(shown.open(), None);
        shown.open_row("refs/heads/a");
        shown.switched();
        assert_eq!((shown.open(), shown.query()), (None, ""));
        assert_eq!(shown.tab(), Tab::Merged);
        assert!(shown.input().list.is_none());
        let model = panel(shown.input(), shown.tab(), shown.query());
        assert_eq!(model.state, PanelState::Loading);
    }

    #[test]
    fn equal_input_is_no_redraw() {
        let mut feed = Feed::default();
        feed.apply("/r", Ok(reply(0, &["a", "b"])), 0);
        let mut shown = Shown::default();
        assert!(shown.set_input(input_of(&feed)));
        assert!(!shown.set_input(input_of(&feed)));
        let again = parse_equal(&mut feed);
        assert!(!shown.set_input(again));
    }

    fn parse_equal(feed: &mut Feed) -> Arc<PanelInput> {
        assert_eq!(
            feed.apply("/r", Ok(reply(0, &["a", "b"])), 1),
            Applied::Same
        );
        input_of(feed)
    }

    #[test]
    fn panel_keeps_its_last_good_list_when_a_call_fails() {
        let mut feed = Feed::default();
        feed.apply("/r", Ok(reply(0, &["a"])), 0);
        let mut shown = Shown::default();
        shown.set_input(input_of(&feed));
        let failed = Arc::new(PanelInput {
            failed: Some("slow".into()),
            ..PanelInput::default()
        });
        shown.set_input(failed);
        assert!(shown.input().list.is_some());
        let model = panel(shown.input(), Tab::Working, "");
        assert_eq!(model.state, PanelState::Ready);
    }

    #[test]
    fn unchanged_answer_is_dropped_and_a_failure_keeps_the_list() {
        let mut feed = Feed::default();
        assert_eq!(feed.apply("/r", Ok(reply(0, &["a"])), 0), Applied::Changed);
        assert_eq!(
            feed.apply("/r", Ok(reply(0, &["a"])), 20_000),
            Applied::Same
        );
        assert_eq!(
            feed.apply("/r", Err("slow".into()), 40_000),
            Applied::Changed
        );
        assert!(feed.list().is_some() && feed.failed().is_some());
        assert_eq!(feed.apply("/r", Err("slow".into()), 60_000), Applied::Same);
        assert_eq!(
            feed.apply("/r", Ok(reply(0, &["a"])), 80_000),
            Applied::Changed
        );
        assert!(feed.failed().is_none());
    }

    #[test]
    fn a_new_folder_forgets_the_list() {
        let mut feed = Feed::default();
        feed.apply("/r", Ok(reply(0, &["a"])), 0);
        assert_eq!(feed.apply("/s", Err("slow".into()), 1), Applied::Changed);
        assert!(feed.list().is_none());
    }

    #[test]
    fn pending_answer_asks_again_after_two_seconds() {
        let mut feed = Feed::default();
        assert!(feed.ask(Why::Opened, Some("/r"), true, 0));
        assert!(!feed.ask(Why::Tick, Some("/r"), true, 500));
        feed.apply("/r", Ok(reply(2, &["a"])), 1_000);
        assert!(!feed.ask(Why::Tick, Some("/r"), true, 1_000 + PENDING_AGAIN_MS - 1));
        assert!(feed.ask(Why::Tick, Some("/r"), true, 1_000 + PENDING_AGAIN_MS));
        feed.apply("/r", Ok(reply(0, &["a"])), 4_000);
        assert!(!feed.ask(Why::Tick, Some("/r"), true, 6_000));
        assert!(feed.ask(Why::Tick, Some("/r"), true, 4_000 + 15_000));
        assert!(!feed.ask(Why::Tick, Some("/r"), false, 90_000));
    }

    #[test]
    fn hidden_switch_forgets_the_old_folders_list() {
        let mut feed = Feed::default();
        feed.apply("/a", Ok(reply(0, &["a"])), 0);
        let mut shown = Shown::default();
        shown.set_input(input_of(&feed));
        shown.open_row("refs/heads/a");
        assert!(!hidden_switch(Some("/a"), Some("/a"), &mut feed));
        assert!(feed.list_for(Some("/a")).is_some());
        assert!(hidden_switch(Some("/a"), Some("/b"), &mut feed));
        shown.switched();
        assert!(feed.list_for(Some("/b")).is_none());
        assert!(feed.list_for(Some("/a")).is_none());
        assert_eq!((shown.open(), shown.query()), (None, ""));
        assert!(feed.ask(Why::Opened, Some("/b"), true, 1));
        let generation = feed.generation();
        let got = feed.receive(generation, "/b", Some("/b"), Err("slow".into()), 2);
        assert_eq!(got, Received::Applied(Applied::Changed));
        let input = Arc::new(PanelInput {
            list: feed.list_for(Some("/b")),
            failed: feed.failed_for(Some("/b")),
            ..PanelInput::default()
        });
        shown.set_input(input);
        assert!(shown.input().list.is_none());
        assert_eq!(shown.input().failed.as_deref(), Some("slow"));
        let model = panel(shown.input(), shown.tab(), shown.query());
        assert_eq!(model.state, PanelState::Failed("slow".into()));
    }

    #[test]
    fn list_for_another_folder_is_none() {
        let mut feed = Feed::default();
        feed.apply("/a", Err("slow".into()), 0);
        assert!(feed.failed_for(Some("/a")).is_some());
        assert!(feed.failed_for(Some("/b")).is_none());
        assert!(feed.failed_for(None).is_none());
        feed.apply("/a", Ok(reply(0, &["a"])), 1);
        assert!(feed.list_for(Some("/a")).is_some());
        assert!(feed.list_for(Some("/b")).is_none());
        assert!(feed.list_for(None).is_none());
    }

    #[test]
    fn stale_generation_reply_leaves_the_gate_in_flight() {
        let mut feed = Feed::default();
        assert!(feed.ask(Why::Opened, Some("/a"), true, 0));
        let old = feed.generation();
        feed.forget();
        assert_ne!(feed.generation(), old);
        assert!(feed.ask(Why::Opened, Some("/b"), true, 1));
        let got = feed.receive(old, "/a", Some("/b"), Ok(reply(0, &["a"])), 2);
        assert_eq!(got, Received::Stale);
        assert!(!feed.ask(Why::Opened, Some("/b"), true, 3));
        assert!(feed.list_for(Some("/a")).is_none() && feed.list_for(Some("/b")).is_none());
        let got = feed.receive(old, "/b", Some("/b"), Ok(reply(0, &["a"])), 4);
        assert_eq!(got, Received::Stale);
        assert!(!feed.ask(Why::Opened, Some("/b"), true, 5));
    }

    #[test]
    fn same_generation_reply_for_a_moved_folder_asks_again() {
        let mut feed = Feed::default();
        assert!(feed.ask(Why::Opened, Some("/a"), true, 0));
        let generation = feed.generation();
        let got = feed.receive(generation, "/a", Some("/b"), Ok(reply(0, &["a"])), 1);
        assert_eq!(got, Received::Moved);
        assert!(feed.ask(Why::Folder, Some("/b"), true, 2));
    }
}
