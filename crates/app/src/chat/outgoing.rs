//! Messages on their way: what you sent shows at once as your bubble, marked "Sending", until the
//! transcript holds it. Typing it into a background session takes seconds (`claude attach`, then
//! the transcript is written and read again), and an empty box with nothing in the conversation
//! looked like a lost message.

use notchling_engine::transcript::model::{Conversation, ItemKind};

/// How long a delivered message waits for the transcript before its bubble goes, in ms: a message
/// the transcript keeps in other words (a slash command, pasted images) would otherwise stay.
pub const GRACE_MS: i64 = 30_000;

/// One message sent from this Chat and not yet in its session's transcript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    pub text: String,
    /// How many of your messages (sent or queued) already read `text` when it was sent, so
    /// sending the same words twice still waits for the second copy.
    pub seen: usize,
    /// When the daemon said it was typed in (ms), so the wait for the transcript can end.
    pub delivered_at: Option<i64>,
}

impl Outgoing {
    pub fn new(text: &str, conversation: &Conversation) -> Outgoing {
        Outgoing {
            text: text.to_owned(),
            seen: copies(text, conversation),
            delivered_at: None,
        }
    }

    /// The daemon typed it in at `now_ms`.
    pub fn delivered(&mut self, now_ms: i64) {
        self.delivered_at.get_or_insert(now_ms);
    }

    /// Whether the bubble stays: not in the transcript yet, and still being typed in or delivered
    /// less than [`GRACE_MS`] ago.
    pub fn keep(&self, conversation: &Conversation, now_ms: i64) -> bool {
        !self.arrived(conversation)
            && self
                .delivered_at
                .is_none_or(|at| now_ms.saturating_sub(at) < GRACE_MS)
    }

    /// Whether the transcript now holds this message, so the bubble gives way to the real one.
    pub fn arrived(&self, conversation: &Conversation) -> bool {
        copies(&self.text, conversation) > self.seen
    }
}

/// Your messages in `conversation` that read `text`, sent or queued, ignoring the space around.
fn copies(text: &str, conversation: &Conversation) -> usize {
    let text = text.trim();
    let sent = conversation
        .items
        .iter()
        .filter(|item| matches!(&item.kind, ItemKind::User { text: t, .. } if t.trim() == text))
        .count();
    let queued = conversation
        .queued
        .iter()
        .filter(|t| t.trim() == text)
        .count();
    sent + queued
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::transcript::model::ChatItem;

    fn with_user(texts: &[&str]) -> Conversation {
        Conversation {
            items: texts
                .iter()
                .enumerate()
                .map(|(i, t)| ChatItem {
                    id: format!("i{i}"),
                    kind: ItemKind::User {
                        text: (*t).to_owned(),
                        images: 0,
                    },
                    date: None,
                })
                .collect(),
            ..Conversation::default()
        }
    }

    #[test]
    fn a_sent_message_waits_until_the_transcript_has_it() {
        let before = with_user(&["first"]);
        let out = Outgoing::new("Fix the build", &before);
        assert!(!out.arrived(&before));
        assert!(out.arrived(&with_user(&["first", "Fix the build\n"])));
    }

    #[test]
    fn the_same_words_twice_wait_for_the_second_copy() {
        let before = with_user(&["yes"]);
        let out = Outgoing::new("yes", &before);
        assert!(!out.arrived(&before), "the earlier yes is not this one");
        assert!(out.arrived(&with_user(&["yes", "yes"])));
    }

    #[test]
    fn a_delivered_message_the_transcript_never_shows_lets_go_after_a_while() {
        let c = with_user(&[]);
        let mut out = Outgoing::new("words the transcript keeps differently", &c);
        assert!(
            out.keep(&c, 1_000_000),
            "still being typed: no deadline yet"
        );
        out.delivered(1_000);
        assert!(out.keep(&c, 1_000 + GRACE_MS - 1));
        assert!(!out.keep(&c, 1_000 + GRACE_MS));
    }

    #[test]
    fn a_queued_copy_counts_as_arrived() {
        let before = with_user(&[]);
        let out = Outgoing::new("and bump the version", &before);
        let mut after = with_user(&[]);
        after.queued.push("and bump the version".into());
        assert!(out.arrived(&after));
    }
}
