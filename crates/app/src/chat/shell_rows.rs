use notchling_locale as t;
use std::collections::HashSet;

use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::shells::{self, ShellState};
use notchling_kit::{Icon, Ink, Status};

use super::inspector::{Mark, ShellRow};

pub fn shell_rows(
    conversation: &Conversation,
    shown: Option<&[String]>,
    writers: &dyn Fn(&str) -> Vec<i32>,
) -> Vec<ShellRow> {
    let shown: Option<HashSet<&str>> = shown.map(|ids| ids.iter().map(String::as_str).collect());
    running_first(
        conversation
            .shells
            .iter()
            .filter(|shell| {
                shown
                    .as_ref()
                    .is_none_or(|ids| ids.contains(shell.id.as_str()))
            })
            .map(|shell| {
                let state = shells::state(shell, &conversation.finished_agents, writers);
                let (status, word, mark) = match state {
                    ShellState::Running => (
                        Status::Working,
                        t::chat::status_running().to_owned(),
                        Mark::Spinner,
                    ),
                    ShellState::Ended(how) => {
                        let ok = how == "completed";
                        (
                            if ok { Status::Done } else { Status::Needs },
                            capitalized(&how),
                            if ok {
                                Mark::Icon(Icon::Done, Ink::Done)
                            } else {
                                Mark::Icon(Icon::Failed, Ink::Expired)
                            },
                        )
                    }
                };
                ShellRow {
                    title: if shell.title.is_empty() {
                        shell.command.clone()
                    } else {
                        shell.title.clone()
                    },
                    command: shell.command.clone(),
                    status,
                    status_word: word,
                    running: status == Status::Working,
                    mark,
                    output_path: shell.output_path.clone(),
                }
            })
            .collect(),
    )
}

fn running_first(mut rows: Vec<ShellRow>) -> Vec<ShellRow> {
    rows.reverse();
    rows.sort_by_key(|row| !row.running);
    rows
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
