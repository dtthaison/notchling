//! Chat's sheets (part U8): New Session from the sidebar's +, Settings from its footer, each drawn
//! over the window until it asks to close. The actions are the app's shared ones (`AppActions`),
//! so the sheets and the notch agree on acting; without them nothing can be sent.

use std::path::Path;
use std::sync::Arc;

use gpui::{div, prelude::*, px, AnyView, Context, Div, Entity};
use notchling_engine::delegate::options::Tool as DelegateTool;
use notchling_engine::delegate::second_opinion::{codex_options, Subject};
use notchling_engine::tools::agy;
use notchling_engine::tools::codex::models;
use notchling_engine::tools::codex::plan::find_binary;
use notchling_kit::ui::{hsla, KitStyled};
use notchling_kit::{Appearance, Ink};

use super::ChatView;
use crate::acting::Acting;
use crate::actions::Actions;
use crate::chat::data::Tool;
use crate::mac::shell::AppActions;
use crate::sheets::new_session::{accounts, folders, NewSession};
use crate::sheets::second_opinion_view::SecondOpinionView;
use crate::sheets::settings_view::SettingsView;
use crate::sheets::view::{Acts, Dismiss, NewSessionView};
use crate::tools::second_opinion::SecondOpinionSheet;

/// The sheet on show.
pub struct Sheet {
    pub view: AnyView,
    _closing: gpui::Subscription,
}

impl ChatView {
    pub(super) fn acts(&self, cx: &mut Context<Self>) -> Acts {
        let actions = cx
            .try_global::<AppActions>()
            .map(|a| a.0.clone())
            .unwrap_or_else(|| Arc::new(Actions::fixed(Acting::Detached)));
        Acts {
            actions,
            sessions: Some(self.source.clone()),
        }
    }

    fn show<V: gpui::Render + gpui::EventEmitter<Dismiss>>(
        &mut self,
        entity: Entity<V>,
        cx: &mut Context<Self>,
    ) {
        let closing = cx.subscribe(&entity, |this, _, _: &Dismiss, cx| {
            this.sheet = None;
            cx.notify();
        });
        self.sheet = Some(Sheet {
            view: entity.into(),
            _closing: closing,
        });
        cx.notify();
    }

    /// New Session over Claude Code's sessions: their lanes and their folders.
    pub(super) fn open_new_session(&mut self, cx: &mut Context<Self>) {
        let agents: Vec<_> = self
            .sessions
            .iter()
            .filter(|s| s.tool == Tool::Claude)
            .map(|s| s.agent.clone())
            .collect();
        let home = self.root.home.to_string_lossy().into_owned();
        let model = NewSession::new(accounts(&agents), folders(&[], &agents), &home);
        let acts = self.acts(cx);
        let entity = cx.new(|_| NewSessionView::new(model, acts));
        self.show(entity, cx);
    }

    /// Second Opinion over a reply or an edit of the selected session: the tools installed, Codex's
    /// cached models, and the app's store (none for a fixture root, so Run is off).
    pub(super) fn open_second_opinion(
        &mut self,
        subject: Subject,
        anchor: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let (row, folder) = (session.agent.id.clone(), session.agent.cwd.clone());
        let home = &self.root.home;
        let mut tools = Vec::new();
        if find_binary(
            home,
            &std::env::var("PATH").unwrap_or_default(),
            Path::is_file,
        )
        .is_some()
        {
            tools.push(DelegateTool::Codex);
        }
        if agy::find_binary(home).is_some() {
            tools.push(DelegateTool::Agy);
        }
        let mut model = SecondOpinionSheet::new(subject, &row, anchor, &folder, tools);
        model.codex_models = codex_options(&models::load(&self.root.codex_dir));
        let acts = self.acts(cx);
        let store = super::opinions::store(cx);
        let entity = cx.new(|cx| SecondOpinionView::new(model, acts, store, cx));
        self.show(entity, cx);
    }

    /// Settings over Notchling's own store; a fixture root has none.
    pub(super) fn open_settings(&mut self, cx: &mut Context<Self>) {
        let data_dir = self
            .root
            .fixture
            .is_none()
            .then(|| self.root.data_dir.clone());
        let acts = self.acts(cx);
        let entity = cx.new(|_| SettingsView::new(data_dir.as_deref(), acts));
        self.show(entity, cx);
    }
}

/// The sheet over a dimmed window.
pub fn render(view: &ChatView, a: Appearance) -> Option<Div> {
    let sheet = view.sheet.as_ref()?;
    Some(
        div()
            .absolute()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(Ink::Shadow.color(a)))
            .child(
                div()
                    .w(px(680.0))
                    .h(px(560.0))
                    .rounded(px(14.0))
                    .border(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
                    .fill(Ink::Window, a)
                    .overflow_hidden()
                    .child(sheet.view.clone()),
            ),
    )
}
