use super::agentapi::{
    message_was_sent, new_conversation_args, new_conversation_id, send_message_args, AgentApi,
};
use super::ids::is_uuid;
use super::steps::Steps;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub polls: u32,
    pub poll: Duration,
}

impl Timing {
    pub const REAL: Timing = Timing {
        polls: 40,
        poll: Duration::from_millis(500),
    };
}

pub trait Pause: Send + Sync {
    fn pause(&self, how_long: Duration);
}

pub struct Sleep;

impl Pause for Sleep {
    fn pause(&self, how_long: Duration) {
        std::thread::sleep(how_long);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sent {
    Confirmed { before: u64, after: u64 },
    Unconfirmed { before: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Started {
    Confirmed(String),
    Unconfirmed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fail {
    BadId,
    NoConversation,
    Refused(String),
    Api(String),
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fail::BadId => write!(f, "That is not an Antigravity conversation id."),
            Fail::NoConversation => write!(
                f,
                "Antigravity has no conversation with that id, so nothing was sent."
            ),
            Fail::Refused(why) | Fail::Api(why) => write!(f, "{why}"),
        }
    }
}

pub fn send(
    api: &dyn AgentApi,
    steps: &dyn Steps,
    pause: &dyn Pause,
    conversation: &str,
    text: &str,
    timing: Timing,
) -> Result<Sent, Fail> {
    if !is_uuid(conversation) {
        return Err(Fail::BadId);
    }
    let Some(before) = steps.count(conversation) else {
        return Err(Fail::NoConversation);
    };
    let args = send_message_args(conversation, text).map_err(Fail::Refused)?;
    let output = api.run(&args).map_err(|e| Fail::Api(e.to_string()))?;
    if !message_was_sent(&output) {
        return Err(Fail::Api(
            "agentapi did not say the message was sent".into(),
        ));
    }
    for _ in 0..timing.polls {
        if let Some(after) = steps.count(conversation).filter(|n| *n > before) {
            return Ok(Sent::Confirmed { before, after });
        }
        pause.pause(timing.poll);
    }
    Ok(Sent::Unconfirmed { before })
}

pub fn start(
    api: &dyn AgentApi,
    steps: &dyn Steps,
    pause: &dyn Pause,
    text: &str,
    model: Option<&str>,
    title: Option<&str>,
    timing: Timing,
) -> Result<Started, Fail> {
    let args = new_conversation_args(text, model, title).map_err(Fail::Refused)?;
    let output = api.run(&args).map_err(|e| Fail::Api(e.to_string()))?;
    let id = new_conversation_id(&output).map_err(Fail::Api)?;
    for _ in 0..timing.polls {
        if steps.count(&id).is_some_and(|n| n > 0) {
            return Ok(Started::Confirmed(id));
        }
        pause.pause(timing.poll);
    }
    Ok(Started::Unconfirmed(id))
}
