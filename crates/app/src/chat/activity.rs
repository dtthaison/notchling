use notchling_engine::transcript::model::{Conversation, ItemKind, ToolCall, ToolKind};
use notchling_locale as t;

use super::markers;

pub const STOPPING_MS: i64 = 20_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    pub text: String,
    pub stats: Option<String>,
    pub shells: Option<String>,
}

fn shells_text(n: usize) -> String {
    t::chat::shells_running(n)
}

pub fn work_stats(seconds: i64, tokens: u64) -> String {
    let s = seconds.max(0);
    let time = if s < 60 {
        t::units::duration_seconds(s)
    } else if s < 3600 {
        t::chat::stats_minutes(s / 60, s % 60)
    } else {
        t::units::duration_hours(s / 3600, s % 3600 / 60)
    };
    if tokens == 0 {
        return time;
    }
    let count = if tokens < 1000 {
        t::fmt::count(tokens)
    } else if tokens < 10_000 {
        t::units::thousands(&t::fmt::decimal(tokens as f64 / 1000.0, 1))
    } else {
        t::units::thousands(&(tokens / 1000).to_string())
    };
    t::chat::stats_tokens(&time, &count)
}

fn present(call: &ToolCall) -> String {
    const NOW: [fn(&str) -> String; 8] = [
        t::chat::doing_ran,
        t::chat::doing_read,
        t::chat::doing_edited,
        t::chat::doing_wrote,
        t::chat::doing_searched_web,
        t::chat::doing_searched,
        t::chat::doing_found,
        t::chat::doing_used,
    ];
    let title = call.title.trim();
    for (past, now) in markers::PAST_STEPS.into_iter().zip(NOW) {
        if let Some(rest) = title.strip_prefix(past) {
            return now(rest);
        }
    }
    if call.kind == ToolKind::Shell && title.is_empty() {
        return t::chat::doing_command().to_owned();
    }
    title.to_owned()
}

pub fn doing(conversation: &Conversation) -> String {
    match newest_step(conversation).map(|(_, kind)| kind) {
        Some(ItemKind::Tools(calls)) => match calls.iter().rev().find(|call| !call.finished) {
            Some(call) => format!("{}…", present(call).trim_end_matches('…')),
            None => t::chat::thinking().to_owned(),
        },
        Some(ItemKind::Assistant { .. }) => t::chat::writing().to_owned(),
        _ => t::chat::thinking().to_owned(),
    }
}

pub fn newest_unfinished_shell(conversation: &Conversation) -> Option<(String, String)> {
    conversation
        .shells
        .iter()
        .rev()
        .find(|shell| !conversation.finished_agents.contains_key(&shell.id))
        .map(|shell| {
            let name = if shell.title.is_empty() {
                shell.command.clone()
            } else {
                shell.title.clone()
            };
            (name, shell.output_path.clone())
        })
}

