pub const NO_MESSAGE: &str = "No such message.";
pub const BEING_SENT: &str = "It's being sent.";
pub const CHANGED: &str = "It changed while you edited it.";
pub const IN_QUEUE: &str = "It's in the session's queue.";

pub fn is_settled(message: &str) -> bool {
    matches!(message, NO_MESSAGE | BEING_SENT | IN_QUEUE)
}
