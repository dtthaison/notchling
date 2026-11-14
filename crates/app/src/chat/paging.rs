use std::collections::{HashMap, HashSet};

use notchling_engine::transcript::subagents::Subagent;

pub const FIRST_DONE: usize = 3;
pub const PAGE: usize = 10;

pub trait Item {
    fn id(&self) -> &str;
    fn running(&self) -> bool;
}

impl<T: Item + ?Sized> Item for &T {
    fn id(&self) -> &str {
        (**self).id()
    }

    fn running(&self) -> bool {
        (**self).running()
    }
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
    pub fn begun(&self, session: Option<&str>) -> bool {
        self.started && self.session.as_deref() == session
    }

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub index: usize,
    pub depth: u8,
    pub children: usize,
    pub open: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupMore {
    pub after: usize,
    pub group: String,
    pub next: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeShown {
    pub entries: Vec<Entry>,
    pub more: Vec<GroupMore>,
    pub counts: Counts,
}

#[derive(Default)]
pub struct Tree {
    session: Option<String>,
    top: Page,
    groups: HashMap<String, Page>,
}

struct Walk<'a> {
    rows: &'a [Subagent],
    kids: HashMap<&'a str, Vec<usize>>,
    open: &'a HashSet<String>,
    more: &'a HashMap<String, usize>,
}

impl Tree {
    pub fn show(
        &mut self,
        session: Option<&str>,
        rows: &[Subagent],
        open: &HashSet<String>,
        more: &HashMap<String, usize>,
    ) -> TreeShown {
        if self.session.as_deref() != session {
            *self = Tree {
                session: session.map(str::to_owned),
                ..Tree::default()
            };
        }
        let mut kids: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, row) in rows.iter().enumerate() {
            kids.entry(row.parent.as_str()).or_default().push(i);
        }
        let top: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].depth <= 1).collect();
        let walk = Walk {
            rows,
            kids,
            open,
            more,
        };
        let items: Vec<&Subagent> = top.iter().map(|&i| &rows[i]).collect();
        let first = self
            .top
            .show(session, &items, more.get("").copied().unwrap_or(0));
        let mut out = TreeShown {
            counts: Counts {
                total: rows.len(),
                running: rows.iter().filter(|r| r.is_running()).count(),
                hidden: first.counts.hidden,
            },
            ..TreeShown::default()
        };
        for at in first.indexes {
            self.emit(&walk, top[at], 1, &mut out);
        }
        out
    }

    fn emit(&mut self, walk: &Walk, index: usize, depth: u8, out: &mut TreeShown) {
        let id = walk.rows[index].id.as_str();
        let children: &[usize] = walk.kids.get(id).map_or(&[], Vec::as_slice);
        let open = !children.is_empty() && walk.open.contains(id);
        out.entries.push(Entry {
            index,
            depth,
            children: children.len(),
            open,
        });
        if !open || depth == u8::MAX {
            return;
        }
        let items: Vec<&Subagent> = children.iter().map(|&i| &walk.rows[i]).collect();
        let want = walk.more.get(id).copied().unwrap_or(0);
        let shown = self
            .groups
            .entry(id.to_owned())
            .or_default()
            .show(Some(id), &items, want);
        for at in shown.indexes {
            self.emit(walk, children[at], depth + 1, out);
        }
        if shown.counts.hidden > 0 {
            out.more.push(GroupMore {
                after: out.entries.len() - 1,
                group: id.to_owned(),
                next: shown.counts.next_page(),
            });
        }
    }
}

#[cfg(test)]
#[path = "paging_tests.rs"]
mod tests;
