//! The Allow/Deny prompt for a phone that gave the right pairing code (Milestone 4, U6), pure.
//!
//! The daemon publishes `bridge.pair.request {request, name, platform, address}` and holds the
//! phone until `bridge.pair.answer {request, allow}` or [`WAIT_MS`] pass (then it refuses). The
//! words are the Swift app's `BridgeController.requestApproval` alert, with the address the phone
//! connected from added, so the person can tell the phone they just scanned from a stranger who
//! guessed the code. [`Queue`] holds the requests waiting and says which one is on show; the view
//! (`prompt_view`, macOS) draws [`Prompt`] and the feed (`pairing`) fills the queue.

use serde_json::Value;

/// The alert's title in the Swift app.
pub const TITLE: &str = "Let this phone connect?";
/// The two buttons, Allow first.
pub const ALLOW: &str = "Allow";
pub const DENY: &str = "Deny";
/// How long the daemon waits for an answer before refusing (`APPROVAL_WAIT`, 120 s). A request
/// older than this is already refused, so its prompt goes away.
pub const WAIT_MS: i64 = 120_000;

/// The prompt window's size in points (width, height).
pub const PROMPT_SIZE: (f32, f32) = (420.0, 244.0);

/// One phone waiting to be let in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairRequest {
    /// The id `bridge.pair.answer` takes.
    pub id: String,
    pub name: String,
    pub platform: String,
    /// Where the phone connected from (its IP; for the tunnel, the real client's); empty when the
    /// daemon didn't say.
    pub address: String,
}

impl PairRequest {
    /// Reads a `bridge.pair.request` event's data; `None` without an id or a name (nothing to
    /// answer, or nobody to name). A missing platform reads "unknown", as the bridge does.
    pub fn from_event(data: &Value) -> Option<PairRequest> {
        let text = |key: &str| {
            data.get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_owned()
        };
        let id = text("request");
        let name = text("name");
        if id.is_empty() || name.is_empty() {
            return None;
        }
        let platform = match text("platform") {
            p if p.is_empty() => "unknown".to_owned(),
            p => p,
        };
        Some(PairRequest {
            id,
            name,
            platform,
            address: text("address"),
        })
    }

    /// The Swift alert's text, word for word.
    pub fn message(&self) -> String {
        format!(
            "{} ({}) will see the Claude sessions on this Mac and can message the ones Notchling \
             runs. Allow it only if you just scanned the code yourself.",
            self.name, self.platform
        )
    }

    /// Where it connected from, or nothing when that isn't known.
    pub fn from_line(&self) -> Option<String> {
        (!self.address.is_empty()).then(|| format!("Connecting from {}", self.address))
    }
}

/// What the prompt draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    /// The request an Allow or a Deny answers.
    pub request: String,
    pub title: &'static str,
    pub message: String,
    pub from: Option<String>,
    /// "1 more waiting" while others queue behind this one.
    pub more: Option<String>,
    pub allow: &'static str,
    pub deny: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Waiting {
    request: PairRequest,
    /// When the daemon gives up on it (ms since the epoch).
    refused_at_ms: i64,
}

/// The requests waiting, oldest first; the oldest is the one on show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Queue {
    waiting: Vec<Waiting>,
}

impl Queue {
    /// Adds a request that arrived at `now_ms`; one with an id already waiting is the same.
    pub fn push(&mut self, request: PairRequest, now_ms: i64) {
        if self.waiting.iter().any(|w| w.request.id == request.id) {
            return;
        }
        self.waiting.push(Waiting {
            request,
            refused_at_ms: now_ms + WAIT_MS,
        });
    }

