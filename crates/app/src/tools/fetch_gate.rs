use std::collections::HashMap;

#[derive(Debug, PartialEq, Eq)]
pub enum Ask {
    Start(u64),
    Wait,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Done {
    pub apply: bool,
    pub next: Option<u64>,
}

#[derive(Default)]
struct Row {
    requested: u64,
    shown: u64,
    in_flight: bool,
}

#[derive(Default)]
pub struct FetchGate {
    counter: u64,
    rows: HashMap<String, Row>,
}

impl FetchGate {
    pub fn request(&mut self, id: &str) -> Ask {
        self.counter += 1;
        let row = self.rows.entry(id.to_owned()).or_default();
        row.requested = self.counter;
        if row.in_flight {
            return Ask::Wait;
        }
        row.in_flight = true;
        Ask::Start(self.counter)
    }

    pub fn finished(&mut self, id: &str, number: u64) -> Done {
        let Some(row) = self.rows.get_mut(id) else {
            return Done {
                apply: false,
                next: None,
            };
        };
        row.in_flight = false;
        let apply = number > row.shown;
        if apply {
            row.shown = number;
        }
        let next = (row.requested > number).then_some(row.requested);
        row.in_flight = next.is_some();
        Done { apply, next }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_during_a_read_waits_and_one_more_read_covers_all_of_them() {
        let mut g = FetchGate::default();
        let first = match g.request("agy:a") {
            Ask::Start(n) => n,
            other => panic!("{other:?}"),
        };
        assert_eq!(g.request("agy:a"), Ask::Wait);
        assert_eq!(g.request("agy:a"), Ask::Wait);
        let done = g.finished("agy:a", first);
        assert!(done.apply, "newer than the screen");
        let next = done.next.expect("the changes that waited");
        assert!(next > first);
        assert_eq!(g.request("agy:a"), Ask::Wait);
        let done = g.finished("agy:a", next);
        assert!(done.apply);
        assert!(done.next.is_some(), "and the change that came during it");
    }

    #[test]
    fn nothing_is_asked_when_nothing_changed_meanwhile() {
        let mut g = FetchGate::default();
        let Ask::Start(n) = g.request("agy:a") else {
            panic!("a read");
        };
        assert_eq!(
            g.finished("agy:a", n),
            Done {
                apply: true,
                next: None
            }
        );
        assert!(matches!(g.request("agy:a"), Ask::Start(_)));
    }

    #[test]
    fn an_older_answer_never_replaces_a_newer_one() {
        let mut g = FetchGate::default();
        let Ask::Start(first) = g.request("agy:a") else {
            panic!("a read");
        };
        g.rows.get_mut("agy:a").expect("a row").shown = first + 5;
        assert!(!g.finished("agy:a", first).apply);
    }

    #[test]
    fn rows_are_read_separately() {
        let mut g = FetchGate::default();
        assert!(matches!(g.request("agy:a"), Ask::Start(_)));
        assert!(
            matches!(g.request("agy:b"), Ask::Start(_)),
            "another row's read is its own"
        );
        assert_eq!(g.request("agy:a"), Ask::Wait);
        assert_eq!(g.request("agy:b"), Ask::Wait);
    }

    #[test]
    fn an_answer_nobody_asked_for_is_dropped() {
        let mut g = FetchGate::default();
        assert_eq!(
            g.finished("agy:x", 3),
            Done {
                apply: false,
                next: None
            }
        );
    }
}
