use super::model::ChatMessage;
use super::router::MAX_MESSAGE_BYTES;
use super::wire;

const ENVELOPE_ROOM: usize = 4096;
const CUT_SLACK: usize = 8;

fn room() -> usize {
    MAX_MESSAGE_BYTES - ENVELOPE_ROOM
}

fn size(message: &ChatMessage) -> usize {
    wire::text(&wire::object(message)).len() + 1
}

fn shorten(message: &mut ChatMessage, room: usize) {
    loop {
        let total = size(message);
        let over = total.saturating_sub(room);
        if over == 0 {
            return;
        }
        if !message.text.is_empty() {
            let characters = message.text.chars().count();
            let keep = characters.saturating_sub(over * characters / total + CUT_SLACK);
            let end = message
                .text
                .char_indices()
                .nth(keep)
                .map_or(message.text.len(), |(at, _)| at);
            message.text.truncate(end);
            if keep > 0 {
                message.text.push('…');
            }
        } else if let Some(steps) = message.steps.as_mut().filter(|s| !s.is_empty()) {
            steps.pop();
        } else if message.output.take().is_none() && message.images.pop().is_none() {
            return;
        }
    }
}

pub fn newest(mut messages: Vec<ChatMessage>) -> (Vec<ChatMessage>, bool) {
    let room = room();
    let total = messages.len();
    let mut used = 0;
    let mut kept = 0;
    for message in messages.iter().rev() {
        let next = size(message);
        if used + next > room {
            break;
        }
        used += next;
        kept += 1;
    }
    if kept == 0 {
        let Some(mut last) = messages.pop() else {
            return (messages, false);
        };
        shorten(&mut last, room);
        return (vec![last], total > 1);
    }
    messages.drain(..total - kept);
    (messages, kept < total)
}

pub fn chunks(messages: Vec<ChatMessage>) -> Vec<Vec<ChatMessage>> {
    let room = room();
    let mut out = Vec::new();
    let mut current: Vec<ChatMessage> = Vec::new();
    let mut used = 0;
    for mut message in messages {
        let mut next = size(&message);
        if next > room {
            shorten(&mut message, room);
            next = size(&message);
        }
        if used + next > room && !current.is_empty() {
            out.push(std::mem::take(&mut current));
            used = 0;
        }
        used += next;
        current.push(message);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}
