use notchling_locale as t;

use super::activity::work_stats;
use super::claude_picker::{effort_label, effort_title, model_label};
use super::composer::{add_menu, Add};
use super::data::Tool;
use super::markers;
use super::pickers;
use super::prompt_flow::{already_answered, confirmation, failure_text, sending_text, Sending};
use super::prompts::error_text;
use crate::acting::Decision;

fn english_values() -> Vec<String> {
    t::testing::messages("en")
        .into_iter()
        .filter_map(|(key, _)| t::lookup(key))
        .map(|text| text.english().to_owned())
        .collect()
}

#[test]
fn markers_stay_literal() {
    let values = english_values();
    assert!(values.len() > 100);
    let mut markers_all: Vec<&str> = vec![
        markers::EXITED,
        markers::GONE,
        markers::DIALOG,
        markers::ANSWER_IN_TERMINAL,
        markers::APPROVE,
        markers::DIDNT_ANSWER,
        markers::CLOSED_CONNECTION,
    ];
    markers_all.extend(markers::FILE_TOOLS);
    markers_all.extend(markers::NO_ALWAYS);
    markers_all.extend(markers::PAST_STEPS);
    for marker in markers_all {
        assert!(
            !values.iter().any(|v| v == marker),
            "{marker:?} is matched against another program's text and must stay a literal"
        );
    }
}

#[test]
fn the_activity_stats_read_as_they_did() {
    let table = [
        ((0, 0), "0s"),
        ((59, 0), "59s"),
        ((60, 0), "1m 0s"),
        ((125, 0), "2m 5s"),
        ((3_600, 0), "1h 0m"),
        ((4_320, 0), "1h 12m"),
        ((0, 500), "0s · ↓ 500 tokens"),
        ((12, 1_500), "12s · ↓ 1.5k tokens"),
        ((12, 2_000), "12s · ↓ 2.0k tokens"),
        ((12, 9_999), "12s · ↓ 10.0k tokens"),
        ((12, 12_345), "12s · ↓ 12k tokens"),
        ((12, 1_500_000), "12s · ↓ 1500k tokens"),
    ];
    for ((seconds, tokens), want) in table {
        assert_eq!(work_stats(seconds, tokens), want, "{seconds} s {tokens}");
    }
}

#[test]
fn the_activity_phrases_read_as_they_did() {
    assert_eq!(t::chat::doing_ran("ls"), "Running ls");
    assert_eq!(t::chat::doing_read("a.rs"), "Reading a.rs");
    assert_eq!(t::chat::doing_edited("a.rs"), "Editing a.rs");
    assert_eq!(t::chat::doing_wrote("a.rs"), "Writing a.rs");
    assert_eq!(t::chat::doing_searched_web("x"), "Searching the web for x");
    assert_eq!(t::chat::doing_searched("x"), "Searching for x");
    assert_eq!(t::chat::doing_found("*.rs"), "Finding files matching *.rs");
    assert_eq!(t::chat::doing_used("Skill"), "Using Skill");
    assert_eq!(t::chat::doing_command(), "Running a command");
    assert_eq!(t::chat::thinking(), "Thinking…");
    assert_eq!(t::chat::writing(), "Writing…");
    assert_eq!(t::chat::shells_running(1), "1 background command running");
    assert_eq!(t::chat::shells_running(3), "3 background commands running");
}

#[test]
fn a_decision_is_told_in_messages() {
    let allow = Decision::Allow;
    assert_eq!(
        confirmation("Bash", "ls -la", allow),
        t::chat::decision_tool_subject(t::chat::allowed(), "Bash", "ls -la")
    );
    assert_eq!(
        confirmation("Bash", "ls -la", allow),
        "Allowed: Bash ls -la"
    );
    assert_eq!(confirmation("Bash", "", allow), "Allowed: Bash");
    assert_eq!(
        confirmation("Write", "/x/a.rs", Decision::Always),
        "Always allowed: Write a.rs"
    );
    assert_eq!(
        confirmation("Bash", "ls", Decision::Deny),
        "Denied: Bash ls"
    );
    assert_eq!(
        already_answered("Bash", "ls"),
        t::chat::already_answered_subject("Bash", "ls")
    );
    assert_eq!(
        already_answered("Bash", "ls"),
        "Already answered in the terminal: Bash ls"
    );
    assert_eq!(
        already_answered("Bash", ""),
        "Already answered in the terminal: Bash"
    );
    let slow = failure_text("Claude Code didn't answer in time.");
    assert_eq!(
        slow,
        "Claude Code didn't answer in time. It may still have reached the session: look before sending again."
    );
    assert_eq!(failure_text("Nope."), "Nope.");
}

#[test]
fn what_is_being_sent_is_a_message() {
    assert_eq!(sending_text(Sending::Answer), t::chat::sending_note());
    let on = |decision, on_screen| Sending::Decision {
        decision,
        on_screen,
    };
    assert_eq!(sending_text(on(Decision::Allow, false)), "Allowing…");
    assert_eq!(
        sending_text(on(Decision::Always, false)),
        "Allowing, and not asking again…"
    );
    assert_eq!(sending_text(on(Decision::Deny, false)), "Denying…");
    assert_eq!(
        sending_text(on(Decision::Allow, true)),
        "Allowing… typing it into the session, a few seconds"
    );
    assert_eq!(
        sending_text(on(Decision::Always, true)),
        "Allowing, and not asking again… a few seconds"
    );
    assert_eq!(
        sending_text(on(Decision::Deny, true)),
        "Denying… typing it into the session, a few seconds"
    );
}

