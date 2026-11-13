use super::*;
use crate::notch::layout::{NotchState, Size, MAX_LIST};
use notchling_engine::transcript::snapshot::{ChatLine, LineKind};

fn agent(id: &str, group: AgentGroup) -> Agent {
    Agent {
        id: id.into(),
        name: id.into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group,
        label: match group {
            AgentGroup::NeedsInput => "Needs input",
            AgentGroup::Working => "Working",
            AgentGroup::Completed => "Done",
        }
        .into(),
        detail: None,
        since_ms: 0,
        target: AgentTarget::Job(format!("job-{id}")),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
    }
}

fn reply(text: &str) -> Snapshot {
    Snapshot {
        lines: vec![ChatLine {
            id: 0,
            kind: LineKind::Assistant,
            text: text.into(),
        }],
        ..Snapshot::default()
    }
}

fn permission(session: &str) -> HookRequest {
    HookRequest {
        id: "r1".into(),
        session_id: session.into(),
        created: 0.0,
        kind: RequestKind::Permission,
        tool_name: "Bash".into(),
        summary: "rm -rf build".into(),
        can_always_allow: true,
        agy: None,
    }
}

fn model(agents: Vec<Agent>) -> NotchModel {
    NotchModel {
        agents,
        ..NotchModel::default()
    }
}

#[test]
fn questions_come_from_the_hook_then_the_transcript_then_the_job_summary() {
    let mut a = agent("j", AgentGroup::NeedsInput);
    a.detail = Some("answer: Which target? (Production · Staging · Local)".into());
    let mut m = model(vec![a]);
    let q = m.question("j").unwrap();
    assert_eq!(q.items[0].question, "Which target?");
    assert_eq!(q.items[0].options, vec!["Production", "Staging", "Local"]);
    assert!(q.tool_use_id.starts_with("needs:j:"));

    let snap = Snapshot {
        question: Some(PendingQuestion {
            tool_use_id: "tu1".into(),
            items: vec![],
        }),
        ..Snapshot::default()
    };
    m.snapshots.insert("j".into(), snap);
    assert_eq!(m.question("j").unwrap().tool_use_id, "tu1");

    let mut hook = permission("j");
    hook.kind = RequestKind::Question(PendingQuestion {
        tool_use_id: "hook:r1".into(),
        items: vec![],
    });
    m.requests.insert("j".into(), hook);
    assert_eq!(m.question("j").unwrap().tool_use_id, "hook:r1");
    assert_eq!(m.permission("j"), None);
}

#[test]
fn the_job_summary_refuses_what_it_cannot_carry() {
    let with = |group, detail: &str| {
        let mut a = agent("j", group);
        a.detail = Some(detail.into());
        question_from_needs(&a)
    };
    assert!(with(AgentGroup::NeedsInput, "answer: Pick? (a · b)").is_some());
    assert!(with(AgentGroup::Working, "answer: Pick? (a · b)").is_none());
    assert!(with(AgentGroup::NeedsInput, "answer: Pick? (only)").is_none());
    assert!(with(AgentGroup::NeedsInput, "answer: Pick (a size)? (S · L)").is_none());
    assert!(with(AgentGroup::NeedsInput, "permission: Bash").is_none());
    let mut terminal = agent("t", AgentGroup::NeedsInput);
    terminal.target = AgentTarget::Process(42);
    terminal.detail = Some("answer: Pick? (a · b)".into());
    assert!(question_from_needs(&terminal).is_none());
}

#[test]
fn a_permission_card_beats_a_question() {
    let mut a = agent("j", AgentGroup::NeedsInput);
    a.detail = Some("answer: Pick? (a · b)".into());
    let mut m = model(vec![a]);
    assert!(matches!(m.card("j"), Some(Card::Question(_))));
    m.requests.insert("j".into(), permission("j"));
    assert!(matches!(m.card("j"), Some(Card::Permission(_))));
    assert_eq!(
        m.status_line(m.agent("j").unwrap()),
        "Wants to use Bash: rm -rf build"
    );
}

