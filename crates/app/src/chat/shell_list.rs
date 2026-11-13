use std::collections::HashSet;

use notchling_engine::transcript::model::{BackgroundShell, Conversation};
use notchling_engine::transcript::shells::lsof_args;

use super::paging::{Counts, Item, Page, PAGE};

pub const PROBE_CHUNK: usize = 100;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    pub ids: Vec<String>,
    pub counts: Counts,
    pub waiting: bool,
}

struct Entry<'a> {
    id: &'a str,
    unfinished: bool,
}

impl Item for Entry<'_> {
    fn id(&self) -> &str {
        self.id
    }

    fn running(&self) -> bool {
        self.unfinished
    }
}

fn ended(conversation: &Conversation, shell: &BackgroundShell) -> bool {
    conversation.finished_agents.contains_key(&shell.id)
}

fn unprobed(
    conversation: &Conversation,
    shell: &BackgroundShell,
    writing: &dyn Fn(&str) -> Option<bool>,
) -> bool {
    !ended(conversation, shell)
        && !shell.output_path.is_empty()
        && writing(&shell.output_path).is_none()
}

fn idle(shell: &BackgroundShell, writing: &dyn Fn(&str) -> Option<bool>) -> bool {
    writing(&shell.output_path) == Some(false)
}

#[derive(Default)]
pub struct ShellPage {
    page: Page,
}

impl ShellPage {
    pub fn show(
        &mut self,
        session: Option<&str>,
        conversation: &Conversation,
        more: usize,
        writing: &dyn Fn(&str) -> Option<bool>,
    ) -> Listing {
        let entries: Vec<Entry> = conversation
            .shells
            .iter()
            .map(|shell| Entry {
                id: &shell.id,
                unfinished: !ended(conversation, shell) && !idle(shell, writing),
            })
            .collect();
        let waiting = !self.page.begun(session)
            && conversation
                .shells
                .iter()
                .any(|shell| unprobed(conversation, shell, writing));
        let shown = if waiting {
            Page::default().show(session, &entries, 0)
        } else {
            self.page.show(session, &entries, more)
        };
        let take = if waiting { PAGE } else { usize::MAX };
        Listing {
            ids: shown
                .indexes
                .iter()
                .take(take)
                .map(|&i| entries[i].id.to_owned())
                .collect(),
            counts: Counts {
                hidden: if waiting { 0 } else { shown.counts.hidden },
                ..shown.counts
            },
            waiting,
        }
    }
}

pub fn probe_paths(
    conversation: &Conversation,
    listing: &Listing,
    writing: &dyn Fn(&str) -> Option<bool>,
) -> Vec<String> {
    let shown: HashSet<&str> = listing.ids.iter().map(String::as_str).collect();
    let mut seen = HashSet::new();
    conversation
        .shells
        .iter()
        .rev()
        .filter(|s| listing.waiting || shown.contains(s.id.as_str()))
        .filter(|s| !ended(conversation, s))
        .filter(|s| {
            if listing.waiting {
                unprobed(conversation, s, writing)
            } else {
                !idle(s, writing)
            }
        })
        .filter(|s| !s.output_path.is_empty() && seen.insert(s.output_path.as_str()))
        .map(|s| s.output_path.clone())
        .collect()
}

pub fn lsof_calls(paths: &[String]) -> Vec<Vec<String>> {
    paths.chunks(PROBE_CHUNK).map(lsof_args).collect()
}

#[cfg(test)]
#[path = "shell_list_tests.rs"]
mod tests;
