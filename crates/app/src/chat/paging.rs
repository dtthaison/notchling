use std::collections::HashSet;

pub const FIRST_DONE: usize = 3;
pub const PAGE: usize = 10;

pub trait Item {
    fn id(&self) -> &str;
    fn running(&self) -> bool;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub total: usize,
    pub running: usize,
    pub hidden: usize,
}

impl Counts {
    pub fn next_page(&self) -> usize {
        self.hidden.min(PAGE)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    pub indexes: Vec<usize>,
    pub counts: Counts,
}

#[derive(Default)]
pub struct Page {
    session: Option<String>,
    started: bool,
    seen: HashSet<String>,
    shown: HashSet<String>,
}

impl Page {
    pub fn show<T: Item>(&mut self, session: Option<&str>, items: &[T], more: usize) -> Shown {
        if self.session.as_deref() != session {
            *self = Page {
                session: session.map(str::to_owned),
                ..Page::default()
            };
        }
        if !self.started {
            if !items.is_empty() {
                self.started = true;
                self.seen.extend(items.iter().map(|i| i.id().to_owned()));
                let any_running = items.iter().any(T::running);
                let first = items.iter().rev().filter(|i| any_running == i.running());
                let first: Vec<&T> = if any_running {
                    first.collect()
                } else {
                    first.take(FIRST_DONE).collect()
                };
                self.shown.extend(first.iter().map(|i| i.id().to_owned()));
            }
        } else {
            for item in items {
                if self.seen.insert(item.id().to_owned()) || item.running() {
                    self.shown.insert(item.id().to_owned());
                }
            }
        }
        for _ in 0..more {
            self.reveal(items, PAGE);
        }
        let indexes: Vec<usize> = (0..items.len())
            .rev()
            .filter(|&i| self.shown.contains(items[i].id()))
            .collect();
        Shown {
            counts: Counts {
                total: items.len(),
                running: items.iter().filter(|i| i.running()).count(),
                hidden: items.len() - indexes.len(),
            },
            indexes,
        }
    }

    fn reveal<T: Item>(&mut self, items: &[T], count: usize) {
        let next: Vec<String> = items
            .iter()
            .rev()
            .filter(|i| !self.shown.contains(i.id()))
            .take(count)
            .map(|i| i.id().to_owned())
            .collect();
        self.shown.extend(next);
    }
}

#[cfg(test)]
#[path = "paging_tests.rs"]
mod tests;
