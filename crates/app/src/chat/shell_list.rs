use std::collections::HashSet;

use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::shells::lsof_args;

use super::paging::{Counts, Item, Page};

pub const PROBE_CHUNK: usize = 100;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    pub ids: Vec<String>,
    pub counts: Counts,
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
    ) -> Listing {
        let entries: Vec<Entry> = conversation
            .shells
            .iter()
            .map(|shell| Entry {
                id: &shell.id,
                unfinished: !conversation.finished_agents.contains_key(&shell.id),
            })
            .collect();
        let shown = self.page.show(session, &entries, more);
        Listing {
            ids: shown
                .indexes
                .iter()
                .map(|&i| entries[i].id.to_owned())
                .collect(),
            counts: shown.counts,
        }
    }
}

pub fn probe_paths(conversation: &Conversation, listing: &Listing) -> Vec<String> {
    let shown: HashSet<&str> = listing.ids.iter().map(String::as_str).collect();
    let mut seen = HashSet::new();
    conversation
        .shells
        .iter()
        .rev()
        .filter(|s| shown.contains(s.id.as_str()))
        .filter(|s| !conversation.finished_agents.contains_key(&s.id))
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
