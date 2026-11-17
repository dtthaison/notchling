use crate::chat::outbox::{bubble, Bubble, OutboxRow};
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

pub const OUTBOX_STATES: [&str; 12] = [
    "queued", "retry", "dialog", "turn", "sending", "in_queue", "sent", "maybe", "box", "stopped",
    "gone", "three",
];

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

pub fn outbox_preview(how: &str) -> Option<Vec<Bubble>> {
    let text = "Please run the whole suite again\n\nand then push";
    let one = |state: &str, why: Option<&str>| preview_row(state, why, text).into_iter().collect();
    Some(match how {
        "queued" => one("queued", None),
        "retry" => one("waiting", Some("retry")),
        "dialog" => one("waiting", Some("dialog")),
        "turn" => one("waiting", Some("turn")),
        "sending" => one("sending", None),
        "in_queue" => one("in_queue", None),
        "sent" => vec![Bubble {
            id: "preview-sent".into(),
            text: text.into(),
            status: crate::chat::outbox::Status::Sent,
            buttons: Vec::new(),
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
        _ => return None,
    })
}
