use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_locale as t;

use super::answer::permission_choices;
use super::model::{ago, empty_text, short_host, tab_id, tab_title, NotchModel};
use super::prefs::NotchPrefs;
use crate::acting::Decision;

fn agent(group: AgentGroup) -> Agent {
    Agent {
        id: "a".into(),
        name: "a".into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group,
        label: "Working".into(),
        detail: None,
        since_ms: 0,
        target: AgentTarget::Job("job-a".into()),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
    }
}

#[test]
fn tab_titles_and_empty_texts_are_messages() {
    assert_eq!(tab_title(AgentGroup::NeedsInput), t::notch::TAB_INPUT.get());
    assert_eq!(tab_title(AgentGroup::Working), t::notch::TAB_WORKING.get());
    assert_eq!(tab_title(AgentGroup::Completed), t::notch::TAB_DONE.get());
    assert_eq!(t::notch::TAB_INPUT.english(), "Input");
    assert_eq!(t::notch::TAB_WORKING.english(), "Working");
    assert_eq!(t::notch::TAB_DONE.english(), "Done");

    let mut model = NotchModel::default();
    assert_eq!(empty_text(&model), t::notch::EMPTY_NO_AGENTS.get());
    assert_eq!(
        t::notch::EMPTY_NO_AGENTS.english(),
        "No Claude agents running"
    );
    model.agents = vec![agent(AgentGroup::Working)];
    for (group, message, english) in [
        (
            AgentGroup::NeedsInput,
            t::notch::EMPTY_INPUT,
            "Nothing needs input",
        ),
        (
            AgentGroup::Working,
            t::notch::EMPTY_WORKING,
            "Nothing working",
        ),
        (
            AgentGroup::Completed,
            t::notch::EMPTY_COMPLETED,
            "Nothing completed",
        ),
    ] {
        model.tab = group;
        assert_eq!(empty_text(&model), message.get());
        assert_eq!(message.english(), english);
    }
}

#[test]
fn tab_ids_stay_english_in_every_language() {
    assert_eq!(tab_id(AgentGroup::NeedsInput), "input");
    assert_eq!(tab_id(AgentGroup::Working), "working");
    assert_eq!(tab_id(AgentGroup::Completed), "done");
}

#[test]
fn ages_come_from_messages_and_keep_their_english() {
    assert_eq!(ago(0, 30_000), t::notch::AGE_NOW.get());
    assert_eq!(ago(0, 30_000), "now");
    assert_eq!(ago(0, 5 * 60_000), t::notch::age_minutes(5));
    assert_eq!(ago(0, 5 * 60_000), "5m");
    assert_eq!(ago(0, 3 * 3_600_000), t::notch::age_hours(3));
    assert_eq!(ago(0, 3 * 3_600_000), "3h");
    assert_eq!(ago(0, 2 * 86_400_000), t::notch::age_days(2));
    assert_eq!(ago(0, 2 * 86_400_000), "2d");
}

#[test]
fn the_background_host_is_short_but_other_hosts_are_not_touched() {
    assert_eq!(short_host("Background"), t::notch::HOST_BACKGROUND.get());
    assert_eq!(short_host("Background"), "bg");
    assert_eq!(short_host("claude -p"), "-p");
    assert_eq!(short_host("Terminal"), "Terminal");
}

#[test]
fn a_held_session_says_it_needs_input_in_the_notchs_own_words() {
    assert_eq!(t::notch::NEEDS_INPUT.english(), "Needs input");
    assert_eq!(t::notch::THINKING.english(), "Thinking…");
}

#[test]
fn sentences_with_variables_keep_their_english() {
    assert_eq!(
        t::notch::wants_to_use("Bash", "rm -rf build"),
        "Wants to use Bash: rm -rf build"
    );
    assert_eq!(t::notch::asks("Which db?"), "Asks: Which db?");
    assert_eq!(t::notch::you("hello"), "You: hello");
    assert_eq!(t::notch::row_host("bg", "5m"), "bg · 5m");
    assert_eq!(
        t::notch::detail_sub("Working", "Background", "5m"),
        "Working · Background · 5m"
    );
    assert_eq!(t::notch::question_tab(2), "Question 2");
    assert_eq!(t::notch::question_step(1, 3), "1 of 3");
}

#[test]
fn the_card_words_keep_their_english() {
    for (message, english) in [
        (t::notch::OPEN, "Open ›"),
        (t::notch::NO_MESSAGES, "No messages yet"),
        (t::notch::EXITED, "This session has exited."),
        (
            t::notch::READ_ONLY,
            "Read only here. Reply where the session runs.",
        ),
        (t::notch::CLAUDE_ASKS, "Claude asks"),
        (t::notch::PICK_ANY, "Pick any that apply"),
        (t::notch::TYPE_ELSE, "Type something else…"),
        (t::notch::NEXT, "Next"),
        (t::notch::SUBMIT, "Submit"),
        (
            t::notch::ANSWER_OFF,
            "Answering from the notch is off in Settings.",
        ),
    ] {
        assert_eq!(message.english(), english);
    }
}

#[test]
fn the_answer_gate_says_why_with_the_message() {
    let off = NotchPrefs {
        answer: false,
        ..NotchPrefs::default()
    };
    let on = crate::acting::Control {
        enabled: true,
        tooltip: None,
    };
    let gate = off.answer_gate(on);
    assert_eq!(gate.tooltip, Some(t::notch::ANSWER_OFF.get()));
}

#[test]
fn permission_buttons_use_consent() {
    let with = permission_choices(true);
    let without = permission_choices(false);
    assert_eq!(with.len(), 3);
    assert_eq!(without.len(), 2);
    assert_eq!(
        with[0],
        (
            Decision::Allow,
            t::consent::NOTCH_ALLOW.get(),
            t::consent::NOTCH_ALLOW_DETAIL.get()
        )
    );
    assert_eq!(
        with[1],
        (
            Decision::Always,
            t::consent::NOTCH_ALWAYS.get(),
            t::consent::NOTCH_ALWAYS_DETAIL.get()
        )
    );
    assert_eq!(
        with[2],
        (
            Decision::Deny,
            t::consent::NOTCH_DENY.get(),
            t::consent::NOTCH_DENY_DETAIL.get()
        )
    );
    assert_eq!(without[1].0, Decision::Deny);
    let english: Vec<(&str, &str)> = with
        .iter()
        .map(|(_, title, detail)| (*title, *detail))
        .collect();
    assert_eq!(
        english,
        [
            ("Allow", "This time only"),
            ("Always allow this", "Don't ask again for this"),
            ("Deny", "Claude tries something else"),
        ]
    );
    assert_eq!(
        t::consent::notch_wants_to_use("Bash"),
        "Claude wants to use Bash"
    );
    assert_eq!(
        t::consent::NOTCH_PERMISSION_NEEDED.english(),
        "Permission needed"
    );
}
