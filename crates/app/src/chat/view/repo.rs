use std::sync::Arc;

use gpui::Context;
use serde_json::Value;

use super::{now_ms, ChatView};
use crate::chat::git::{self, DiffText, Gate, GitModel, Why};

#[derive(Default)]
pub struct State {
    gate: Gate,
    folder: Option<String>,
    status: Option<Arc<GitModel>>,
    preview: Option<Arc<GitModel>>,
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

    pub fn show_preview(&mut self) {
        self.preview = Some(git::preview_model());
    }

    pub fn previewing(&self) -> bool {
        self.preview.is_some()
    }
}

impl ChatView {
    pub(super) fn details_visible(&self) -> bool {
        !self.page.is_open()
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
        let visible = self.details_visible();
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
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { acts.actions.read(&request) })
                .await;
            this.update(cx, |view, cx| view.git_status_in(folder, result, cx))
                .ok();
        })
        .detach();
    }

    fn git_status_in(
        &mut self,
        folder: String,
        result: Result<Value, String>,
        cx: &mut Context<Self>,
    ) {
        self.repo.gate.done();
        let Ok(reply) = result else {
            return;
        };
        let model = git::parse_status(&reply).map(Arc::new);
        if self.repo.folder.as_deref() == Some(folder.as_str()) && self.repo.status == model {
            return;
        }
        self.repo.folder = Some(folder);
        self.repo.status = model;
        self.refresh_details(cx);
        self.git_diff_again(cx);
    }

    pub(super) fn open_git_diff(&mut self, path: &str, cx: &mut Context<Self>) {
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

    fn git_diff_again(&mut self, cx: &mut Context<Self>) {
        let (Some(path), Some(folder)) = (self.page.diff_path(), self.selected_folder()) else {
            return;
        };
        let asked = self.page.asked();
        self.fetch_git_diff(folder, path, asked, cx);
    }

    fn fetch_git_diff(&mut self, folder: String, path: String, asked: u64, cx: &mut Context<Self>) {
        let acts = self.acts(cx);
        let request = git::diff_request(&folder, &path);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { acts.actions.read(&request) })
                .await;
            let parsed: Result<DiffText, String> = result.and_then(|reply| {
                git::parse_diff(&reply).ok_or_else(|| "The diff came back unreadable.".to_owned())
            });
            this.update(cx, |view, cx| {
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