#[test]
fn a_permission_peek_is_taller_than_a_plain_one() {
    let mut m = model(vec![
        agent("j", AgentGroup::Working),
        agent("ask", AgentGroup::NeedsInput),
    ]);
    m.requests.insert("ask".into(), permission("ask"));
    m.notch = Size::new(185.0, 32.0);
    assert_eq!(m.count(AgentGroup::Working), 1);
    assert_eq!(m.count(AgentGroup::NeedsInput), 1);
    let ask = m.size(&NotchState::Peek("ask".into()));
    let plain = m.size(&NotchState::Peek("j".into()));
    assert!(ask.h > plain.h);
    assert_eq!(ask.w, m.width.panel());
    assert_eq!(plain, Size::new(m.width.list(), 48.0 + 56.0));
}

#[test]
fn sizes_through_the_model() {
    let mut m = model(vec![
        agent("a", AgentGroup::Working),
        agent("b", AgentGroup::Working),
        agent("c", AgentGroup::Completed),
    ]);
    let prompted = Snapshot {
        last_prompt: Some("fix it".into()),
        ..Snapshot::default()
    };
    m.snapshots.insert("a".into(), prompted);
    m.snapshots.insert("c".into(), reply("done"));
    assert_eq!(m.size(&NotchState::Collapsed), Size::new(201.0, 32.0));
    m.tab = AgentGroup::Working;
    assert_eq!(
        m.size(&NotchState::List).h,
        48.0 + 34.0 + (62.0 + 48.0) + 6.0
    );
    m.tab = AgentGroup::Completed;
    assert_eq!(
        m.size(&NotchState::List).h,
        48.0 + 34.0 + (28.0 + 48.0) + 6.0
    );
    m.tab = AgentGroup::NeedsInput;
    assert_eq!(m.size(&NotchState::List).h, 48.0 + 34.0 + 44.0 + 6.0);
    assert!(m.size(&NotchState::List).h < 48.0 + 34.0 + MAX_LIST + 6.0);
    assert_eq!(m.size(&NotchState::Peek("c".into())).h, 48.0 + 100.0);
    m.show_last_reply = false;
    assert_eq!(m.size(&NotchState::Peek("c".into())).h, 48.0 + 56.0);
    assert_eq!(m.size(&NotchState::Detail("a".into())).h, 48.0 + 420.0);
}

#[test]
fn detail_is_taller_while_a_question_waits() {
    let mut a = agent("j", AgentGroup::NeedsInput);
    a.detail = Some("answer: Pick? (a · b)".into());
    let m = model(vec![a, agent("k", AgentGroup::Working)]);
    assert_eq!(m.size(&NotchState::Detail("j".into())).h, 48.0 + 470.0);
    assert_eq!(m.size(&NotchState::Detail("k".into())).h, 48.0 + 420.0);
}

#[test]
fn opening_the_list_picks_the_tab_most_likely_to_need_you() {
    let mut m = model(vec![
        agent("w", AgentGroup::Working),
        agent("d", AgentGroup::Completed),
    ]);
    m.tab = AgentGroup::Completed;
    m.go(NotchState::List);
    assert_eq!(m.tab, AgentGroup::Working);
    m.tab = AgentGroup::Completed;
    m.open("d");
    m.go(NotchState::List);
    assert_eq!(m.tab, AgentGroup::Completed);
    let mut empty = model(vec![]);
    empty.tab = AgentGroup::Working;
    empty.go(NotchState::List);
    assert_eq!(empty.tab, AgentGroup::NeedsInput);
}

#[test]
fn an_open_conversation_outlives_its_row() {
    let mut m = model(vec![agent("a", AgentGroup::Working)]);
    m.open("a");
    m.update(vec![], HashMap::new(), HashMap::new());
    assert_eq!(m.agent("a").map(|a| a.id.as_str()), Some("a"));
}