    /// Takes a request off, answered or not.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.waiting.len();
        self.waiting.retain(|w| w.request.id != id);
        self.waiting.len() != before
    }

    /// Drops what the daemon has refused by now. Whether anything went.
    pub fn expire(&mut self, now_ms: i64) -> bool {
        let before = self.waiting.len();
        self.waiting.retain(|w| w.refused_at_ms > now_ms);
        self.waiting.len() != before
    }

    /// The daemon refused them all (the bridge went off) or the connection to it is gone.
    pub fn clear(&mut self) -> bool {
        let any = !self.waiting.is_empty();
        self.waiting.clear();
        any
    }

    pub fn len(&self) -> usize {
        self.waiting.len()
    }

    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }

    /// The prompt for the oldest request that is still waiting at `now_ms`.
    pub fn prompt(&self, now_ms: i64) -> Option<Prompt> {
        let live: Vec<&Waiting> = self
            .waiting
            .iter()
            .filter(|w| w.refused_at_ms > now_ms)
            .collect();
        let first = live.first()?;
        let more = match live.len() - 1 {
            0 => None,
            n => Some(format!("{n} more waiting")),
        };
        Some(Prompt {
            request: first.request.id.clone(),
            title: TITLE,
            message: first.request.message(),
            from: first.request.from_line(),
            more,
            allow: ALLOW,
            deny: DENY,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(id: &str, name: &str) -> PairRequest {
        PairRequest {
            id: id.into(),
            name: name.into(),
            platform: "android".into(),
            address: "192.168.1.40".into(),
        }
    }

    #[test]
    fn the_daemons_event_is_read() {
        let data = json!({"request": "pair-1", "name": " Pixel 9 ", "platform": "android",
                          "address": "192.168.1.40"});
        assert_eq!(
            PairRequest::from_event(&data),
            Some(request("pair-1", "Pixel 9"))
        );
        // An older daemon sends no address, and a phone may send no platform.
        let bare = PairRequest::from_event(&json!({"request": "pair-2", "name": "Phone"}));
        let bare = bare.unwrap();
        assert_eq!(bare.platform, "unknown");
        assert_eq!(bare.address, "");
        assert_eq!(bare.from_line(), None);
    }

    #[test]
    fn an_event_that_cant_be_answered_or_named_is_not_a_prompt() {
        assert_eq!(PairRequest::from_event(&json!({"name": "Phone"})), None);
        assert_eq!(PairRequest::from_event(&json!({"request": "pair-1"})), None);
        assert_eq!(
            PairRequest::from_event(&json!({"request": "pair-1", "name": "  "})),
            None
        );
        assert_eq!(PairRequest::from_event(&json!("pair-1")), None);
    }

    #[test]
    fn the_words_are_the_swift_alerts_with_the_address_added() {
        let prompt = {
            let mut queue = Queue::default();
            queue.push(request("pair-1", "Pixel 9"), 0);
            queue.prompt(1).unwrap()
        };
        assert_eq!(prompt.title, "Let this phone connect?");
        assert_eq!(
            prompt.message,
            "Pixel 9 (android) will see the Claude sessions on this Mac and can message the \
             ones Notchling runs. Allow it only if you just scanned the code yourself."
        );
        assert_eq!(prompt.from.as_deref(), Some("Connecting from 192.168.1.40"));
        assert_eq!((prompt.allow, prompt.deny), ("Allow", "Deny"));
        assert_eq!(prompt.more, None);
        assert_eq!(prompt.request, "pair-1");
    }

    #[test]
    fn the_oldest_request_is_on_show_and_the_rest_are_counted() {
        let mut queue = Queue::default();
        assert_eq!(queue.prompt(0), None);
        queue.push(request("pair-1", "First"), 0);
        queue.push(request("pair-2", "Second"), 10);
        queue.push(request("pair-3", "Third"), 20);
        queue.push(request("pair-2", "Second again"), 30);
        assert_eq!(queue.len(), 3, "the same id is the same request");
        let prompt = queue.prompt(40).unwrap();
        assert_eq!(prompt.request, "pair-1");
        assert_eq!(prompt.more.as_deref(), Some("2 more waiting"));
        assert!(queue.remove("pair-1"));
        assert!(!queue.remove("pair-1"));
        let prompt = queue.prompt(40).unwrap();
        assert_eq!(prompt.request, "pair-2");
        assert!(prompt.message.starts_with("Second ("), "{}", prompt.message);
        assert_eq!(prompt.more.as_deref(), Some("1 more waiting"));
    }

    #[test]
    fn a_request_the_daemon_has_refused_goes_away() {
        let mut queue = Queue::default();
        queue.push(request("pair-1", "Old"), 0);
        queue.push(request("pair-2", "New"), 100_000);
        // Not yet.
        assert!(!queue.expire(WAIT_MS - 1));
        assert_eq!(queue.prompt(WAIT_MS - 1).unwrap().request, "pair-1");
        // At the daemon's deadline the first is refused; the prompt moves on before the sweep.
        assert_eq!(queue.prompt(WAIT_MS).unwrap().request, "pair-2");
        assert!(queue.expire(WAIT_MS));
        assert_eq!(queue.len(), 1);
        assert!(queue.expire(100_000 + WAIT_MS));
        assert!(queue.is_empty());
        assert_eq!(queue.prompt(100_000 + WAIT_MS), None);
    }

    #[test]
    fn clearing_says_whether_anything_was_waiting() {
        let mut queue = Queue::default();
        assert!(!queue.clear());
        queue.push(request("pair-1", "Phone"), 0);
        assert!(queue.clear());
        assert!(queue.is_empty());
    }
}
