use std::sync::Arc;

use gpui::Context;
use serde_json::Value;

use super::{now_ms, ChatView};
use crate::chat::data::Session;
use crate::chat::git::Why;
use crate::chat::git_gates::{fresh_after, MinuteGate, SlowGate};
use crate::chat::inspector::SubagentRow;
use crate::chat::worktrees::{self, GitSection, Occupancy, Worktrees};

const MINUTE_MS: i64 = 60_000;

#[derive(Default)]
pub struct WorktreesState {
    gate: SlowGate,
    minute: MinuteGate,
    folder: Option<String>,
    rows: Option<Arc<Worktrees>>,
    kinds: Vec<(String, u8)>,
    preview: bool,
}

fn kind(held: &Option<Occupancy>) -> u8 {
    match held {
        None => 0,
        Some(Occupancy::Idle { .. }) => 1,
        Some(Occupancy::NeedsInput { .. }) => 2,
        Some(Occupancy::Running { .. }) => 3,
    }
}

fn paths(rows: &Worktrees) -> Vec<(&str, &str)> {
    rows.list
        .iter()
        .map(|entry| (entry.path.as_str(), entry.real.as_str()))
        .collect()
}

impl WorktreesState {
    pub fn rows_for(&self, folder: Option<&str>) -> Option<Arc<Worktrees>> {
        if self.preview {
            return self.rows.clone();
        }
        if self.folder.as_deref() == folder && folder.is_some() {
            self.rows.clone()
        } else {
            None
        }
    }

    pub fn show_preview(&mut self) {
        self.preview = true;
        self.rows = Some(Arc::new(worktrees::preview()));
    }

    pub fn previewing(&self) -> bool {
        self.preview
    }

    pub fn due(
        &mut self,
        in_repo: bool,
        why: Why,
        folder: Option<&str>,
        visible: bool,
        now_ms: i64,
    ) -> bool {
        in_repo && self.gate.ask(why, folder, visible, now_ms)
    }

    fn kinds_now(
        &self,
        folder: &str,
        sessions: &[Session],
        subagents: &[SubagentRow],
    ) -> Option<Vec<(String, u8)>> {
        let rows = self
            .rows
            .as_ref()
            .filter(|_| self.folder.as_deref() == Some(folder))?;
        let held = worktrees::occupancy(&paths(rows), sessions, subagents);
        Some(
            rows.list
                .iter()
                .zip(&held)
                .map(|(entry, held)| (entry.path.clone(), kind(held)))
                .collect(),
        )
    }

    pub fn occupancy_changed(
        &self,
        folder: &str,
        sessions: &[Session],
        subagents: &[SubagentRow],
    ) -> bool {
        self.kinds_now(folder, sessions, subagents)
            .is_some_and(|after| !fresh_after(&self.kinds, &after).is_empty())
    }

    pub fn seed(&mut self, folder: &str, sessions: &[Session], subagents: &[SubagentRow]) {
        if !self.kinds.is_empty() {
            return;
        }
        if let Some(after) = self.kinds_now(folder, sessions, subagents) {
            self.kinds = after;
        }
    }

    pub fn fresh_paths(
        &mut self,
        folder: &str,
        sessions: &[Session],
        subagents: &[SubagentRow],
    ) -> Vec<String> {
        let Some(after) = self.kinds_now(folder, sessions, subagents) else {
            self.kinds.clear();
            return Vec::new();
        };
        let fresh = fresh_after(&self.kinds, &after);
        self.kinds = after;
        fresh
    }

    pub fn apply(&mut self, folder: &str, result: Result<Value, String>) -> bool {
        let Ok(reply) = result else {
            return false;
        };
        let parsed = worktrees::parse_worktrees(&reply).map(Arc::new);
        if self.folder.as_deref() == Some(folder) && self.rows == parsed {
            return false;
        }
        if self.folder.as_deref() != Some(folder) {
            self.kinds.clear();
        }
        self.folder = Some(folder.to_owned());
        self.rows = parsed;
        true
    }

    pub fn minute_due(&mut self, visible: bool, now_ms: i64) -> bool {
        self.minute.due(visible, now_ms)
    }

    pub fn done(&mut self, now_ms: i64) {
        self.gate.done(false, now_ms);
    }
}

pub(super) fn section_of(view: &ChatView) -> Option<Arc<GitSection>> {
    let folder = view.selected_folder();
    let status = view.git_for_details()?;
    let stat = view.repo.branch_for(folder.as_deref());
    let rows = view.repo.wt.rows_for(folder.as_deref());
    let previewing = view.repo.wt.previewing();
    let held = match rows.as_deref() {
        Some(rows) if previewing => worktrees::preview_occupancy(rows.list.len()),
        Some(rows) => worktrees::occupancy(&paths(rows), &view.sessions, &view.subagent_rows),
        None => Vec::new(),
    };
    let minute_ms = if previewing {
        worktrees::PREVIEW_NOW_MS
    } else {
        now_ms() / MINUTE_MS * MINUTE_MS
    };
    Some(Arc::new(worktrees::section(
        &status,
        stat.as_deref(),
        rows.as_deref(),
        &held,
        minute_ms,
    )))
}

impl ChatView {
    pub(super) fn worktrees_after_load(&mut self, cx: &mut Context<Self>) {
        let Some(folder) = self.selected_folder() else {
            return;
        };
        if self
            .repo
            .wt
            .occupancy_changed(&folder, &self.sessions, &self.subagent_rows)
        {
            self.worktrees_ask(Why::TurnEnded, cx);
        }
    }

    pub(super) fn worktrees_ask(&mut self, why: Why, cx: &mut Context<Self>) {
        if self.root.fixture.is_some() {
            return;
        }
        let folder = self.selected_folder();
        let in_repo = self.repo.model_for(folder.as_deref()).is_some();
        let visible = self.details_visible();
        if !self
            .repo
            .wt
            .due(in_repo, why, folder.as_deref(), visible, now_ms())
        {
            return;
        }
        let Some(folder) = folder else {
            return;
        };
        let fresh = self
            .repo
            .wt
            .fresh_paths(&folder, &self.sessions, &self.subagent_rows);
        let request = worktrees::worktrees_request(&folder, &fresh);
        let acts = self.acts(cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { acts.actions.read(&request) })
                .await;
            this.update(cx, |view, cx| view.worktrees_in(folder, result, cx))
                .ok();
        })
        .detach();
    }

    fn worktrees_in(
        &mut self,
        folder: String,
        result: Result<Value, String>,
        cx: &mut Context<Self>,
    ) {
        self.repo.wt.done(now_ms());
        if self.selected_folder().as_deref() != Some(folder.as_str()) {
            self.worktrees_ask(Why::Folder, cx);
            return;
        }
        let changed = self.repo.wt.apply(&folder, result);
        self.repo
            .wt
            .seed(&folder, &self.sessions, &self.subagent_rows);
        if changed {
            self.refresh_details(cx);
        }
    }
}

#[cfg(test)]
#[path = "worktrees_ask_tests.rs"]
mod tests;
