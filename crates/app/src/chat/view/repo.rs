use notchling_locale as t;
use std::sync::Arc;

use gpui::Context;
use serde_json::{json, Value};

use super::worktrees_ask::WorktreesState;
use super::{now_ms, ChatView};
use crate::acting::Request;
use crate::chat::branch::{self, BranchModel};
use crate::chat::branches_state::Feed as BranchesFeed;
use crate::chat::changes_target::{self, Applied, DiffVia, Target, TargetState};
use crate::chat::git::{self, DiffText, Gate, GitModel, Why};
use crate::chat::remote_control::Action;
use crate::chat::worktrees;
use notchling_engine::sessions::Agent;

struct Answer {
    base: Option<String>,
    result: Result<Value, String>,
    stat: Option<Result<Value, String>>,
    other: Option<(Target, Option<String>, Result<Value, String>)>,
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
    pub(super) diff_via: Option<DiffVia>,
    pub(super) other: Option<TargetState>,
    pub(super) changes_from_branches: bool,
    pub(super) branches_shown: bool,
    pub(super) branches: BranchesFeed,
    pub(super) wt: WorktreesState,
}

pub(super) fn folder_of(agent: &Agent) -> Option<String> {
    Some(worktrees::session_folder(agent))
        .filter(|folder| !folder.is_empty())
        .map(str::to_owned)
}

pub(super) fn own_stat_request(folder: &str, base_choice: Option<&str>) -> Request {
    changes_target::stat_request(folder, &Target::Own, base_choice)
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
        self.wt.show_preview();
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
            && !self.repo.branches_shown
            && !self.terminal.shown()
            && !self.simulator.shown()
            && !self.details_hidden
    }

    pub(super) fn selected_folder(&self) -> Option<String> {
        self.selected_session()
            .filter(|s| !self.remote_refused(&s.agent.id, Action::Files))
            .and_then(|s| folder_of(&s.agent))
    }

    pub(super) fn git_for_details(&self) -> Option<Arc<GitModel>> {
        self.repo.model_for(self.selected_folder().as_deref())
    }

    pub(super) fn git_ask(&mut self, why: Why, cx: &mut Context<Self>) {
        if self.root.fixture.is_some() {
            return;
        }
        let folder = self.selected_folder();
        if self.repo.other.is_some() && self.repo.folder.is_some() && self.repo.folder != folder {
            changes_target::on_folder_change(
                &mut self.repo.other,
                &mut self.repo.changes_from_branches,
            );
            self.push_branch(cx);
        }
        if self.repo.branch.is_some() && self.repo.folder != folder {
            self.repo.branch = None;
            self.repo.base_choice = None;
            self.push_branch(cx);
        }
        self.worktrees_ask(why, cx);
        self.branches_ask(why, cx);
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
        let stat_request = own_stat_request(&folder, base.as_deref());
        let other_request = self
            .changes_visible()
            .then(|| changes_target::asking(self.repo.other.as_ref()))
            .flatten()
            .map(|(target, base)| {
                let request = changes_target::stat_request(&folder, &target, base.as_deref());
                (target, base, request)
            });
        cx.spawn(async move |this, cx| {
            let (result, stat, other) = cx
                .background_executor()
                .spawn(async move {
                    let result = acts.actions.read(&request);
                    let in_repo = result.as_ref().is_ok_and(|r| r["repo"] == json!(true));
                    let stat = in_repo.then(|| acts.actions.read(&stat_request));
                    let other = other_request
                        .filter(|_| in_repo)
                        .map(|(target, base, request)| (target, base, acts.actions.read(&request)));
                    (result, stat, other)
                })
                .await;
            this.update(cx, |view, cx| {
                let answer = Answer {
                    base,
                    result,
                    stat,
                    other,
                };
                view.git_status_in(folder, answer, cx)
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
        let Answer {
            base,
            result,
            stat,
            other,
        } = answer;
        let Ok(reply) = result else {
            return;
        };
        let model = git::parse_status(&reply).map(Arc::new);
        let in_repo = model.is_some();
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
        let applied = other.map(|(target, asked, result)| {
            changes_target::apply_stat(&mut self.repo.other, &target, asked.as_deref(), result)
        });
        let unchanged = same_folder
            && self.repo.status == model
            && self.repo.branch == wanted
            && applied != Some(Applied::Changed);
        if !unchanged {
            self.repo.folder = Some(folder);
            self.repo.status = model;
            self.repo.branch = wanted;
            self.refresh_details(cx);
            self.push_branch(cx);
            self.git_diff_again(cx);
        }
        if in_repo {
            self.worktrees_ask(Why::Opened, cx);
        }
        let again = changes_target::needs_ask_again(
            self.repo.other.as_ref(),
            applied.as_ref(),
            in_repo,
            self.changes_visible(),
        );
        if stale || rejected || again {
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
            Some(via) => via.request(&folder, &path),
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
