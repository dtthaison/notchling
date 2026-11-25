use std::time::Duration;

use gpui::Context;
use serde_json::Value;

use super::SettingsView;
use crate::search::panel::AppSearch;
use crate::sheets::semantic::{self, Step};
use crate::sheets::settings::{Field, Row, Section};

const POLL: Duration = Duration::from_millis(500);

impl SettingsView {
    pub fn with_semantic(mut self, status: Value) -> SettingsView {
        self.semantic = Some(status);
        self
    }

    pub(super) fn semantic_status(&self, cx: &Context<Self>) -> Value {
        match &self.semantic {
            Some(status) => status.clone(),
            None => cx
                .try_global::<AppSearch>()
                .map(|g| g.0.embed_status())
                .unwrap_or(Value::Null),
        }
    }

    pub(super) fn show_semantic(&self, sections: &mut [Section], cx: &Context<Self>) {
        let status = self.semantic_status(cx);
        for row in sections.iter_mut().flat_map(|s| s.rows.iter_mut()) {
            if let Field::Semantic(shown) = &mut row.field {
                *shown = semantic::model(shown.on, shown.paused, &status);
            }
        }
    }

    pub fn watch_semantic(&mut self, cx: &mut Context<Self>) {
        if self.semantic.is_some() {
            return;
        }
        let mut seen = Value::Null;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL).await;
            let alive = this.update(cx, |view, cx| {
                let now = view.semantic_status(cx);
                if now != seen {
                    seen = now;
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
    }

    pub(in crate::sheets) fn semantic_act(
        &mut self,
        row: &Row,
        option: &str,
        cx: &mut Context<Self>,
    ) {
        let Field::Semantic(shown) = &row.field else {
            return;
        };
        let Some(action) = semantic::action(shown, option) else {
            self.apply(row, option, cx);
            return;
        };
        let Some(search) = cx.try_global::<AppSearch>().map(|g| g.0.clone()) else {
            return;
        };
        let Step { op, model, remove } = semantic::step(&action);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    search.embed_model_op(op, &model)?;
                    if remove {
                        search.refresh()?;
                    }
                    Ok::<(), String>(())
                })
                .await;
            this.update(cx, |view, cx| {
                view.error = result.err();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
