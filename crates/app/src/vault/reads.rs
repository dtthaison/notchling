use std::collections::HashMap;

use super::reveal::{Part, Seen};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Intent {
    Eye,
    Copy,
    Cache,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticket {
    id: u64,
    rev: String,
    seen: Seen,
}

type ReadKey = (String, Part, Intent);

#[derive(Default)]
pub struct Reads {
    next: u64,
    open: HashMap<ReadKey, Ticket>,
}

impl Reads {
    pub fn begin(
        &mut self,
        name: &str,
        part: Part,
        intent: Intent,
        rev: &str,
        unlocked: Option<Seen>,
    ) -> Option<Ticket> {
        let seen = unlocked?;
        let key = (name.to_owned(), part, intent);
        if self
            .open
            .get(&key)
            .is_some_and(|open| open.rev == rev && open.seen == seen)
        {
            return None;
        }
        self.next += 1;
        let ticket = Ticket {
            id: self.next,
            rev: rev.to_owned(),
            seen,
        };
        self.open.insert(key, ticket.clone());
        Some(ticket)
    }

    pub fn land(
        &mut self,
        name: &str,
        part: Part,
        intent: Intent,
        ticket: &Ticket,
        current: Option<&str>,
        unlocked: Option<Seen>,
    ) -> bool {
        let key = (name.to_owned(), part, intent);
        if self.open.get(&key) != Some(ticket) {
            return false;
        }
        self.open.remove(&key);
        Some(ticket.seen) == unlocked && current == Some(ticket.rev.as_str())
    }

    pub fn keep_current(&mut self, current: &[(&str, String)]) -> bool {
        let before = self.open.len();
        self.open.retain(|(name, _, _), ticket| {
            current
                .iter()
                .any(|(n, rev)| *n == name.as_str() && *rev == ticket.rev)
        });
        self.open.len() != before
    }

    pub fn is_open(&self, name: &str, part: Part, intent: Intent) -> bool {
        self.open.contains_key(&(name.to_owned(), part, intent))
    }

    pub fn cancel(&mut self, name: &str, part: Part, intent: Intent) {
        self.open.remove(&(name.to_owned(), part, intent));
    }

    pub fn cancel_all(&mut self) {
        self.open.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::super::reveal::Epoch;
    use super::*;

    fn calm() -> Option<Seen> {
        Some(Seen::of(&Epoch::new()))
    }

    fn current(list: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
        list.iter().map(|(n, r)| (*n, (*r).to_owned())).collect()
    }

    #[test]
    fn a_read_that_lands_after_a_hide_is_dropped() {
        let mut reads = Reads::default();
        let ticket = reads
            .begin("a", Part::Secret, Intent::Eye, "r1", calm())
            .unwrap();
        reads.cancel("a", Part::Secret, Intent::Eye);
        assert!(!reads.land("a", Part::Secret, Intent::Eye, &ticket, Some("r1"), calm()));
        assert!(!reads.is_open("a", Part::Secret, Intent::Eye));
    }

    #[test]
    fn a_read_that_lands_after_a_lock_a_tab_change_or_a_form_is_dropped() {
        let mut reads = Reads::default();
        let eye = reads
            .begin("a", Part::Secret, Intent::Eye, "r1", calm())
            .unwrap();
        let copy = reads
            .begin("a", Part::Secret, Intent::Copy, "r1", calm())
            .unwrap();
        let user = reads
            .begin("a", Part::Username, Intent::Cache, "r1", calm())
            .unwrap();
        reads.cancel_all();
        assert!(!reads.land("a", Part::Secret, Intent::Eye, &eye, Some("r1"), calm()));
        assert!(!reads.land("a", Part::Secret, Intent::Copy, &copy, Some("r1"), calm()));
        assert!(!reads.land(
            "a",
            Part::Username,
            Intent::Cache,
            &user,
            Some("r1"),
            calm()
        ));
    }

    #[test]
    fn an_old_read_cannot_land_as_a_newer_one_for_the_same_field() {
        let mut reads = Reads::default();
        let first = reads
            .begin("a", Part::Secret, Intent::Eye, "r1", calm())
            .unwrap();
        reads.cancel_all();
        let second = reads
            .begin("a", Part::Secret, Intent::Eye, "r1", calm())
            .unwrap();
        assert_ne!(first, second);
        assert!(!reads.land("a", Part::Secret, Intent::Eye, &first, Some("r1"), calm()));
        assert!(reads.is_open("a", Part::Secret, Intent::Eye));
        assert!(reads.land("a", Part::Secret, Intent::Eye, &second, Some("r1"), calm()));
    }

    #[test]
    fn a_second_click_while_a_read_is_open_starts_no_second_read() {
        let mut reads = Reads::default();
        assert!(reads
            .begin("a", Part::Secret, Intent::Eye, "r1", calm())
            .is_some());
        assert!(reads
            .begin("a", Part::Secret, Intent::Eye, "r1", calm())
            .is_none());
        assert!(reads
            .begin("a", Part::Secret, Intent::Copy, "r1", calm())
            .is_some());
        assert!(reads
            .begin("b", Part::Secret, Intent::Eye, "r1", calm())
            .is_some());
    }

    #[test]
    fn a_read_lands_once_and_only_for_the_revision_it_started_on() {
        let mut reads = Reads::default();
        let ticket = reads
            .begin("a", Part::Secret, Intent::Eye, "r1", calm())
            .unwrap();
        assert!(reads.land("a", Part::Secret, Intent::Eye, &ticket, Some("r1"), calm()));
        assert!(!reads.land("a", Part::Secret, Intent::Eye, &ticket, Some("r1"), calm()));
        let ticket = reads
            .begin("a", Part::Secret, Intent::Eye, "r1", calm())
            .unwrap();
        assert!(!reads.land("a", Part::Secret, Intent::Eye, &ticket, Some("r2"), calm()));
        assert!(!reads.is_open("a", Part::Secret, Intent::Eye));
        let ticket = reads
            .begin("a", Part::Secret, Intent::Eye, "r1", calm())
            .unwrap();
        assert!(!reads.land("a", Part::Secret, Intent::Eye, &ticket, None, calm()));
    }

    #[test]
    fn a_read_that_started_before_a_lock_cannot_land_even_before_any_view_sweeps() {
        let locked = Epoch::new();
        let mut reads = Reads::default();
        let eye = reads
            .begin(
                "a",
                Part::Secret,
                Intent::Eye,
                "r1",
                Some(Seen::of(&locked)),
            )
            .unwrap();
        let copy = reads
            .begin(
                "a",
                Part::Secret,
                Intent::Copy,
                "r1",
                Some(Seen::of(&locked)),
            )
            .unwrap();
        locked.bump();
        assert!(!reads.land(
            "a",
            Part::Secret,
            Intent::Eye,
            &eye,
            Some("r1"),
            Some(Seen::of(&locked))
        ));
        assert!(!reads.land(
            "a",
            Part::Secret,
            Intent::Copy,
            &copy,
            Some("r1"),
            Some(Seen::of(&locked))
        ));
        let again = reads
            .begin(
                "a",
                Part::Secret,
                Intent::Copy,
                "r1",
                Some(Seen::of(&locked)),
            )
            .unwrap();
        assert!(reads.land(
            "a",
            Part::Secret,
            Intent::Copy,
            &again,
            Some("r1"),
            Some(Seen::of(&locked))
        ));
    }

    #[test]
    fn a_read_for_an_old_revision_is_evicted_so_it_cannot_block_a_read_for_the_new_one() {
        let mut reads = Reads::default();
        reads.begin("a", Part::Secret, Intent::Copy, "r1", calm());
        reads.begin("a", Part::Secret, Intent::Eye, "r1", calm());
        reads.begin("a", Part::Username, Intent::Cache, "r1", calm());
        reads.begin("b", Part::Secret, Intent::Eye, "r1", calm());
        reads.begin("gone", Part::Secret, Intent::Eye, "r1", calm());
        assert!(reads.keep_current(&current(&[("a", "r2"), ("b", "r1")])));
        assert!(!reads.is_open("a", Part::Secret, Intent::Copy));
        assert!(!reads.is_open("a", Part::Secret, Intent::Eye));
        assert!(!reads.is_open("a", Part::Username, Intent::Cache));
        assert!(!reads.is_open("gone", Part::Secret, Intent::Eye));
        assert!(
            reads.is_open("b", Part::Secret, Intent::Eye),
            "a read for a revision that is still current stays"
        );
        assert!(reads
            .begin("a", Part::Secret, Intent::Copy, "r2", calm())
            .is_some());
        assert!(!reads.keep_current(&current(&[("a", "r2"), ("b", "r1")])));
    }

    #[test]
    fn a_read_cannot_begin_or_land_while_the_mac_is_locked() {
        let mut reads = Reads::default();
        assert!(reads
            .begin("a", Part::Secret, Intent::Copy, "r1", None)
            .is_none());
        assert!(!reads.is_open("a", Part::Secret, Intent::Copy));
        let copy = reads
            .begin("a", Part::Secret, Intent::Copy, "r1", calm())
            .unwrap();
        assert!(!reads.land("a", Part::Secret, Intent::Copy, &copy, Some("r1"), None));
        let eye = reads
            .begin("a", Part::Secret, Intent::Eye, "r1", calm())
            .unwrap();
        assert!(reads.land("a", Part::Secret, Intent::Eye, &eye, Some("r1"), calm()));
    }

    #[test]
    fn a_click_after_the_entry_changed_or_a_lock_replaces_the_read_still_open_from_before() {
        let locked = Epoch::new();
        let mut reads = Reads::default();
        let old = reads
            .begin(
                "a",
                Part::Secret,
                Intent::Copy,
                "r1",
                Some(Seen::of(&locked)),
            )
            .unwrap();
        let new = reads
            .begin(
                "a",
                Part::Secret,
                Intent::Copy,
                "r2",
                Some(Seen::of(&locked)),
            )
            .expect("a read for the new revision starts at once");
        assert!(!reads.land(
            "a",
            Part::Secret,
            Intent::Copy,
            &old,
            Some("r2"),
            Some(Seen::of(&locked))
        ));
        locked.bump();
        let after = reads
            .begin(
                "a",
                Part::Secret,
                Intent::Copy,
                "r2",
                Some(Seen::of(&locked)),
            )
            .expect("a read after a lock starts at once");
        assert!(!reads.land(
            "a",
            Part::Secret,
            Intent::Copy,
            &new,
            Some("r2"),
            Some(Seen::of(&locked))
        ));
        assert!(reads.land(
            "a",
            Part::Secret,
            Intent::Copy,
            &after,
            Some("r2"),
            Some(Seen::of(&locked))
        ));
    }
}
