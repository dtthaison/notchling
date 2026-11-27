use super::*;
use crate::accounts::store::Snapshot;
use crate::chat::sidebar::tests::session;
use crate::chat::sidebar::{sidebar_model, SidebarModel};
use notchling_engine::sessions::history;
use notchling_engine::sessions::history::Kind;

const NOW: i64 = 1_000_000_000_000;

fn ended(id: &str, name: &str, ago_ms: i64) -> Session {
    let mut s = session(id, AgentGroup::Completed, None, Tool::Claude);
    s.agent.label = markers::EXITED.into();
    s.agent.name = name.into();
    s.agent.since_ms = NOW - ago_ms;
    s
}

fn item(id: &str, title: &str, ago_ms: i64) -> Item {
    Item {
        kind: Kind::Past,
        id: id.into(),
        title: title.into(),
        folder: "/w/proj".into(),
        config_dir: None,
        path: Some(format!("/c/projects/-w-proj/{id}.jsonl")),
        modified_ms: NOW - ago_ms,
    }
}

fn model(open: bool, sessions: &[Session], past: &Past) -> SidebarModel {
    sidebar_model(sessions, NOW, None, &Snapshot::default(), None)
        .with_completed(open, sessions, past, NOW)
}

fn names(m: &SidebarModel) -> Vec<&str> {
    m.completed.rows.iter().map(|r| r.name.as_str()).collect()
}

fn past_of(items: Vec<Item>, next: Option<&str>) -> Past {
    let mut past = Past::new();
    past.add(items, next.map(str::to_owned));
    past
}

#[test]
fn closed_has_no_completed_rows_and_the_live_sections_are_the_same() {
    let sessions = vec![
        session("a", AgentGroup::Working, None, Tool::Claude),
        ended("gone", "Ended one", 60_000),
    ];
    let past = past_of(vec![item("old", "Old one", 90_000_000)], None);
    let closed = model(false, &sessions, &past);
    assert_eq!(closed.completed, Completed::default());
    let open = model(true, &sessions, &past);
    assert_eq!(closed.sections, open.sections);
    assert_ne!(closed, open);
}

#[test]
fn open_lists_ended_rows_first_newest_first_then_past_rows_in_order() {
    let sessions = vec![
        ended("gone-old", "Ended earlier", 7_200_000),
        ended("gone-new", "Ended later", 60_000),
    ];
    let past = past_of(
        vec![
            item("p1", "Past newest", 90_000_000),
            item("p2", "Past older", 190_000_000),
        ],
        None,
    );
    let m = model(true, &sessions, &past);
    assert!(m.completed.open);
    assert_eq!(
        names(&m),
        ["Ended later", "Ended earlier", "Past newest", "Past older"]
    );
    assert_eq!(m.completed.rows[0].age, "1m");
    assert_eq!(m.completed.rows[2].folder, "proj");
    assert!(!m.completed.more);
}

#[test]
fn a_past_row_is_not_listed_twice_or_beside_a_live_row() {
    let mut live = session("job-1", AgentGroup::Working, None, Tool::Claude);
    live.agent.resume_session_id = Some("same-as-live".into());
    let sessions = vec![
        live,
        session("by-stem", AgentGroup::Completed, None, Tool::Claude),
        ended("was-ended", "Ended row", 1_000),
    ];
    let mut by_stem = sessions.clone();
    by_stem[1].transcript = Some(PathBuf::from("/c/projects/x/stem-id.jsonl"));
    let past = past_of(
        vec![
            item("same-as-live", "dup of resume", 5_000),
            item("stem-id", "dup of transcript", 6_000),
            item("job-1", "dup of id", 7_000),
            item("was-ended", "dup of ended", 8_000),
            item("fresh", "Only this", 9_000),
        ],
        None,
    );
    let m = model(true, &by_stem, &past);
    assert_eq!(names(&m), ["Ended row", "Only this"]);
}

#[test]
fn a_real_ended_row_wins_over_a_past_item_with_the_same_id() {
    let sessions = vec![ended("same", "Real ended", 1_000)];
    let past = past_of(vec![item("same", "From file", 2_000)], None);
    let m = model(true, &sessions, &past);
    assert_eq!(names(&m), ["Real ended"]);
    let mut both = sessions.clone();
    both.extend(past.sessions());
    assert_eq!(names(&model(true, &both, &past)), ["Real ended"]);
}

#[test]
fn injected_past_sessions_are_ended_rows_that_the_sections_hide() {
    let past = past_of(vec![item("p1", "Past one", 1_000)], None);
    let held = past.sessions();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].agent.label, markers::EXITED);
    assert_eq!(held[0].tool, Tool::Claude);
    assert_eq!(
        held[0].transcript.as_deref(),
        Some(Path::new("/c/projects/-w-proj/p1.jsonl"))
    );
    let m = model(true, &held, &past);
    assert!(m.sections.is_empty());
    assert_eq!(names(&m), ["Past one"]);
}