pub fn line(conversation: &Conversation, working: bool, now_s: f64) -> Option<Activity> {
    if !working {
        return None;
    }
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

fn newest_step(conversation: &Conversation) -> Option<(usize, &ItemKind)> {
    conversation
        .items
        .iter()
        .enumerate()
        .rev()
        .map(|(i, item)| (i, &item.kind))
        .find(|(_, kind)| !matches!(kind, ItemKind::Notice { .. }))
}

pub fn live_group(conversation: &Conversation, working: bool) -> Option<usize> {
    match newest_step(conversation) {
        Some((i, ItemKind::Tools(_))) if working => Some(i),
        _ => None,
    }
}

pub const STALL_S: f64 = 45.0;

pub fn stalled(conversation: &Conversation, working: bool, now_s: f64) -> Option<String> {
    if !working || conversation.turn_tokens > 0 {
        return None;
    }
    let start = conversation.turn_start?;
    if now_s - start < STALL_S {
        return None;
    }
    match conversation.items.last().map(|item| &item.kind) {
        Some(ItemKind::User { text, .. }) if !text.trim().is_empty() => Some(text.clone()),
        _ => None,
    }
}

pub fn is_stopping(stopped_ms: Option<i64>, working: bool, now_ms: i64) -> bool {
    working && stopped_ms.is_some_and(|at| now_ms - at < STOPPING_MS)
}

pub fn clock_due(drawn_ms: Option<i64>, now_ms: i64) -> bool {
    drawn_ms.is_some_and(|drawn| now_ms.div_euclid(1000) > drawn.div_euclid(1000))
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::transcript::model::{BackgroundShell, ChatItem};

    #[test]
    fn the_clock_is_drawn_again_once_a_second_while_the_line_shows() {
        assert!(!clock_due(None, 5_000), "not on screen: nothing to redraw");
        assert!(!clock_due(Some(5_000), 5_999), "same second");
        assert!(clock_due(Some(5_000), 6_000), "a new second");
        assert!(
            clock_due(Some(5_999), 6_000),
            "drawn late in a second, due at the next"
        );
        assert!(
            clock_due(Some(5_000), 9_500),
            "a late tick catches up in one redraw"
        );
        assert!(!clock_due(Some(6_000), 6_400), "drawn this second already");
        assert!(
            !clock_due(Some(6_000), 5_999),
            "a clock that stepped back waits"
        );
    }

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
    fn a_notice_at_the_end_does_not_hide_what_the_session_is_doing() {
        let note = ItemKind::notice("A delegation is running this session.", "lock");
        let running = conversation(vec![
            ItemKind::user("Check the image."),
            ItemKind::Tools(vec![call(
                "Ran sips -g pixelWidth logo.png",
                ToolKind::Shell,
                false,
            )]),
            note.clone(),
        ]);
        assert_eq!(
            line(&running, true, 0.0).unwrap().text,
            "Running sips -g pixelWidth logo.png…"
        );
        let reading = conversation(vec![
            ItemKind::user("Go."),
            ItemKind::Tools(vec![call("Read a.rs", ToolKind::Read, true)]),
            note.clone(),
        ]);
        assert_eq!(line(&reading, true, 0.0).unwrap().text, "Thinking…");
        let writing = conversation(vec![ItemKind::Assistant { text: "A".into() }, note.clone()]);
        assert_eq!(line(&writing, true, 0.0).unwrap().text, "Writing…");
        let only = conversation(vec![note]);
        assert_eq!(line(&only, true, 0.0).unwrap().text, "Thinking…");
    }

    #[test]
    fn the_live_group_is_the_newest_steps_with_only_notices_after_them() {
        let note = ItemKind::notice("Read only here.", "lock");
        let steps = ItemKind::Tools(vec![call("Read a.rs", ToolKind::Read, false)]);
        let reply = ItemKind::Assistant { text: "A".into() };
        let c = conversation(vec![ItemKind::user("Go."), steps.clone()]);
        assert_eq!(live_group(&c, true), Some(1));
        assert_eq!(live_group(&c, false), None, "an idle session has none");
        let held = conversation(vec![ItemKind::user("Go."), steps.clone(), note.clone()]);
        assert_eq!(live_group(&held, true), Some(1));
        let answered = conversation(vec![steps.clone(), reply, note]);
        assert_eq!(live_group(&answered, true), None, "a reply came after them");
        assert_eq!(live_group(&Conversation::default(), true), None);
    }

    #[test]
    fn the_clock_and_tokens_count_from_the_prompt() {
        let mut c = conversation(vec![ItemKind::Assistant { text: "A".into() }]);
        c.turn_start = Some(1_000.0);
        c.turn_tokens = 2_900;
        let a = line(&c, true, 1_050.4).unwrap();
        assert_eq!(a.stats.as_deref(), Some("50s · ↓ 2.9k tokens"));
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
    fn the_line_opens_the_newest_shell_still_running() {
        let mut c = conversation(vec![]);
        assert_eq!(newest_unfinished_shell(&c), None);
        let mut titled = shell("b2");
        titled.title = "Full tap-book check".into();
        c.shells = vec![shell("b1"), titled, shell("b3")];
        c.finished_agents.insert("b3".into(), "completed".into());
        assert_eq!(
            newest_unfinished_shell(&c),
            Some(("Full tap-book check".into(), "/tmp/b2.output".into()))
        );
        c.finished_agents.insert("b2".into(), "completed".into());
        assert_eq!(
            newest_unfinished_shell(&c),
            Some(("sleep 60".into(), "/tmp/b1.output".into())),
            "no title: the command"
        );
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
    fn a_prompt_with_nothing_back_after_a_while_is_stalled() {
        let mut c = conversation(vec![
            ItemKind::Assistant {
                text: "Done".into(),
            },
            ItemKind::TurnEnd {
                seconds: 5.0,
                model: None,
            },
            ItemKind::user("why page 6"),
        ]);
        c.turn_start = Some(1_000.0);
        assert_eq!(stalled(&c, true, 1_000.0 + STALL_S - 1.0), None, "not yet");
        assert_eq!(
            stalled(&c, true, 1_000.0 + STALL_S).as_deref(),
            Some("why page 6")
        );
        assert_eq!(
            stalled(&c, false, 9_999.0),
            None,
            "an idle session isn't stalled"
        );
        c.turn_tokens = 12;
        assert_eq!(stalled(&c, true, 9_999.0), None);
        c.turn_tokens = 0;
        c.items.push(item(ItemKind::Thinking {
            seconds: 1.0,
            text: String::new(),
        }));
        assert_eq!(stalled(&c, true, 9_999.0), None);
        let mut bare = conversation(vec![ItemKind::user("hi")]);
        assert_eq!(stalled(&bare, true, 9_999.0), None);
        bare.turn_start = Some(0.0);
        assert_eq!(stalled(&bare, true, 9_999.0).as_deref(), Some("hi"));
        let mut image = conversation(vec![ItemKind::User {
            text: "  ".into(),
            images: 1,
        }]);
        image.turn_start = Some(0.0);
        assert_eq!(stalled(&image, true, 9_999.0), None);
    }

    #[test]
    fn a_stop_is_on_its_way_for_twenty_seconds_while_the_session_works() {
        assert!(!is_stopping(None, true, 5));
        assert!(is_stopping(Some(0), true, STOPPING_MS - 1));
        assert!(!is_stopping(Some(0), true, STOPPING_MS));
        assert!(!is_stopping(Some(0), false, 1), "it stopped");
    }
}
