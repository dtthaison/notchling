use notchling_locale as t;
use std::sync::Arc;

use gpui::Context;
use serde_json::{json, Value};

use super::{now_ms, ChatView};
use crate::chat::branch::{self, BranchModel};
use crate::chat::git::{self, DiffText, Gate, GitModel, Why};

struct Answer {
    base: Option<String>,
    result: Result<Value, String>,
    stat: Option<Result<Value, String>>,
}

#[derive(Default)]
pub struct State {
    gate: Gate,
    folder: Option<String>,
    status: Option<Arc<GitModel>>,
    preview: Option<Arc<GitModel>>,
    diff_last: Option<i64>,
    diff_inflight: bool,
    branch: Option<std::sync::Arc<BranchModel>>,
    preview_branch: Option<std::sync::Arc<BranchModel>>,
    pub(super) base_choice: Option<String>,
    pub(super) changes_shown: bool,
    pub(super) diff_via: Option<(String, Option<String>)>,
}

impl State {
    pub fn model_for(&self, folder: Option<&str>) -> Option<Arc<GitModel>> {
        if self.preview.is_some() {
            return self.preview.clone();
        }
        let folder = folder?;
        if self.folder.as_deref() == Some(folder) {
            self.status.clone()
        } else {
            None
        }
    }

    pub fn branch_for(&self, folder: Option<&str>) -> Option<Arc<BranchModel>> {
        if self.preview_branch.is_some() {
            return self.preview_branch.clone();
        }
        let folder = folder?;
        if self.folder.as_deref() == Some(folder) {
            self.branch.clone()
        } else {
            None
        }
    }

    pub fn show_preview(&mut self) {
        self.preview = Some(git::preview_model());
        self.preview_branch = Some(branch::preview_model());
    }

    pub fn gate_reset(&mut self) {
        self.gate.reset();
    }

    pub fn previewing(&self) -> bool {
        self.preview.is_some()
    }
}

impl ChatView {
    pub(super) fn details_visible(&self) -> bool {
        !self.page.is_open()
            && !self.repo.changes_shown
            && !self.terminal.shown()
            && !self.simulator.shown()
            && !self.details_hidden
    }

    pub(super) fn selected_folder(&self) -> Option<String> {
        self.selected_session()
            .map(|s| s.agent.cwd.clone())
            .filter(|cwd| !cwd.is_empty())
    }

    pub(super) fn git_for_details(&self) -> Option<Arc<GitModel>> {
        self.repo.model_for(self.selected_folder().as_deref())
    }

    pub(super) fn git_ask(&mut self, why: Why, cx: &mut Context<Self>) {
        if self.root.fixture.is_some() {
            return;
        }
        let folder = self.selected_folder();
        if self.repo.branch.is_some() && self.repo.folder != folder {
            self.repo.branch = None;
            self.repo.base_choice = None;
            self.push_branch(cx);
        }
        let visible = self.details_visible() || self.changes_visible();
        if !self
            .repo
            .gate
            .ask(why, folder.as_deref(), visible, now_ms())
        {
            return;
        }
        let Some(folder) = folder else {
            return;
        };
        let acts = self.acts(cx);
        let request = git::status_request(&folder);
        let base = self.repo.base_choice.clone();
        let stat_request = branch::stat_request(&folder, base.as_deref());
        cx.spawn(async move |this, cx| {
            let (result, stat) = cx
                .background_executor()
                .spawn(async move {
                    let result = acts.actions.read(&request);
                    let in_repo = result.as_ref().is_ok_and(|r| r["repo"] == json!(true));
                    let stat = in_repo.then(|| acts.actions.read(&stat_request));
                    (result, stat)
                })
                .await;
            this.update(cx, |view, cx| {
                view.git_status_in(folder, Answer { base, result, stat }, cx)
            })
            .ok();
        })
        .detach();
    }

    fn git_status_in(&mut self, folder: String, answer: Answer, cx: &mut Context<Self>) {
        self.repo.gate.done();
        if self.selected_folder().as_deref() != Some(folder.as_str()) {
            self.git_ask(Why::Folder, cx);
            return;
        }
        let Answer { base, result, stat } = answer;
        let Ok(reply) = result else {
            return;
        };
        let model = git::parse_status(&reply).map(Arc::new);
        let same_folder = self.repo.folder.as_deref() == Some(folder.as_str());
        let stale = base != self.repo.base_choice;
        let rejected = base.is_some() && matches!(stat, Some(Err(_)));
        let wanted = match stat {
            Some(Ok(value)) if !stale => branch::parse_stat(&value).map(Arc::new),
            _ if stale && same_folder => self.repo.branch.clone(),
            _ => None,
        };
        if rejected && !stale {
            self.repo.base_choice = None;
        }
        let unchanged = same_folder && self.repo.status == model && self.repo.branch == wanted;
        if !unchanged {
            self.repo.folder = Some(folder);
            self.repo.status = model;
            self.repo.branch = wanted;
            self.refresh_details(cx);
            self.push_branch(cx);
            self.git_diff_again(cx);
        }
        if stale || rejected {
            self.repo.gate.reset();
            self.git_ask(Why::Opened, cx);
        }
    }

    pub(super) fn open_git_diff(&mut self, path: &str, cx: &mut Context<Self>) {
        self.repo.diff_via = None;
        let folder = self.selected_folder();
        let previewing = self.repo.previewing();
        if folder.is_none() && !previewing {
            return;
        }
        super::select::clear();
        let asked = self.page.show_diff(path);
        if self.root.fixture.is_some() {
            if previewing {
                self.page.set_diff(asked, path, Ok(git::preview_diff()));
            }
        } else if let Some(folder) = folder {
            self.fetch_git_diff(folder, path.to_owned(), asked, cx);
        }
        cx.notify();
    }

    pub(super) fn git_diff_tick(&mut self, cx: &mut Context<Self>) {
        if self.root.fixture.is_some()
            || self.repo.diff_inflight
            || !git::diff_due(self.repo.diff_last, now_ms())
        {
            return;
        }
        self.git_diff_again(cx);
    }

    pub(super) fn close_git_diff(&mut self, cx: &mut Context<Self>) {
        if self.page.diff_path().is_some() {
            self.close_page(cx);
        }
    }

    fn git_diff_again(&mut self, cx: &mut Context<Self>) {
        let (Some(path), Some(folder)) = (self.page.diff_path(), self.selected_folder()) else {
            return;
        };
        let asked = self.page.asked();
        self.fetch_git_diff(folder, path, asked, cx);
    }

    pub(super) fn fetch_git_diff(
        &mut self,
        folder: String,
        path: String,
        asked: u64,
        cx: &mut Context<Self>,
    ) {
        self.repo.diff_last = Some(now_ms());
        self.repo.diff_inflight = true;
        let acts = self.acts(cx);
        let request = match &self.repo.diff_via {
            Some((base, from)) => branch::file_request(&folder, base, &path, from.as_deref()),
            None => git::diff_request(&folder, &path),
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { acts.actions.read(&request) })
                .await;
            let parsed: Result<DiffText, String> = result.and_then(|reply| {
                git::parse_diff(&reply).ok_or_else(|| t::chat::changes_unreadable().to_owned())
            });
            this.update(cx, |view, cx| {
                view.repo.diff_inflight = false;
                if view.page.set_diff(asked, &path, parsed) {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn preview_git(&mut self, diff: bool, cx: &mut Context<Self>) {
        self.repo.show_preview();
        if diff {
            self.open_git_diff(git::PREVIEW_PATH, cx);
        }
        self.refresh_details(cx);
    }
}
