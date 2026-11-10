//! When the Tasks bar shows (pure). It shows while a task is unfinished; once all are done it
//! stays [`DONE_FOR_MS`] and goes, and × hides it until the list changes. A list that was already
//! all done when Chat first saw it (an old session opened) doesn't show at all, unless the session
//! is working.

/// How long a finished list stays up.
pub const DONE_FOR_MS: i64 = 30_000;

/// What Chat saw of one session's list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    /// The list's ids and statuses: a change shows it again.
    pub signature: String,
    /// When it was first seen all done; `None` while a task is unfinished.
    pub done_at: Option<i64>,
    /// × was pressed on this list.
    pub dismissed: bool,
    /// Already all done when first seen, on an idle session: never shown.
    pub stale: bool,
}

/// What Chat knows of `signature` now, after `prev`.
pub fn observe(
    prev: Option<&Seen>,
    signature: &str,
    all_done: bool,
    working: bool,
    now_ms: i64,
) -> Seen {
    match prev.filter(|p| p.signature == signature) {
        Some(p) => Seen {
            done_at: if all_done {
                p.done_at.or(Some(now_ms))
            } else {
                None
            },
            ..p.clone()
        },
        None => Seen {
            signature: signature.to_owned(),
            done_at: all_done.then_some(now_ms),
            dismissed: false,
            // A first sight of a finished list: an old one, unless the session is on it now.
            stale: all_done && !working && prev.is_none(),
        },
    }
}

/// Whether the bar shows.
pub fn visible(seen: &Seen, now_ms: i64) -> bool {
    if seen.dismissed || seen.stale {
        return false;
    }
    seen.done_at.is_none_or(|at| now_ms - at < DONE_FOR_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unfinished_list_shows_and_a_finished_one_goes_after_a_while() {
        let s = observe(None, "a:w,b:n", false, true, 0);
        assert!(visible(&s, 0));
        let s = observe(Some(&s), "a:d,b:d", true, true, 1_000);
        assert!(visible(&s, 1_000));
        let s = observe(Some(&s), "a:d,b:d", true, false, 20_000);
        assert_eq!(s.done_at, Some(1_000), "the first time it was done");
        assert!(visible(&s, 30_999));
        assert!(!visible(&s, 31_000));
    }

    #[test]
    fn an_old_finished_list_never_shows_unless_the_session_works_on_it() {
        assert!(!visible(&observe(None, "a:d", true, false, 0), 0));
        assert!(visible(&observe(None, "a:d", true, true, 0), 0));
    }

    #[test]
    fn dismissed_stays_hidden_until_the_list_changes() {
        let mut s = observe(None, "a:w", false, true, 0);
        s.dismissed = true;
        let same = observe(Some(&s), "a:w", false, true, 5);
        assert!(!visible(&same, 5));
        let changed = observe(Some(&same), "a:d,b:n", false, true, 6);
        assert!(visible(&changed, 6), "a new task shows it again");
        // A finished list seen change (not a first sight) is not stale.
        let done = observe(Some(&changed), "a:d,b:d", true, false, 7);
        assert!(visible(&done, 7));
    }
}
