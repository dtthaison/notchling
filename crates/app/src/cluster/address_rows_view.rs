use gpui::{div, prelude::*, px, Context, Div, Entity, SharedString, Subscription, Window};
use notchling_engine::machines::addresses::AddressKind;
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, label, TextEditor, TextEditorEvent};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::addresses_form::{cycle, kind_title, may_swap, swap_rows, RowDraft, RowMeta};
use super::view::{field_box, ClusterSheetView};

pub(super) struct RowEditor {
    pub editor: Entity<TextEditor>,
    pub meta: RowMeta,
    pub _events: Option<Subscription>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Rows {
    Add,
    Edit,
}

impl ClusterSheetView {
    fn rows_of(&mut self, at: Rows) -> Option<&mut Vec<RowEditor>> {
        match at {
            Rows::Add => self.add.sheet.as_mut().map(|s| &mut s.rows),
            Rows::Edit => self.edit.as_mut().map(|s| &mut s.rows),
        }
    }

    fn rows_ref(&self, at: Rows) -> Option<&Vec<RowEditor>> {
        match at {
            Rows::Add => self.add.sheet.as_ref().map(|s| &s.rows),
            Rows::Edit => self.edit.as_ref().map(|s| &s.rows),
        }
    }

    pub(super) fn rows_drafts(&self, at: Rows, cx: &Context<Self>) -> Vec<RowDraft> {
        self.rows_ref(at)
            .map(|rows| {
                rows.iter()
                    .map(|r| RowDraft {
                        text: r.editor.read(cx).text().to_owned(),
                        meta: r.meta.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn row_new(
        &mut self,
        at: Rows,
        text: &str,
        meta: RowMeta,
        cx: &mut Context<Self>,
    ) -> RowEditor {
        let editor =
            cx.new(|cx| TextEditor::new(t::cluster::address_placeholder(), 1, cx).single_line());
        if !text.is_empty() {
            editor.update(cx, |editor, cx| editor.set_text(text, cx));
        }
        let events = cx.subscribe(&editor, move |this, _, event: &TextEditorEvent, cx| {
            if *event == TextEditorEvent::Changed {
                this.rows_changed(at, cx);
            }
        });
        RowEditor {
            editor,
            meta,
            _events: Some(events),
        }
    }

    pub(super) fn rows_changed(&mut self, at: Rows, cx: &mut Context<Self>) {
        if at == Rows::Add {
            self.add_test_reset();
        }
        match at {
            Rows::Add => {
                if let Some(sheet) = self.add.sheet.as_mut() {
                    sheet.error = None;
                }
            }
            Rows::Edit => {
                if let Some(sheet) = self.edit.as_mut() {
                    sheet.error = None;
                }
            }
        }
        cx.notify();
    }

    pub(super) fn rows_add(&mut self, at: Rows, cx: &mut Context<Self>) {
        if at == Rows::Add {
            self.add_test_reset();
        }
        let row = self.row_new(at, "", RowMeta::default(), cx);
        if let Some(rows) = self.rows_of(at) {
            rows.push(row);
        }
        cx.notify();
    }

    pub(super) fn rows_remove(&mut self, at: Rows, i: usize, cx: &mut Context<Self>) {
        if at == Rows::Add {
            self.add_test_reset();
        }
        if let Some(rows) = self.rows_of(at) {
            if (i > 0 || at == Rows::Edit) && i < rows.len() {
                rows.remove(i);
            }
        }
        cx.notify();
    }

    pub(super) fn rows_swap(&mut self, at: Rows, i: usize, j: usize, cx: &mut Context<Self>) {
        if at == Rows::Add {
            self.add_test_reset();
        }
        let mut drafts = self.rows_drafts(at, cx);
        if !may_swap(drafts.len(), i, j, at == Rows::Add) || !swap_rows(&mut drafts, i, j) {
            return;
        }
        let Some(rows) = self.rows_of(at) else {
            return;
        };
        for (row, draft) in rows.iter_mut().zip(&drafts) {
            row.meta = draft.meta.clone();
            row.editor
                .update(cx, |editor, cx| editor.set_text(&draft.text, cx));
        }
        cx.notify();
    }

    pub(super) fn rows_cycle_kind(&mut self, at: Rows, i: usize, cx: &mut Context<Self>) {
        if at == Rows::Add {
            self.add_test_reset();
        }
        let drafts = self.rows_drafts(at, cx);
        let Some(draft) = drafts.get(i) else {
            return;
        };
        let next = draft.meta.kind_for(&draft.text).next();
        if let Some(row) = self.rows_of(at).and_then(|rows| rows.get_mut(i)) {
            row.meta.kind = Some(next);
            if !next.is_tailscale() {
                row.meta.tailnet = None;
            }
            if next != AddressKind::Jump {
                row.meta.via = None;
            }
        }
        cx.notify();
    }

    pub(super) fn rows_cycle_network(&mut self, at: Rows, i: usize, cx: &mut Context<Self>) {
        if at == Rows::Add {
            self.add_test_reset();
        }
        let mut options: Vec<Option<String>> = vec![None];
        options.extend(self.tailnets.iter().cloned().map(Some));
        if let Some(row) = self.rows_of(at).and_then(|rows| rows.get_mut(i)) {
            let current = row.meta.tailnet.clone();
            if let Some(tailnet) = &current {
                if !options.contains(&Some(tailnet.clone())) {
                    options.push(current.clone());
                }
            }
            row.meta.tailnet = cycle(&options, &current);
        }
        cx.notify();
    }

    pub(super) fn rows_cycle_via(&mut self, at: Rows, i: usize, cx: &mut Context<Self>) {
        if at == Rows::Add {
            self.add_test_reset();
        }
        let options: Vec<Option<String>> = self.via_names.iter().cloned().map(Some).collect();
        if let Some(row) = self.rows_of(at).and_then(|rows| rows.get_mut(i)) {
            let current = row.meta.via.clone();
            row.meta.via = cycle(&options, &current);
        }
        cx.notify();
    }

    pub(super) fn rows_view(
        &self,
        at: Rows,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        self.rows_view_with(at, false, None, window, a, cx)
    }

    pub(super) fn rows_view_with(
        &self,
        at: Rows,
        first_is_elsewhere: bool,
        decided_by_command: Option<&str>,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let tag = match at {
            Rows::Add => "add",
            Rows::Edit => "edit",
        };
        let Some(rows) = self.rows_ref(at) else {
            return div();
        };
        let last = rows.len().saturating_sub(1);
        let mut list = div().flex().flex_col().gap(px(SPACE[1]));
        for (i, row) in rows.iter().enumerate() {
            let text = row.editor.read(cx).text().to_owned();
            let kind = row.meta.kind_for(&text);
            let focused = row.editor.read(cx).is_focused(window);
            let mut controls = div().flex().items_center().gap(px(SPACE[0]));
            let locked = (first_is_elsewhere && i == 0)
                .then_some(decided_by_command)
                .flatten();
            if let Some(machine) = locked {
                controls = controls.child(label(
                    &t::cluster::add_jump_decides(machine),
                    TypeRole::Meta,
                    Ink::TextMuted,
                    a,
                ));
            } else {
                controls = controls.child(
                    button(
                        SharedString::from(format!("{tag}-row-kind-{i}")),
                        kind_title(kind),
                        ButtonKind::Secondary,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.rows_cycle_kind(at, i, cx))),
                );
                if kind.is_tailscale() {
                    let title = match row.meta.tailnet.as_deref() {
                        Some(name) => t::cluster::address_network(name),
                        None => t::cluster::address_network_any().to_owned(),
                    };
                    controls = controls.child(
                        button(
                            SharedString::from(format!("{tag}-row-network-{i}")),
                            &title,
                            ButtonKind::Secondary,
                            PartState::Rest,
                            a,
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.rows_cycle_network(at, i, cx)),
                        ),
                    );
                }
                if kind == AddressKind::Jump {
                    let title = match row.meta.via.as_deref() {
                        Some(name) => t::cluster::address_via(name),
                        None => t::cluster::address_via_none().to_owned(),
                    };
                    controls = controls.child(
                        button(
                            SharedString::from(format!("{tag}-row-via-{i}")),
                            &title,
                            ButtonKind::Secondary,
                            PartState::Rest,
                            a,
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.rows_cycle_via(at, i, cx)),
                        ),
                    );
                }
            }
            controls = controls.child(div().flex_1());
            if i > usize::from(first_is_elsewhere) {
                controls = controls.child(
                    button(
                        SharedString::from(format!("{tag}-row-up-{i}")),
                        t::cluster::address_up(),
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.rows_swap(at, i, i - 1, cx))),
                );
            }
            if i < last && !(first_is_elsewhere && i == 0) {
                controls = controls.child(
                    button(
                        SharedString::from(format!("{tag}-row-down-{i}")),
                        t::cluster::address_down(),
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.rows_swap(at, i, i + 1, cx))),
                );
            }
            if i > 0 || at == Rows::Edit {
                controls = controls.child(
                    button(
                        SharedString::from(format!("{tag}-row-remove-{i}")),
                        t::cluster::address_remove(),
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.rows_remove(at, i, cx))),
                );
            }
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(SPACE[0]))
                    .children(
                        (!(first_is_elsewhere && i == 0))
                            .then(|| field_box(&row.editor, focused, a)),
                    )
                    .child(controls),
            );
        }
        let add = button(
            SharedString::from(format!("{tag}-row-add")),
            t::cluster::address_add(),
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.rows_add(at, cx)));
        div()
            .flex()
            .flex_col()
            .gap(px(SPACE[1]))
            .child(label(
                t::cluster::address_list_label(),
                TypeRole::Label,
                Ink::TextFaint,
                a,
            ))
            .child(list)
            .child(div().flex().child(add))
            .child(label(
                t::cluster::address_list_note(),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            ))
    }
}
