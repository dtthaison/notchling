use std::path::PathBuf;

use gpui::{Context, Window};
use notchling_locale as t;

use super::page_panel::{home, FileState, Shown};
use super::ChatView;
use crate::chat::file_panel::{self, Body, Code};
use crate::chat::page_panel::Opens;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Via {
    Link,
    Drop,
    Agent,
}

impl ChatView {
    pub(super) fn load_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let again = path.clone();
            let fresh = cx
                .background_executor()
                .spawn(async move { FileState::read(&again) })
                .await;
            this.update(cx, |view, cx| {
                if let Some(Shown {
                    file: Some(file),
                    subtitle,
                    ..
                }) = view.page.shown.as_mut()
                {
                    if file.path == path {
                        let mut fresh = fresh;
                        fresh.opened = std::mem::take(&mut file.opened);
                        *subtitle =
                            file_panel::subtitle(&path, home().as_deref(), &fresh.loaded.body);
                        *file = fresh;
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn toggle_line(&mut self, number: usize, cx: &mut Context<Self>) {
        let Some(Shown {
            file: Some(file), ..
        }) = self.page.shown.as_mut()
        else {
            return;
        };
        if file.opened.remove(&number).is_none() {
            let Body::Jsonl(j) = &file.loaded.body else {
                return;
            };
            if let Some(row) = j.rows.iter().find(|r| r.number == number) {
                let code = Code::of_json(&row.raw);
                file.opened.insert(number, code);
            }
        }
        cx.notify();
    }

    pub(super) fn load_more_lines(&mut self, cx: &mut Context<Self>) {
        let Some(Shown {
            file: Some(file), ..
        }) = self.page.shown.as_mut()
        else {
            return;
        };
        let Body::Jsonl(j) = &file.loaded.body else {
            return;
        };
        if file.paging || !j.more() {
            return;
        }
        file.paging = true;
        let (path, shown) = (file.path.clone(), j.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let again = path.clone();
            let more = cx
                .background_executor()
                .spawn(async move { crate::chat::jsonl_view::earlier(&again, &shown) })
                .await;
            this.update(cx, |view, cx| {
                if let Some(Shown {
                    file: Some(file), ..
                }) = view.page.shown.as_mut()
                {
                    if file.path == path {
                        file.paging = false;
                        if let Some(more) = more {
                            file.loaded.body = Body::Jsonl(more);
                        }
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn open_dropped(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = paths.first() {
            self.classify_then_show(path.clone(), Via::Drop, window, cx);
        }
    }

    pub(super) fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.classify_then_show(path, Via::Agent, window, cx);
    }

    pub(super) fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.classify_then_show(path, Via::Link, window, cx);
    }

    fn classify_then_show(
        &mut self,
        path: PathBuf,
        via: Via,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            let opens = cx
                .background_executor()
                .spawn(async move { file_panel::opens(&path) })
                .await;
            this.update_in(cx, |view, window, cx| match opens {
                Opens::Folder(_) if via == Via::Drop => {
                    view.tools.error = Some(t::chat::panel_drop_folder().to_owned());
                    cx.notify();
                }
                Opens::System(path) if via != Via::Link => {
                    view.show_page(Opens::File(path), "", window, cx)
                }
                other => view.show_page(other, "", window, cx),
            })
            .ok();
        })
        .detach();
    }
}
