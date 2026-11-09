//! The live line at the end of the conversation while a session works (the Swift app's live row in
//! `ConversationView`): what it is doing, then "(50s · ↓ 2.9k tokens)" since your prompt. A turn
//! that has ended while the session still works is a background command, and the line says so.
//! Pure, so the wording is tested on every system; the view draws it and redraws once a second
//! only while there is one.

use notchling_engine::transcript::model::{Conversation, ItemKind, ToolCall, ToolKind};

/// A Stop sent to a session counts as in flight this long, or until the session stops working.
pub const STOPPING_MS: i64 = 20_000;

/// What the live line says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    /// "Thinking…", "Running cargo test…", "Writing…", "2 background commands running".
    pub text: String,
    /// "50s · ↓ 2.9k tokens" (the view wraps it in parentheses); none once the turn is over.
    pub stats: Option<String>,
    /// Background commands still running beside a live turn ("1 background command running").
    pub shells: Option<String>,
}

/// "1 background command running", "2 background commands running".
fn shells_text(n: usize) -> String {
    if n == 1 {
        "1 background command running".to_owned()
    } else {
        format!("{n} background commands running")
    }
}

/// "47s · ↓ 2.3k tokens", "3m 5s · ↓ 12k tokens" (the Swift `ChatFormat.workStats`); the tokens
/// are left out while there are none.
pub fn work_stats(seconds: i64, tokens: u64) -> String {
    let s = seconds.max(0);
    let time = if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {}s", s / 60, s % 60)
    } else {
        format!("{}h {}m", s / 3600, s % 3600 / 60)
    };
    if tokens == 0 {
        return time;
    }
    let count = if tokens < 1000 {
        tokens.to_string()
    } else if tokens < 10_000 {
        format!("{:.1}k", tokens as f64 / 1000.0)
    } else {
        format!("{}k", tokens / 1000)
    };
    format!("{time} · ↓ {count} tokens")
}

/// A call's title as something happening now: "Ran cargo test" is "Running cargo test", "Read a.rs"
/// is "Reading a.rs". A Bash call's own description reads as it is.
fn present(call: &ToolCall) -> String {
    const NOW: [(&str, &str); 8] = [
        ("Ran ", "Running "),
        ("Read ", "Reading "),
        ("Edited ", "Editing "),
        ("Wrote ", "Writing "),
        ("Searched the web for ", "Searching the web for "),
        ("Searched for ", "Searching for "),
        ("Found files matching ", "Finding files matching "),
        ("Used ", "Using "),
    ];
    let title = call.title.trim();
    for (past, now) in NOW {
        if let Some(rest) = title.strip_prefix(past) {
            return format!("{now}{rest}");
        }
    }
    if call.kind == ToolKind::Shell && title.is_empty() {
        return "Running a command".to_owned();
    }
    title.to_owned()
}

/// What the newest item says is going on, as a verb with its object.
fn doing(conversation: &Conversation) -> String {
    match conversation.items.last().map(|item| &item.kind) {
        Some(ItemKind::Tools(calls)) => match calls.iter().rev().find(|call| !call.finished) {
            Some(call) => format!("{}…", present(call).trim_end_matches('…')),
            // Every call has its result: the model is reading them.
            None => "Thinking…".to_owned(),
        },
        Some(ItemKind::Assistant { .. }) => "Writing…".to_owned(),
        _ => "Thinking…".to_owned(),
    }
}

/// The live line, or none. `working` is the session's own state (a working group, a running turn):
/// without it nothing is shown, whatever a stale transcript says. `now_s` is the wall clock in
/// seconds since the epoch, like the transcript's timestamps.
pub fn line(conversation: &Conversation, working: bool, now_s: f64) -> Option<Activity> {
    if !working {
        return None;
    }
    // A shell counts until its notification says it ended.
    let shells = conversation
        .shells
        .iter()
        .filter(|shell| !conversation.finished_agents.contains_key(&shell.id))
        .count();
    let turn_over = matches!(
        conversation.items.last().map(|item| &item.kind),
        Some(ItemKind::TurnEnd { .. })
    );
    if turn_over {
        // Working outlasts the turn only for a command left running in the background.
        return (shells > 0).then(|| Activity {
            text: shells_text(shells),
            stats: None,
            shells: None,
        });
    }
    let stats = conversation
        .turn_start
        .map(|start| work_stats((now_s - start).floor() as i64, conversation.turn_tokens));
    Some(Activity {
        text: doing(conversation),
        stats,
        shells: (shells > 0).then(|| shells_text(shells)),
    })
}