#[test]
fn hovering_opens_after_a_moment_and_leaving_folds_after_one() {
    let mut m = model(vec![agent("a", AgentGroup::Working)]);
    m.hover(true, 1_000);
    assert!(!m.tick(1_100), "too soon");
    assert!(m.tick(1_250));
    assert_eq!(m.state, NotchState::List);
    m.hover(false, 2_000);
    m.hover(true, 2_300);
    assert!(!m.tick(2_700));
    assert_eq!(m.state, NotchState::List);
    m.hover(false, 3_000);
    assert!(m.tick(3_600));
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn a_brush_past_the_strip_does_not_open_it() {
    let mut m = model(vec![]);
    m.hover(true, 0);
    m.hover(false, 100);
    assert!(!m.tick(1_000));
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn a_peek_ends_on_its_own_and_waits_longer_for_a_card() {
    let mut m = model(vec![
        agent("a", AgentGroup::Completed),
        agent("q", AgentGroup::NeedsInput),
    ]);
    m.peek("a", 0);
    assert_eq!(m.state, NotchState::Peek("a".into()));
    assert!(!m.tick(7_999));
    assert!(m.tick(8_000));
    assert_eq!(m.state, NotchState::Collapsed);

    m.requests.insert("q".into(), permission("q"));
    m.peek("q", 10_000);
    assert!(!m.tick(10_000 + 44_999));
    m.hover(true, 20_000);
    assert!(!m.tick(100_000));
    m.hover(false, 100_000);
    assert!(!m.tick(144_999));
    assert!(m.tick(145_000));
    assert_eq!(m.state, NotchState::Collapsed);
}

#[test]
fn a_peek_never_covers_the_open_list_or_one_being_read() {
    let mut m = model(vec![
        agent("a", AgentGroup::Completed),
        agent("b", AgentGroup::Completed),
    ]);
    m.go(NotchState::List);
    m.peek("a", 0);
    assert_eq!(m.state, NotchState::List);
    m.go(NotchState::Collapsed);
    m.peek("a", 0);
    m.hover(true, 10);
    m.peek("b", 20);
    assert_eq!(m.state, NotchState::Peek("a".into()));
}

#[test]
fn updates_peek_for_a_new_question_then_someone_waiting_then_something_done() {
    let mut m = NotchModel::default();
    let first = vec![
        agent("w1", AgentGroup::Working),
        agent("w2", AgentGroup::Working),
        agent("w3", AgentGroup::Working),
    ];
    assert_eq!(
        m.update(first, HashMap::new(), HashMap::new()),
        None,
        "baseline"
    );

    let done = vec![
        agent("w1", AgentGroup::Completed),
        agent("w2", AgentGroup::Working),
        agent("w3", AgentGroup::Working),
    ];
    assert_eq!(
        m.update(done.clone(), HashMap::new(), HashMap::new())
            .as_deref(),
        Some("w1")
    );
    assert_eq!(
        m.update(done, HashMap::new(), HashMap::new()),
        None,
        "no change"
    );

    let mut waiting = agent("w2", AgentGroup::NeedsInput);
    waiting.detail = Some("Continue?".into());
    let rows = vec![
        waiting,
        agent("w1", AgentGroup::Completed),
        agent("w3", AgentGroup::Completed),
    ];
    assert_eq!(
        m.update(rows.clone(), HashMap::new(), HashMap::new())
            .as_deref(),
        Some("w2")
    );

    let mut snaps = HashMap::new();
    let s = Snapshot {
        question: Some(PendingQuestion {
            tool_use_id: "tu9".into(),
            items: vec![],
        }),
        ..Snapshot::default()
    };
    snaps.insert("w3".into(), s);
    assert_eq!(m.update(rows, snaps, HashMap::new()).as_deref(), Some("w3"));
}

#[test]
fn status_lines_say_what_a_row_waits_on_or_does() {
    let mut m = model(vec![
        agent("w", AgentGroup::Working),
        agent("n", AgentGroup::NeedsInput),
    ]);
    assert_eq!(m.status_line(&m.agents[0].clone()), "Thinking…");
    let snap = Snapshot {
        activity: Some("Reading main.rs".into()),
        ..Snapshot::default()
    };
    m.snapshots.insert("w".into(), snap);
    assert_eq!(m.status_line(&m.agents[0].clone()), "Reading main.rs");
    assert_eq!(m.status_line(&m.agents[1].clone()), "Needs input");
    m.snapshots.insert("n".into(), reply("Shall I go on?"));
    assert_eq!(m.status_line(&m.agents[1].clone()), "Shall I go on?");
}

#[test]
fn small_formatters() {
    assert_eq!(ago(0, 59_000), "now");
    assert_eq!(ago(0, 60_000), "1m");
    assert_eq!(ago(0, 3_600_000), "1h");
    assert_eq!(ago(0, 2 * 86_400_000), "2d");
    assert_eq!(ago(10_000, 0), "now", "a clock behind the file is now");
    assert_eq!(short_host("Background"), "bg");
    assert_eq!(short_host("claude -p"), "-p");
    assert_eq!(short_host("iTerm"), "iTerm");
    assert_eq!(one_line(" a\nb "), "a b");
    assert_eq!(last_paragraph("one\n\ntwo\n\n  \n"), "two");
    assert_eq!(last_paragraph(&"x".repeat(500)).len(), 400);
    assert_eq!(status(AgentGroup::NeedsInput), Status::Needs);
    assert_eq!(TABS.map(tab_title), ["Input", "Working", "Done"]);
    let mut m = model(vec![]);
    assert_eq!(empty_text(&m), "No Claude agents running");
    m.agents.push(agent("a", AgentGroup::Working));
    assert_eq!(empty_text(&m), "Nothing needs input");
}

#[test]
fn an_answered_card_hides_until_the_files_catch_up() {
    let mut a = agent("j", AgentGroup::NeedsInput);
    a.detail = Some("answer: Pick? (a · b)".into());
    let mut m = model(vec![a]);
    m.requests.insert("j".into(), permission("j"));
    m.mark_answered("j");
    assert!(m.answered.contains("r1"), "a hook request by its id");
    assert!(matches!(m.card("j"), Some(Card::Question(_))));
    m.requests.clear();
    m.mark_answered("j");
    assert_eq!(m.card("j"), None, "a question by its tool use id");
    m.mark_answered("nobody");
}

#[test]
fn an_answer_hides_the_prompt_it_answered_and_not_one_that_came_after() {
    let mut m = model(vec![agent("j", AgentGroup::NeedsInput)]);
    m.requests.insert("j".into(), permission("j"));
    let key = m.answer_key("j").expect("a prompt to answer");
    assert_eq!(key, "r1");
    let mut newer = permission("j");
    newer.id = "r2".into();
    m.requests.insert("j".into(), newer);
    m.mark_answered_key(&key);
    assert!(m.answered.contains("r1"));
    assert!(
        matches!(m.card("j"), Some(Card::Permission(r)) if r.id == "r2"),
        "the newer prompt still shows"
    );
    assert_eq!(m.answer_key("j").as_deref(), Some("r2"));
    assert_eq!(m.answer_key("nobody"), None);
}

#[test]
fn the_settings_switches_set_the_width_the_peek_text_and_fold_a_hidden_notch() {
    let mut m = model(vec![agent("a", AgentGroup::Completed)]);
    m.snapshots.insert("a".into(), reply("All done"));
    m.go(NotchState::List);
    let medium = m.size(&NotchState::List).w;
    let prefs = NotchPrefs {
        width: NotchWidth::Large,
        show_last_reply: false,
        ..NotchPrefs::default()
    };
    assert!(m.apply(&prefs));
    assert!(
        m.size(&NotchState::List).w > medium,
        "Large is wider than Medium"
    );
    let a = m.agents[0].clone();
    assert_eq!(m.peek_text(&a), None, "no last reply in a peek");
    assert_eq!(m.state, NotchState::List, "a shown notch keeps its state");
    assert!(!m.apply(&prefs), "nothing changed the second time");

    let hidden = NotchPrefs {
        shown: false,
        ..prefs
    };
    assert!(m.apply(&hidden));
    assert_eq!(
        m.state,
        NotchState::Collapsed,
        "a hidden notch comes back closed"
    );
}
