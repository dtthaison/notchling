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
        about: "Chat over the fixture with made-up git data in the Details Panel (NOTCHLING_RENDER_GIT_DIFF=1 opens a file's diff in the file panel, NOTCHLING_RENDER_CHANGES=1|expanded|menu opens the Changes panel)",
    },
];

pub fn held_preview(how: &str) -> Option<(String, Option<String>)> {
    let text = "Please run the whole suite again\n\nand then push".to_owned();
    let note = match how {
        "plain" => None,
        "box" => Some("Waiting — the terminal box has other text"),
        "mixed" => Some(
            "Something else was typed in the session's input box while the message went in, so it wasn't sent. Check the terminal.",
        ),
        _ => return None,
    };
    Some((text, note.map(String::from)))
}
