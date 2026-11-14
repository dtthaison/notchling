pub const EXITED: &str = "Exited";
pub const CODEX_GROUP: &str = "Codex";

pub fn bubble_part(queued: bool, sending: bool) -> &'static str {
    match (queued, sending) {
        (_, true) => "Sending Bubble",
        (true, false) => "Queued Bubble",
        (false, false) => "Your Message",
    }
}
