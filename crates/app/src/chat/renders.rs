use crate::chat::busy::Busy;
use crate::chat::outbox::{bubble, Bubble, OutboxFeed, OutboxRow};
use crate::views::ViewSpec;

pub const VIEWS: [ViewSpec; 2] = [
    ViewSpec {
    name: "chat-list-probe",
    size: (900.0, 700.0),
    about: "GPUI's list over 300 made-up rows: follow, anchor, cost, resize, spinners (set NOTCHLING_RENDER_BENCH=<seconds> to run its checks and print list-probe)",
    },
    ViewSpec {
        name: "chat-git",
        size: (1240.0, 780.0),
        about: "Chat over the fixture with made-up git data in the Details Panel (NOTCHLING_RENDER_GIT_DIFF=1 opens a file's diff in the file panel, NOTCHLING_RENDER_CHANGES=1|expanded|menu opens the Changes panel, NOTCHLING_RENDER_BRANCHES=working|not-merged|merged|open-row|loading|error|empty opens the Branches panel)",
    },
];

pub const OUTBOX_STATES: [&str; 25] = [
    "queued",
    "retry",
    "dialog",
    "answer",
    "busy_tasks",
    "busy_commands",
    "busy_replying",
    "busy_unconfirmed",
    "turn",
    "sending",
    "in_queue",
    "sent",
    "maybe",
    "box",
    "stopped",
    "gone",
    "three",
    "wait_queue",
    "wait_unsure",
    "editing",
    "holding",
    "aside",
    "aside_sending",
    "joined",
    "two_behind_unsure",
];

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}

fn preview_wire(id: &str, state: &str, why: Option<&str>, text: &str, n: i64) -> serde_json::Value {
    let mut wire = serde_json::json!({
        "id": id, "session": "preview", "tool": "claude",
        "kind": "prompt", "text": text, "state": state, "position": n,
        "queuedMs": n, "updatedMs": n, "via": "chat",
    });
    if let Some(why) = why {
        wire["why"] = serde_json::json!(why);
    }
    wire
}

fn preview_feed(wires: Vec<serde_json::Value>) -> Vec<Bubble> {
    let mut feed = OutboxFeed::default();
    feed.seed(&wires);
    feed.bubbles("preview")
}

fn preview_row(state: &str, why: Option<&str>, text: &str) -> Option<Bubble> {
    let mut wire = serde_json::json!({
        "id": format!("preview-{state}"), "session": "preview", "tool": "claude",
        "kind": "prompt", "text": text, "state": state, "position": 1,
        "queuedMs": 1, "updatedMs": 1, "via": "chat",
    });
    if let Some(why) = why {
        wire["why"] = serde_json::json!(why);
    }
    if state == "box" {
        wire["box"] = serde_json::json!("a half-written line\nthat someone typed in the terminal");
    }
    OutboxRow::from_wire(&wire).map(|row| bubble(&row))
}

fn busy_row(why: Busy, text: &str) -> Vec<Bubble> {
    vec![Bubble {
        id: "preview-busy".into(),
        text: text.into(),
        status: crate::chat::outbox::Status::Busy(why),
        buttons: vec![
            crate::chat::outbox::Button::Edit,
            crate::chat::outbox::Button::Remove,
        ],
        joined: 1,
    }]
}

pub fn outbox_preview(how: &str) -> Option<Vec<Bubble>> {
    let text = "Please run the whole suite again\n\nand then push";
    let one = |state: &str, why: Option<&str>| preview_row(state, why, text).into_iter().collect();
    Some(match how {
        "queued" => one("queued", None),
        "retry" => one("waiting", Some("retry")),
        "dialog" => one("waiting", Some("dialog")),
        "answer" => one("waiting", Some("answer")),
        "busy_tasks" => busy_row(
            Busy::Tasks {
                count: 9,
                commands: false,
            },
            text,
        ),
        "busy_commands" => busy_row(
            Busy::Tasks {
                count: 3,
                commands: true,
            },
            text,
        ),
        "busy_replying" => busy_row(Busy::Replying, text),
        "busy_unconfirmed" => {
            let mut shown: Vec<Bubble> = preview_row("unsure", Some("restart"), text)
                .into_iter()
                .collect();
            shown.extend(busy_row(Busy::Unconfirmed, "and then push"));
            shown
        }
        "turn" => one("waiting", Some("turn")),
        "sending" => one("sending", None),
        "in_queue" => one("in_queue", None),
        "sent" => vec![Bubble {
            id: "preview-sent".into(),
            text: text.into(),
            status: crate::chat::outbox::Status::Sent,
            buttons: Vec::new(),
            joined: 1,
        }],
        "maybe" => one("unsure", Some("restart")),
        "box" => one("box", None),
        "stopped" => one("stopped", None),
        "gone" => one("gone", None),
        "three" => [
            preview_row("waiting", Some("turn"), "First, check the build"),
            preview_row("queued", None, "Then fix whatever fails"),
            preview_row("box", None, text),
        ]
        .into_iter()
        .flatten()
        .collect(),
        "wait_queue" => preview_feed(vec![preview_wire(
            "preview-wait-queue",
            "waiting",
            Some("queue"),
            text,
            1,
        )]),
        "wait_unsure" | "two_behind_unsure" => {
            let mut head = preview_wire(
                "preview-head",
                "unsure",
                Some("not_found"),
                "Please run the whole suite again",
                1,
            );
            head["asideAtMs"] = serde_json::json!(now_ms() + 102_000);
            let mut wires = vec![head];
            wires.push(preview_wire(
                "preview-next",
                "queued",
                None,
                "and then push",
                2,
            ));
            if how == "two_behind_unsure" {
                wires.push(preview_wire(
                    "preview-last",
                    "queued",
                    None,
                    "Then tell me what changed",
                    3,
                ));
            }
            preview_feed(wires)
        }
        "editing" => {
            let mut wire = preview_wire("preview-editing", "queued", None, text, 1);
            wire["heldUntilMs"] = serde_json::json!(now_ms() + 600_000);
            preview_feed(vec![
                wire,
                preview_wire("preview-after", "queued", None, "and then push", 2),
            ])
        }
        "holding" => {
            let mut wire = preview_wire("preview-holding", "unsure", Some("not_found"), text, 1);
            wire["asideAtMs"] = serde_json::json!(now_ms() + 102_000);
            preview_feed(vec![wire])
        }
        "aside" => {
            let mut wire = preview_wire("preview-aside", "unsure", Some("not_found"), text, 1);
            wire["asideMs"] = serde_json::json!(now_ms() - 5_000);
            preview_feed(vec![wire])
        }
        "aside_sending" => {
            let mut wire = preview_wire("preview-aside", "unsure", Some("not_found"), text, 1);
            wire["asideMs"] = serde_json::json!(now_ms() - 5_000);
            preview_feed(vec![
                wire,
                preview_wire(
                    "preview-after",
                    "sending",
                    None,
                    "and tell me what changed",
                    2,
                ),
            ])
        }
        "joined" => {
            let words =
                "Please run the whole suite again\n\nand then push\n\nand tell me what changed";
            let mut wire = preview_wire("preview-joined", "sending", None, words, 1);
            wire["joined"] = serde_json::json!(3);
            preview_feed(vec![wire])
        }
        _ => return None,
    })
}
