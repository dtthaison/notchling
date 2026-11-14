pub const EXITED: &str = "Exited";
pub const CODEX_GROUP: &str = "Codex";

pub const GONE: &str = "isn't showing that permission";
pub const DIALOG: &str = "The session is asking you something";
pub const ANSWER_IN_TERMINAL: &str = "Answer it in the terminal";
pub const APPROVE: &str = "approve ";
pub const DIDNT_ANSWER: &str = "didn't answer in time";
pub const CLOSED_CONNECTION: &str = "closed the connection";

pub const FILE_TOOLS: [&str; 5] = ["Write", "Edit", "MultiEdit", "NotebookEdit", "Read"];
pub const NO_ALWAYS: [&str; 4] = ["Write", "Edit", "MultiEdit", "NotebookEdit"];

pub const PAST_STEPS: [&str; 8] = [
    "Ran ",
    "Read ",
    "Edited ",
    "Wrote ",
    "Searched the web for ",
    "Searched for ",
    "Found files matching ",
    "Used ",
];

pub fn model_command(id: &str) -> String {
    format!("/model {id}")
}

pub fn effort_command(level: &str) -> String {
    format!("/effort {level}")
}

pub fn bubble_part(queued: bool, sending: bool) -> &'static str {
    match (queued, sending) {
        (_, true) => "Sending Bubble",
        (true, false) => "Queued Bubble",
        (false, false) => "Your Message",
    }
}

pub const FACT_FOLDER: &str = "Folder";
pub const FACT_BRANCH: &str = "Branch";
pub const FACT_TURNS: &str = "Turns";
pub const FACT_TOKENS: &str = "Tokens";
pub const FACT_COMMANDS: &str = "Commands";
