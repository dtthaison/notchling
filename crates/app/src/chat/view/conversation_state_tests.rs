use super::*;

#[test]
fn toggle_opens_then_closes() {
    let mut s = ConversationState::new();
    assert!(!s.is_open("g1"));
    s.toggle("g1");
    assert!(s.is_open("g1"));
    assert!(!s.is_open("g2"));
    s.toggle("g1");
    assert!(!s.is_open("g1"));
}

#[test]
fn expand_all_opens_everything() {
    let mut s = ConversationState::new();
    s.expand_all = true;
    assert!(s.is_open("anything"));
    assert!(!s.expanded.contains("anything"));
}

#[test]
fn hover_asks_for_a_redraw_only_when_times_are_stale() {
    let mut s = ConversationState::new();
    s.drawn_ms = now_ms();
    assert!(!s.hover("m1", true), "drawn just now: the times are fresh");
    assert_eq!(s.hovered_message.as_deref(), Some("m1"));
    s.drawn_ms = 0;
    assert!(s.hover("m2", true), "drawn long ago: draw the times again");
    assert_eq!(s.hovered_message.as_deref(), Some("m2"));
    assert!(!s.hover("m1", false));
    assert_eq!(s.hovered_message.as_deref(), Some("m2"));
    assert!(!s.hover("m2", false));
    assert_eq!(s.hovered_message, None);
}

#[test]
fn the_pill_shows_when_scrolled_up_and_not_while_loading() {
    let mut s = ConversationState::new();
    assert!(!s.latest_pill(false));
    s.latest_preview = true;
    assert!(s.latest_pill(false));
    assert!(!s.latest_pill(true));
    s.jump_to_latest();
    assert!(!s.latest_preview);
    assert!(!s.latest_pill(false));
}

fn input() -> ConversationInput {
    ConversationInput {
        shown: Arc::default(),
        subagent: None,
        session: Some(SessionShown {
            id: "s1".into(),
            tool: Tool::Claude,
            cwd: "/work".into(),
            working: false,
        }),
        held_working: false,
        outgoing: Vec::new(),
        loading: false,
        empty_note: None,
        subagents: Vec::new(),
        tasks_inset: false,
        selected: Some("s1".into()),
        can_run: false,
        chat: WeakEntity::new_invalid(),
    }
}

#[test]
fn the_input_is_equal_when_only_a_draft_or_a_menu_changed() {
    let a = input();
    let mut b = input();
    b.chat = WeakEntity::new_invalid();
    assert!(a == b);
    let mut c = input();
    c.shown = Arc::new(Conversation::default());
    assert!(a == c);
}

#[test]
fn a_new_conversation_arc_changes_the_input() {
    let a = input();
    let mut b = input();
    let mut next = Conversation::default();
    next.queued.push("one more".into());
    b.shown = Arc::new(next);
    assert!(a != b);
}

#[test]
fn what_the_column_draws_from_the_session_changes_the_input() {
    let a = input();
    let mut working = input();
    working.session.as_mut().unwrap().working = true;
    assert!(a != working);
    let mut loading = input();
    loading.loading = true;
    assert!(a != loading);
    let mut other = input();
    other.selected = Some("s2".into());
    assert!(a != other);
    let mut links = input();
    links.subagents = vec![(Some("call".into()), "agent".into())];
    assert!(a != links);
}

#[test]
fn run_showing_or_not_is_a_change_the_column_draws() {
    let mut shown = input();
    shown.can_run = true;
    assert!(shown != input());
}