#[test]
fn more_shows_while_a_cursor_is_left_and_adding_pages_keeps_order_without_repeats() {
    let mut past = past_of(
        vec![item("a", "A", 1_000), item("b", "B", 2_000)],
        Some("p1:b"),
    );
    assert!(model(true, &[], &past).completed.more);
    past.add(vec![item("b", "B", 2_000), item("c", "C", 3_000)], None);
    let m = model(true, &[], &past);
    assert_eq!(names(&m), ["A", "B", "C"]);
    assert!(!m.completed.more);
}

#[test]
fn without_hidden_drops_past_sessions_a_live_row_already_shows() {
    let live = vec![session("job-1", AgentGroup::Working, None, Tool::Claude)];
    let incoming = vec![
        session_of(&item("job-1", "dup", 1_000)),
        session_of(&item("other", "kept", 2_000)),
    ];
    let kept = without_hidden(incoming, &hidden_ids(&live, &Past::new()));
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].agent.id, "other");
}

#[test]
fn an_ended_row_hides_every_id_it_goes_by() {
    let mut by_resume = ended("job-1", "By resume", 1_000);
    by_resume.agent.resume_session_id = Some("resume-id".into());
    let mut by_stem = ended("job-2", "By stem", 2_000);
    by_stem.transcript = Some(PathBuf::from("/c/projects/x/stem-id.jsonl"));
    let mut by_target = ended("job-3", "By target", 3_000);
    by_target.agent.target = AgentTarget::Resume {
        session_id: "target-id".into(),
        cwd: "/w/proj".into(),
    };
    let sessions = vec![by_resume, by_stem, by_target];
    let past = past_of(
        vec![
            item("resume-id", "dup of resume", 5_000),
            item("stem-id", "dup of stem", 6_000),
            item("target-id", "dup of target", 7_000),
            item("fresh", "Only this", 9_000),
        ],
        None,
    );
    let m = model(true, &sessions, &past);
    assert_eq!(
        names(&m),
        ["By resume", "By stem", "By target", "Only this"]
    );
}

#[test]
fn the_skip_set_holds_live_and_real_ended_ids_but_not_the_ones_the_past_list_injected() {
    let live = session("job-1", AgentGroup::Working, None, Tool::Claude);
    let mut gone = ended("job-2", "Gone", 1_000);
    gone.transcript = Some(PathBuf::from("/c/projects/x/gone-stem.jsonl"));
    gone.agent.resume_session_id = Some("gone-resume".into());
    let past = past_of(vec![item("p1", "Past one", 2_000)], None);
    let mut sessions = vec![live, gone];
    sessions.extend(past.sessions());
    let skip = hidden_ids(&sessions, &past);
    for id in ["job-1", "job-2", "gone-stem", "gone-resume"] {
        assert!(skip.contains(id), "{id}");
    }
    assert!(!skip.contains("p1"));
}

#[test]
fn a_page_with_too_few_visible_rows_asks_for_the_next_one_up_to_three_pages() {
    let hidden: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
    let page = vec![item("a", "A", 1), item("b", "B", 2), item("c", "C", 3)];
    let mut past = Past::new();
    let generation = past.open_request().generation;
    let first = past.refill(&page, &hidden, Some("p1:c")).expect("page two");
    assert_eq!(first.after.as_deref(), Some("p1:c"));
    assert_eq!(first.limit, history::PAGE - 1);
    assert_eq!(first.generation, generation);
    assert!(!first.replace);
    let second = past
        .refill(&page, &hidden, Some("p2:c"))
        .expect("page three");
    assert_eq!(second.limit, history::PAGE - 2);
    assert!(past.refill(&page, &hidden, Some("p3:c")).is_none());
    past.add(page.clone(), Some("p3:c".into()));
    past.more_request().expect("a new batch");
    assert!(past.refill(&page, &hidden, Some("p4:c")).is_some());
}

#[test]
fn visible_rows_count_across_the_batch_not_one_page() {
    let hidden: HashSet<String> = (0..10).map(|n| format!("i{n}")).collect();
    let page: Vec<Item> = (0..30).map(|n| item(&format!("i{n}"), "T", n)).collect();
    let more: Vec<Item> = (30..40).map(|n| item(&format!("i{n}"), "T", n)).collect();
    let mut past = Past::new();
    past.open_request();
    let ask = past
        .refill(&page, &hidden, Some("p1:i29"))
        .expect("ten short");
    assert_eq!(ask.limit, 10);
    assert!(past.refill(&more, &hidden, Some("p1:i39")).is_none());
}

