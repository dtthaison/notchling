use notchling_locale as t;
use std::collections::HashMap;
use std::sync::Arc;

use notchling_kit::Ink;

use super::branch::ChangedFile;
use super::git::{chunks, DiffText};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Slot {
    Loading,
    Ready {
        chunks: Vec<(Ink, Arc<str>)>,
        truncated: bool,
    },
    Failed(String),
}

impl Slot {
    pub fn from_diff(diff: &DiffText) -> Slot {
        Slot::Ready {
            chunks: chunks(diff)
                .into_iter()
                .map(|(ink, text)| (ink, Arc::from(text)))
                .collect(),
            truncated: diff.truncated,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    File(usize),
    Chunk(Ink, Arc<str>),
    Note(String, Ink),
}

pub fn rows(files: &[ChangedFile], shown: &[usize], open: &HashMap<String, Slot>) -> Vec<Row> {
    let mut out = Vec::with_capacity(shown.len());
    for &index in shown {
        let Some(file) = files.get(index) else {
            continue;
        };
        out.push(Row::File(index));
        match open.get(&file.path) {
            None => {}
            Some(Slot::Loading) => out.push(Row::Note(
                t::chat::changes_reading().to_owned(),
                Ink::TextMuted,
            )),
            Some(Slot::Failed(why)) => out.push(Row::Note(why.clone(), Ink::Expired)),
            Some(Slot::Ready { chunks, truncated }) => {
                if chunks.is_empty() {
                    out.push(Row::Note(
                        t::chat::changes_none_to_show().to_owned(),
                        Ink::TextMuted,
                    ));
                }
                for (ink, text) in chunks {
                    out.push(Row::Chunk(*ink, text.clone()));
                }
                if *truncated {
                    out.push(Row::Note(
                        t::chat::changes_truncated().to_owned(),
                        Ink::TextMuted,
                    ));
                }
            }
        }
    }
    out
}

pub fn splice(old: &[Row], new: &[Row]) -> (usize, usize, usize) {
    let limit = old.len().min(new.len());
    let start = (0..limit).take_while(|&i| old[i] == new[i]).count();
    let end = (0..limit - start)
        .take_while(|&i| old[old.len() - 1 - i] == new[new.len() - 1 - i])
        .count();
    (start, old.len() - start - end, new.len() - start - end)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Refetch {
    pub gone: Vec<String>,
    pub again: Vec<String>,
}

pub fn refetch(old: &[ChangedFile], new: &[ChangedFile], open: &HashMap<String, Slot>) -> Refetch {
    let before: HashMap<&str, &ChangedFile> = old.iter().map(|f| (f.path.as_str(), f)).collect();
    let after: HashMap<&str, &ChangedFile> = new.iter().map(|f| (f.path.as_str(), f)).collect();
    let mut out = Refetch::default();
    for path in open.keys() {
        match (before.get(path.as_str()), after.get(path.as_str())) {
            (_, None) => out.gone.push(path.clone()),
            (Some(was), Some(now)) if was == now => {}
            _ => out.again.push(path.clone()),
        }
    }
    out.gone.sort();
    out.again.sort();
    out
}

#[cfg(test)]
#[path = "branch_rows_tests.rs"]
mod tests;
