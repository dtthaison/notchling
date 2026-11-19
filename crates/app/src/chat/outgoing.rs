use notchling_engine::terminal::input_box::Pictured;
use notchling_engine::transcript::model::{Conversation, ItemKind};
use notchling_engine::transcript::stored::squeezed_chars;

pub const GRACE_MS: i64 = 30_000;
const CLOCK_SLACK_MS: f64 = 2_000.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    pub text: String,
    pub seen: usize,
    pub delivered_at: Option<i64>,
    pub sent_at: i64,
    pub boxed: Option<String>,
}

impl Outgoing {
    pub fn new(text: &str, conversation: &Conversation, now_ms: i64) -> Outgoing {
        Outgoing {
            text: text.to_owned(),
            seen: copies(text, conversation),
            delivered_at: None,
            sent_at: now_ms,
            boxed: None,
        }
    }

    pub fn delivered(&mut self, now_ms: i64) {
        self.delivered_at.get_or_insert(now_ms);
    }

    pub fn keep(&self, conversation: &Conversation, now_ms: i64) -> bool {
        !self.arrived(conversation)
            && self
                .delivered_at
                .is_none_or(|at| now_ms.saturating_sub(at) < GRACE_MS)
    }

    pub fn arrived(&self, conversation: &Conversation) -> bool {
        copies(&self.text, conversation) > self.seen || self.dated_copy(conversation)
    }

    fn dated_copy(&self, conversation: &Conversation) -> bool {
        dated_since(&self.text, sent_since(self.sent_at), conversation)
    }
}

pub fn sent_since(queued_ms: i64) -> f64 {
    queued_ms as f64 - CLOCK_SLACK_MS
}

pub struct Wanted {
    exact: String,
    pictured: Option<Pictured>,
}

impl Wanted {
    pub fn of(text: &str) -> Wanted {
        Wanted {
            exact: comparable(text).collect(),
            pictured: Pictured::of(text),
        }
    }

    pub fn is(&self, turn: &str, images: Option<usize>) -> bool {
        comparable(turn).eq(self.exact.chars())
            || self
                .pictured
                .as_ref()
                .is_some_and(|pictured| pictured.is_shown_as(turn, images))
    }
}

pub struct Turns<'a> {
    conversation: &'a Conversation,
    taken: Vec<usize>,
}

impl<'a> Turns<'a> {
    pub fn new(conversation: &'a Conversation) -> Turns<'a> {
        Turns {
            conversation,
            taken: Vec::new(),
        }
    }

    pub fn take(&mut self, wanted: &Wanted, since_ms: f64) -> bool {
        let found = self
            .conversation
            .items
            .iter()
            .enumerate()
            .position(|(at, item)| {
                item.date.is_some_and(|d| d * 1000.0 >= since_ms)
                    && !self.taken.contains(&at)
                    && matches!(&item.kind, ItemKind::User { text, images, .. } if wanted.is(text, Some(*images)))
            });
        if let Some(at) = found {
            self.taken.push(at);
        }
        found.is_some()
    }
}

pub fn dated_since(text: &str, since_ms: f64, conversation: &Conversation) -> bool {
    Turns::new(conversation).take(&Wanted::of(text), since_ms)
}

fn copies(text: &str, conversation: &Conversation) -> usize {
    let wanted = Wanted::of(text);
    let sent = conversation
        .items
        .iter()
        .filter(|item| {
            matches!(&item.kind, ItemKind::User { text, images, .. } if wanted.is(text, Some(*images)))
        })
        .count();
    let queued = conversation
        .queued
        .iter()
        .filter(|t| wanted.is(t, None))
        .count();
    sent + queued
}

fn comparable(text: &str) -> impl Iterator<Item = char> + '_ {
    let mut first = true;
    let mut after_bang = false;
    squeezed_chars(text).filter(move |c| {
        let gap = after_bang && *c == ' ';
        after_bang = first && *c == '!';
        first = false;
        !gap
    })
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
                        merged: false,
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
        let out = Outgoing::new("Fix the build", &before, 0);
        assert!(!out.arrived(&before));
        assert!(out.arrived(&with_user(&["first", "Fix the build\n"])));
    }

    #[test]
    fn the_same_words_twice_wait_for_the_second_copy() {
        let before = with_user(&["yes"]);
        let out = Outgoing::new("yes", &before, 0);
        assert!(!out.arrived(&before), "the earlier yes is not this one");
        assert!(out.arrived(&with_user(&["yes", "yes"])));
    }

    #[test]
    fn a_delivered_message_the_transcript_never_shows_lets_go_after_a_while() {
        let c = with_user(&[]);
        let mut out = Outgoing::new("words the transcript keeps differently", &c, 0);
        assert!(
            out.keep(&c, 1_000_000),
            "still being typed: no deadline yet"
        );
        out.delivered(1_000);
        assert!(out.keep(&c, 1_000 + GRACE_MS - 1));
        assert!(!out.keep(&c, 1_000 + GRACE_MS));
    }

    #[test]
    fn a_shell_command_arrives_as_the_transcript_writes_it() {
        let out = Outgoing::new("!ls -la", &with_user(&[]), 0);
        assert!(out.arrived(&with_user(&["! ls -la"])));
    }

    #[test]
    fn a_clear_arrives_in_the_new_transcript_that_has_as_many_copies() {
        let mut before = with_user(&["/clear", "fix it"]);
        before.items[0].date = Some(1_000.0);
        let out = Outgoing::new("/clear", &before, 2_000_000);
        assert!(!out.arrived(&before));
        let mut after = with_user(&["/clear"]);
        after.items[0].date = Some(2_000.5);
        assert!(out.arrived(&after), "the new file's /clear is this one");
    }

    #[test]
    fn a_copy_dated_before_the_send_is_not_this_one() {
        let mut before = with_user(&["yes"]);
        before.items[0].date = Some(1_990.0);
        let out = Outgoing::new("yes", &before, 2_000_000);
        assert!(!out.arrived(&before));
    }

    #[test]
    fn a_queued_copy_counts_as_arrived() {
        let before = with_user(&[]);
        let out = Outgoing::new("and bump the version", &before, 0);
        let mut after = with_user(&[]);
        after.queued.push("and bump the version".into());
        assert!(out.arrived(&after));
    }
}