#[test]
fn a_full_page_or_the_last_page_asks_for_nothing_more() {
    let none: HashSet<String> = HashSet::new();
    let page: Vec<Item> = (0..30).map(|n| item(&format!("i{n}"), "T", n)).collect();
    let mut past = Past::new();
    past.open_request();
    assert!(past.refill(&page, &none, Some("p1:i29")).is_none());
    let hidden: HashSet<String> = ["i0"].iter().map(|s| s.to_string()).collect();
    past.open_request();
    assert!(past.refill(&page, &hidden, None).is_none());
}

#[test]
fn opening_reloads_as_many_rows_as_were_loaded_and_never_less_than_a_page() {
    let mut past = past_of(vec![item("a", "A", 1_000)], None);
    let req = past.open_request();
    assert!(req.replace);
    assert_eq!(req.after, None);
    assert_eq!(req.limit, history::PAGE);
    assert!(past.loading);
    let items: Vec<Item> = (0..45).map(|n| item(&format!("i{n}"), "T", n)).collect();
    let mut deep = past_of(items, Some("p1:i44"));
    assert_eq!(deep.open_request().limit, 45);
}

#[test]
fn a_watched_reload_keeps_the_pages_already_loaded_and_waits_between_reloads() {
    let items: Vec<Item> = (0..45).map(|n| item(&format!("i{n}"), "T", n)).collect();
    let mut past = past_of(items, Some("p1:i44"));
    let req = past.watch_request(100_000).expect("a reload");
    assert!(req.replace);
    assert_eq!(req.after, None);
    assert_eq!(req.limit, 45);
    past.replace(vec![item("a", "A", 1)], None, 100_000, None);
    assert!(past.watch_request(101_000).is_none());
    assert!(past.watch_request(100_000 + WATCH_EVERY_MS).is_some());
}

#[test]
fn a_watched_reload_never_starts_over_a_request_in_flight() {
    let mut past = past_of(vec![item("a", "A", 1_000)], Some("p1:a"));
    past.more_request().expect("next page");
    assert!(past.watch_request(i64::MAX / 2).is_none());
}

#[test]
fn a_user_request_supersedes_the_one_in_flight_and_the_old_answer_is_dropped() {
    let mut past = past_of(vec![item("a", "A", 1_000)], Some("p1:a"));
    let watch = past.watch_request(100_000).expect("a reload");
    assert!(past.accepts(&watch));
    let more = past.more_request().expect("a click is never ignored");
    assert!(!past.accepts(&watch));
    assert!(past.accepts(&more));
    let open = past.open_request();
    assert!(!past.accepts(&more));
    assert!(past.accepts(&open));
}

#[test]
fn more_asks_for_the_cursor_and_only_while_there_is_one() {
    let mut past = past_of(vec![item("a", "A", 1_000)], Some("p1:a"));
    let req = past.more_request().expect("next page");
    assert!(!req.replace);
    assert_eq!(req.after.as_deref(), Some("p1:a"));
    past.add(vec![item("b", "B", 2_000)], None);
    assert!(past.more_request().is_none());
}

#[test]
fn replacing_drops_what_is_gone_and_clears_loading() {
    let mut past = past_of(vec![item("a", "A", 1_000), item("b", "B", 2_000)], None);
    past.replace(
        vec![item("b", "B2", 2_000), item("c", "C", 500)],
        Some("p1:c".into()),
        5,
        None,
    );
    let ids: Vec<&str> = past.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["b", "c"]);
    assert_eq!(past.next.as_deref(), Some("p1:c"));
    assert!(!past.loading);
}

#[test]
fn replacing_keeps_the_selected_item_even_when_the_new_page_leaves_it_out() {
    let mut past = past_of(vec![item("a", "A", 1_000), item("far", "Far", 9_000)], None);
    past.replace(
        vec![item("a", "A", 1_000)],
        Some("p1:a".into()),
        5,
        Some("far"),
    );
    let ids: Vec<&str> = past.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["a", "far"]);
    past.replace(vec![item("a", "A", 1_000)], None, 6, Some("unknown"));
    assert_eq!(past.items.len(), 1);
}

#[test]
fn a_load_that_began_before_a_reload_gets_the_current_past_rows() {
    let live = session("job-1", AgentGroup::Working, None, Tool::Claude);
    let old = session_of(&item("p1", "Old title", 2_000));
    let past = past_of(
        vec![item("p1", "New title", 2_000), item("p2", "Added", 3_000)],
        None,
    );
    let all = with_past(vec![live.clone(), old.clone()], &[old], &past);
    let m = model(true, &all, &past);
    assert_eq!(names(&m), ["New title", "Added"]);
    assert_eq!(all.len(), 3);
    assert_eq!(all[0], live);
}

#[test]
fn a_real_ended_row_still_wins_when_the_past_rows_are_rebuilt() {
    let real = ended("same", "Real ended", 1_000);
    let past = past_of(vec![item("same", "From file", 2_000)], None);
    let all = with_past(vec![real.clone()], &[], &past);
    assert_eq!(all, vec![real]);
}
