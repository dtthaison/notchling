use super::agentapi::{
    message_was_sent, new_conversation_args, new_conversation_id, reported_error,
    send_message_args, AgentApi, ApiError,
};
use super::ids::is_uuid;
use super::steps::Steps;
use std::time::Duration;

pub const WORST_CASE: Duration = Duration::from_secs(80);
pub const RUN_BUDGET: Duration = Duration::from_secs(100);
pub const PICKUP_BUDGET: Duration = Duration::from_secs(100);
pub const START_CALL_WITHIN: Duration = Duration::from_secs(220);

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
    Unknown,
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

fn surely_not_sent(error: &ApiError) -> bool {
    matches!(error, ApiError::Missing | ApiError::NotStarted(_))
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
    match api.run(&args) {
        Err(e) if surely_not_sent(&e) => return Err(Fail::Api(e.to_string())),
        Ok(output) if message_was_sent(&output) => {}
        Ok(output) => {
            if let Some(why) = reported_error(&output) {
                return match steps.count(conversation).filter(|n| *n > before) {
                    Some(after) => Ok(Sent::Confirmed { before, after }),
                    None => Err(Fail::Api(why)),
                };
            }
        }
        Err(_) => {}
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
    let output = match api.run(&args) {
        Err(e) if surely_not_sent(&e) => return Err(Fail::Api(e.to_string())),
        Err(_) => return Ok(Started::Unknown),
        Ok(output) => output,
    };
    let id = match new_conversation_id(&output) {
        Ok(id) => id,
        Err(_) => {
            return match reported_error(&output) {
                Some(reported) => Err(Fail::Api(reported)),
                None => Ok(Started::Unknown),
            }
        }
    };
    for _ in 0..timing.polls {
        if steps.count(&id).is_some_and(|n| n > 0) {
            return Ok(Started::Confirmed(id));
        }
        pause.pause(timing.poll);
    }
    Ok(Started::Unconfirmed(id))
}