#[test]
fn a_dialog_error_points_above_when_the_card_is_shown() {
    let said = "The session is asking you something. Answer it in the terminal.";
    assert_eq!(
        error_text(said, true),
        "The session is asking you something. Answer it above."
    );
    assert_eq!(error_text(said, false), said);
    assert_eq!(t::chat::answer_above(), "Answer it above");
}

#[test]
fn the_pickers_are_made_of_messages() {
    let menus = pickers::claude(None, None, &[]);
    assert_eq!(menus[0].tip, t::chat::model_tip());
    assert_eq!(menus[1].tip, t::chat::effort_tip());
    assert_eq!(menus[0].label, t::chat::model_placeholder());
    assert_eq!(menus[1].label, t::chat::effort_placeholder());
    assert_eq!(model_label(None, &[]), "Model");
    assert_eq!(effort_label(None), "Effort");
    assert_eq!(effort_title("xhigh"), "Extra High");
    assert_eq!(
        t::chat::model_tip(),
        "The model this session uses. Pick another to switch it (like /model in the terminal)."
    );
    assert_eq!(
        t::chat::effort_tip(),
        "How hard the model thinks. Pick another to switch it (like /effort in the terminal, which also makes it the default for new sessions)."
    );
    assert_eq!(t::chat::no_efforts(), "This model has no effort levels");
    assert_eq!(t::chat::more_models(), "More Models");
    assert_eq!(
        t::chat::priced_as("Opus 5.5"),
        "Priced as Opus 5.5 until Notchling knows this model"
    );
    assert_eq!(
        t::chat::codex_model_tip(),
        "The model this Codex session's next turns use. Codex Default follows Codex's own setting."
    );
    assert_eq!(
        t::chat::codex_effort_tip(),
        "How hard the model reasons on this session's next turns."
    );
    assert_eq!(t::chat::default_model(), "Default Model");
    assert_eq!(t::chat::pick_model_first(), "Pick a model first.");
    assert_eq!(
        t::chat::agy_not_started(),
        "Your first message starts the session; then this can be picked here."
    );
    assert_eq!(
        t::chat::agy_opening(),
        "Antigravity is still opening this session."
    );
    assert_eq!(
        t::chat::agy_option_tip("Model"),
        "Model for this session's next turns."
    );
    let off = pickers::antigravity(&[], false, false);
    assert_eq!(off[0].tip, t::chat::agy_not_started());
    assert_eq!(off[0].label, t::chat::model_placeholder());
}

#[test]
fn the_add_menu_is_made_of_messages() {
    let titles: Vec<&str> = add_menu(Tool::Claude).iter().map(|i| i.title).collect();
    assert_eq!(
        titles,
        [
            t::chat::add_files(),
            t::chat::add_folder(),
            t::chat::slash_commands()
        ]
    );
    assert_eq!(
        titles,
        ["Add files or photos", "Add folder", "Slash commands"]
    );
    let codex: Vec<Add> = add_menu(Tool::Codex).iter().map(|i| i.add).collect();
    assert_eq!(codex.len(), 2);
}

#[test]
fn the_reply_boxs_words_are_what_they_were() {
    assert_eq!(
        t::chat::first_message_placeholder(),
        "Your first message starts the session"
    );
    assert_eq!(
        t::chat::messages_wait(1),
        "1 message waits for this turn to end."
    );
    assert_eq!(
        t::chat::messages_wait(2),
        "2 messages wait for this turn to end."
    );
    assert_eq!(
        t::chat::read_only(),
        "Read only here. Reply where the session runs."
    );
    assert_eq!(
        t::chat::agy_busy(),
        "Antigravity is still on the last turn. Send again when it ends."
    );
    assert_eq!(t::chat::session_exited(), "This session has exited.");
    assert_eq!(t::chat::change_failed("no"), "Couldn't change it: no");
    assert_eq!(t::chat::drop_hint(), "Drop to add to your reply");
    assert_eq!(t::chat::claude_asks(), "Claude asks");
    assert_eq!(t::chat::step_of(2, 3), "2 of 3");
    assert_eq!(t::chat::wants_to_use("Bash"), "Claude wants to use Bash");
    assert_eq!(t::chat::allow(), "Allow");
    assert_eq!(t::chat::allow_this_time(), "This time only");
    assert_eq!(t::chat::allow_always(), "Always allow this");
    assert_eq!(t::chat::allow_always_note(), "Don't ask again for this");
    assert_eq!(t::chat::deny(), "Deny");
    assert_eq!(t::chat::deny_note(), "Claude tries something else");
    assert_eq!(t::chat::permission_needed(), "Permission needed");
    assert_eq!(
        t::chat::second_opinion_tip(),
        "Second Opinion: ask Codex or Antigravity to review this"
    );
    assert_eq!(t::chat::second_opinion_of("a.rs"), "Second Opinion: a.rs");
    assert_eq!(
        t::chat::opinion_running("Codex", "a.rs"),
        "Codex is reviewing a.rs"
    );
    assert_eq!(t::chat::opinion_done("Codex", "a.rs"), "Codex on a.rs");
    assert_eq!(
        t::chat::opinion_failed("Codex", "a.rs"),
        "Codex couldn't review a.rs"
    );
    assert_eq!(
        t::chat::opinion_cancelled("Codex", "a.rs"),
        "Codex on a.rs, cancelled"
    );
    assert_eq!(t::chat::tasks_now("Port it"), "Now: Port it");
    assert_eq!(t::chat::tasks_all_done(), "All done");
}

#[test]
fn the_effort_title_still_capitalises_other_levels() {
    assert_eq!(effort_title("high"), "High");
    assert_eq!(effort_title(""), "");
}
