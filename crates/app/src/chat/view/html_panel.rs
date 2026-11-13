use std::path::Path;

use gpui::{div, prelude::*, px, Context, Div, Window};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::icon_button;
use notchling_kit::{Appearance, Icon, IconSize};

use super::page_panel::{home, FileState};
use super::ChatView;
use crate::chat::file_panel;
use crate::chat::html_preview::{self, HtmlView, Look};
use crate::mac::gate;
use crate::pages::web::{PanelPage, PanelWeb};

pub(super) fn looks(html: &HtmlView, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let seg = |id: &'static str, shape: Icon, tip: &'static str, on: bool, look: Look| {
        gate::tip_text(
            icon_button(
                id,
                shape,
                IconSize::Button.points(),
                if on {
                    ButtonKind::Secondary
                } else {
                    ButtonKind::Ghost
                },
                PartState::Rest,
                a,
            ),
            tip.into(),
            a,
        )
        .flex_shrink_0()
        .on_click(cx.listener(move |this, _, window, cx| this.set_html_look(look, window, cx)))
    };
    div()
        .flex()
        .items_center()
        .gap(px(2.0))
        .flex_shrink_0()
        .child(seg(
            "page-panel-look-page",
            Icon::Page,
            html.page_tip(),
            html.is_page(),
            Look::Page,
        ))
        .child(seg(
            "page-panel-look-source",
            if html.changes {
                Icon::Branch
            } else {
                Icon::Code
            },
            html.other_tip(),
            !html.is_page(),
            Look::Source,
        ))
}

impl ChatView {
    pub(super) fn refresh_html(&mut self, cx: &mut Context<Self>) {
        let web = self.page.web.clone();
        let Some(shown) = self.page.shown.as_mut() else {
            return;
        };
        let Some(html) = shown.html.as_mut().filter(|h| h.is_page()) else {
            return;
        };
        if !html.moved() {
            return;
        }
        if html.stamp.is_none() {
            shown.error = Some(html_preview::GONE.to_owned());
        } else {
            if shown.error.as_deref() == Some(html_preview::GONE) {
                shown.error = None;
            }
            if let Some(web) = web.borrow().as_ref() {
                web.reload();
            }
        }
        cx.notify();
    }

    pub(super) fn set_html_look(
        &mut self,
        look: Look,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(shown) = self.page.shown.as_mut() else {
            return;
        };
        let Some(html) = shown.html.as_mut().filter(|h| h.look != look) else {
            return;
        };
        html.look = look;
        html.stamp = file_panel::stamp(&html.path);
        let (path, changes) = (html.path.clone(), html.changes);
        shown.error = None;
        match (look, changes) {
            (Look::Source, true) => {}
            (Look::Source, false) => {
                let file = FileState::read(&path);
                shown.subtitle = file_panel::subtitle(&path, home().as_deref(), &file.loaded.body);
                shown.file = Some(file);
            }
            (Look::Page, _) => shown.file = None,
        }
        self.page.web.borrow_mut().take();
        super::select::clear();
        if look == Look::Page && !self.page.render_only {
            self.show_html_web(window, cx);
        }
        cx.notify();
    }

    fn show_html_web(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self
            .page
            .shown
            .as_ref()
            .and_then(|s| s.html.as_ref())
            .map(|h| h.path.clone())
        else {
            return;
        };
        match self.make_html_web(&path, window, cx) {
            Ok(web) => *self.page.web.borrow_mut() = Some(web),
            Err(e) => {
                if let Some(shown) = self.page.shown.as_mut() {
                    shown.error = Some(e);
                }
            }
        }
    }

    pub(super) fn offer_page(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(folder), Some(model)) = (self.selected_folder(), self.git_for_details()) else {
            return;
        };
        let Some(file) = model.files.iter().find(|f| f.path == path) else {
            return;
        };
        if !html_preview::git_offers(&file.status) {
            return;
        }
        let Some(found) = html_preview::git_file(Path::new(&folder), path) else {
            return;
        };
        let look = html_preview::git_look(&file.status);
        self.page
            .attach_html(HtmlView::git(found, look, home().as_deref()));
        if look == Look::Page && !self.page.render_only {
            self.show_html_web(window, cx);
        }
        cx.notify();
    }

    pub(super) fn open_git_row(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_git_diff(path, cx);
        self.offer_page(path, window, cx);
    }

    pub fn preview_html(&mut self, how: &str, window: &mut Window, cx: &mut Context<Self>) {
        match how {
            "source" => self.set_html_look(Look::Source, window, cx),
            "web" => self.show_html_web(window, cx),
            _ => {}
        }
    }

    pub(super) fn make_html_web(
        &self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<PanelWeb, String> {
        let source = html_preview::source(path)?;
        PanelWeb::open(window, PanelPage::Untrusted(&source), self.web_hooks(cx))
    }
}