/// Whether a Stop sent at `stopped_ms` is still on its way: the session works and it was sent
/// less than [`STOPPING_MS`] ago.
pub fn is_stopping(stopped_ms: Option<i64>, working: bool, now_ms: i64) -> bool {
    working && stopped_ms.is_some_and(|at| now_ms - at < STOPPING_MS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::transcript::model::{BackgroundShell, ChatItem};

    fn item(kind: ItemKind) -> ChatItem {
        ChatItem {
            id: "i".into(),
            kind,
            date: None,
        }
    }

    fn call(title: &str, kind: ToolKind, finished: bool) -> ToolCall {
        let mut c = ToolCall::new("t", "Bash", kind, title.into(), String::new());
        c.finished = finished;
        c
    }

    fn shell(id: &str) -> BackgroundShell {
        BackgroundShell {
            id: id.into(),
            command: "sleep 60".into(),
            title: String::new(),
            output_path: format!("/tmp/{id}.output"),
            started: None,
        }
    }

    fn conversation(items: Vec<ItemKind>) -> Conversation {
        Conversation {
            items: items.into_iter().map(item).collect(),
            ..Conversation::default()
        }
    }

    #[test]
    fn work_stats_reads_like_the_swift_app() {
        assert_eq!(work_stats(47, 0), "47s");
        assert_eq!(work_stats(47, 2300), "47s · ↓ 2.3k tokens");
        assert_eq!(work_stats(50, 2_900), "50s · ↓ 2.9k tokens");
        assert_eq!(work_stats(185, 12_345), "3m 5s · ↓ 12k tokens");
        assert_eq!(work_stats(3_700, 950), "1h 1m · ↓ 950 tokens");
        assert_eq!(work_stats(-4, 0), "0s");
    }

    #[test]
    fn nothing_shows_while_the_session_is_idle() {
        let mut c = conversation(vec![ItemKind::Assistant { text: "hi".into() }]);
        c.shells.push(shell("b1"));
        assert_eq!(line(&c, false, 100.0), None);
    }

    #[test]
    fn a_session_with_no_output_yet_is_thinking() {
        let a = line(&Conversation::default(), true, 100.0).expect("a line");
        assert_eq!(a.text, "Thinking…");
        assert_eq!(a.stats, None, "no prompt time to count from");
    }

    #[test]
    fn the_verb_follows_the_last_item() {
        let thinking = conversation(vec![ItemKind::Thinking {
            seconds: 2.0,
            text: String::new(),
        }]);
        assert_eq!(line(&thinking, true, 0.0).unwrap().text, "Thinking…");
        let user = conversation(vec![ItemKind::User {
            text: "go".into(),
            images: 0,
        }]);
        assert_eq!(line(&user, true, 0.0).unwrap().text, "Thinking…");
        let writing = conversation(vec![ItemKind::Assistant { text: "A".into() }]);
        assert_eq!(line(&writing, true, 0.0).unwrap().text, "Writing…");
        let waiting = conversation(vec![ItemKind::Tools(vec![call(
            "Ran ls",
            ToolKind::Shell,
            true,
        )])]);
        assert_eq!(
            line(&waiting, true, 0.0).unwrap().text,
            "Thinking…",
            "every call answered: the model is reading"
        );
    }

    #[test]
    fn a_call_without_its_result_is_what_it_is_doing() {
        let running = conversation(vec![ItemKind::Tools(vec![
            call("Read a.rs", ToolKind::Read, true),
            call("Ran cargo test -p x", ToolKind::Shell, false),
        ])]);
        assert_eq!(
            line(&running, true, 0.0).unwrap().text,
            "Running cargo test -p x…"
        );
        let reading = conversation(vec![ItemKind::Tools(vec![call(
            "Read b.rs",
            ToolKind::Read,
            false,
        )])]);
        assert_eq!(line(&reading, true, 0.0).unwrap().text, "Reading b.rs…");
        // A Bash call's description is its title as given.
        let described = conversation(vec![ItemKind::Tools(vec![call(
            "Run the test suite",
            ToolKind::Shell,
            false,
        )])]);
        assert_eq!(
            line(&described, true, 0.0).unwrap().text,
            "Run the test suite…"
        );
    }

    #[test]
    fn the_clock_and_tokens_count_from_the_prompt() {
        let mut c = conversation(vec![ItemKind::Assistant { text: "A".into() }]);
        c.turn_start = Some(1_000.0);
        c.turn_tokens = 2_900;
        let a = line(&c, true, 1_050.4).unwrap();
        assert_eq!(a.stats.as_deref(), Some("50s · ↓ 2.9k tokens"));
        // The clock moving is all that changes.
        let later = line(&c, true, 1_061.0).unwrap();
        assert_eq!(later.stats.as_deref(), Some("1m 1s · ↓ 2.9k tokens"));
    }

    #[test]
    fn a_turn_over_but_working_session_is_a_background_command() {
        let mut c = conversation(vec![
            ItemKind::Assistant {
                text: "Started".into(),
            },
            ItemKind::TurnEnd {
                seconds: 5.0,
                model: None,
            },
        ]);
        c.turn_start = Some(1.0);
        c.shells = vec![shell("b1"), shell("b2")];
        let a = line(&c, true, 100.0).unwrap();
        assert_eq!(a.text, "2 background commands running");
        assert_eq!(a.stats, None, "no clock once the turn is over");
        c.finished_agents.insert("b2".into(), "completed".into());
        assert_eq!(
            line(&c, true, 100.0).unwrap().text,
            "1 background command running"
        );
        c.finished_agents.insert("b1".into(), "killed".into());
        assert_eq!(line(&c, true, 100.0), None, "all ended: nothing to show");
    }

    #[test]
    fn shells_show_beside_a_live_turn() {
        let mut c = conversation(vec![ItemKind::Assistant { text: "A".into() }]);
        c.shells = vec![shell("b1")];
        let a = line(&c, true, 0.0).unwrap();
        assert_eq!(a.text, "Writing…");
        assert_eq!(a.shells.as_deref(), Some("1 background command running"));
    }

    #[test]
    fn a_stop_is_on_its_way_for_twenty_seconds_while_the_session_works() {
        assert!(!is_stopping(None, true, 5));
        assert!(is_stopping(Some(0), true, STOPPING_MS - 1));
        assert!(!is_stopping(Some(0), true, STOPPING_MS));
        assert!(!is_stopping(Some(0), false, 1), "it stopped");
    }
}
