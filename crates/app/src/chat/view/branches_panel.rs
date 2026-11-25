use notchling_locale as t;
use std::sync::Arc;

use gpui::{
    div, list, prelude::*, px, AnyElement, Context, Entity, ListAlignment, ListOffset,
    ListSizingBehavior, ListState, Render, SharedString, Stateful, Subscription, WeakEntity,
    Window,
};
use notchling_kit::metric::RADIUS_SM;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, hsla, icon, icon_button, label, scrollable_list, window_appearance, KitStyled,
    TextEditor, TextEditorEvent, ThumbRecord,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::changes_panel::HEADER;
use super::inspector::{MARK_BOX, ROW_LINE};
use super::repo_section::{pill, sub_line, working_dot};
use super::ChatView;
use crate::chat::branch_list::{
    panel, tab_label, Buttons, Group, PanelInput, PanelModel, PanelRow, PanelState, Right, Tab,
};
use crate::chat::branches_state::Shown;
use crate::mac::gate;

const PAD: f32 = 12.0;
const SEARCH: f32 = 28.0;
const OVERDRAW: f32 = 300.0;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Item {
    Heading(Group),
    Row(usize, usize),
    Note,
}

pub struct BranchesPanel {
    chat: WeakEntity<ChatView>,
    shown: Shown,
    model: Arc<PanelModel>,
    items: Arc<Vec<Item>>,
    editor: Entity<TextEditor>,
    list: ListState,
    thumb: ThumbRecord,
    _editor_events: Subscription,
}

fn items_of(model: &PanelModel) -> Vec<Item> {
    let mut items = Vec::new();
    for (group_ix, (group, rows)) in model.groups.iter().enumerate() {
        items.push(Item::Heading(*group));
        items.extend((0..rows.len()).map(|row_ix| Item::Row(group_ix, row_ix)));
    }
    if model.note.is_some() && model.state == PanelState::Ready {
        items.push(Item::Note);
    }
    items
}

impl BranchesPanel {
    pub fn new(chat: WeakEntity<ChatView>, cx: &mut Context<Self>) -> BranchesPanel {
        let editor = cx.new(|cx| TextEditor::new(t::chat::branches_filter(), 1, cx).single_line());
        let events = cx.subscribe(&editor, |this, editor, event: &TextEditorEvent, cx| {
            if matches!(event, TextEditorEvent::Changed) {
                let text = editor.read(cx).text().to_owned();
                if this.shown.set_query(&text) {
                    this.rebuild(true, cx);
                }
            }
        });
        let shown = Shown::default();
        let model = Arc::new(panel(shown.input(), shown.tab(), shown.query()));
        BranchesPanel {
            chat,
            items: Arc::new(items_of(&model)),
            model,
            shown,
            editor,
            list: ListState::new(0, ListAlignment::Top, px(OVERDRAW)),
            thumb: ThumbRecord::default(),
            _editor_events: events,
        }
    }

    pub fn set_input(&mut self, input: Arc<PanelInput>, cx: &mut Context<Self>) -> bool {
        if !self.shown.set_input(input) {
            return false;
        }
        self.rebuild(false, cx);
        true
    }

    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.shown.set_tab(tab) {
            self.rebuild(true, cx);
        }
    }

    pub fn open_row(&mut self, key: &str, cx: &mut Context<Self>) {
        self.shown.open_row(key);
        self.rebuild(false, cx);
    }

    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        self.shown.toggle(key);
        self.rebuild(false, cx);
    }

    pub fn reset_view(&mut self, cx: &mut Context<Self>) {
        self.shown.reset_view();
        self.clear_editor(cx);
        self.rebuild(true, cx);
    }

    pub fn switched(&mut self, cx: &mut Context<Self>) {
        self.shown.switched();
        self.clear_editor(cx);
        self.rebuild(true, cx);
    }

    fn clear_editor(&mut self, cx: &mut Context<Self>) {
        if !self.editor.read(cx).text().is_empty() {
            self.editor
                .update(cx, |editor, ecx| editor.set_text("", ecx));
        }
    }

    fn rebuild(&mut self, to_top: bool, cx: &mut Context<Self>) {
        let model = Arc::new(panel(
            self.shown.input(),
            self.shown.tab(),
            self.shown.query(),
        ));
        let items = items_of(&model);
        let saved = self.list.logical_scroll_top();
        self.list.reset(items.len());
        let at = if to_top {
            ListOffset {
                item_ix: 0,
                offset_in_item: px(0.0),
            }
        } else {
            saved
        };
        self.list.scroll_to(at);
        self.items = Arc::new(items);
        self.model = model;
        cx.notify();
    }
}

fn counts(right: Right, a: Appearance) -> gpui::Div {
    let mut row = div().flex_shrink_0().flex().items_center().gap(px(4.0));
    match right {
        Right::Lines(added, removed) => {
            row = row
                .child(
                    label(&format!("+{added}"), TypeRole::Meta, Ink::Done, a)
                        .line_height(px(ROW_LINE)),
                )
                .child(
                    label(
                        &format!("\u{2212}{removed}"),
                        TypeRole::Meta,
                        Ink::Expired,
                        a,
                    )
                    .line_height(px(ROW_LINE)),
                );
        }
        Right::Pending => {
            row = row.child(
                label("\u{2026}", TypeRole::Meta, Ink::TextFaint, a).line_height(px(ROW_LINE)),
            );
        }
        Right::Empty => {}
    }
    row
}

fn has_buttons(buttons: &Buttons) -> bool {
    buttons.changes || buttons.open_session || buttons.finder
}

fn button_line(ix: usize, row: &PanelRow, chat: &WeakEntity<ChatView>, a: Appearance) -> gpui::Div {
    let mut line = div()
        .pl(px(MARK_BOX + 7.0))
        .pt(px(6.0))
        .flex()
        .flex_wrap()
        .gap(px(6.0));
    if let (true, Some(target)) = (row.buttons.changes, row.target.clone()) {
        let chat = chat.clone();
        line = line.child(
            button(
                SharedString::from(format!("branch-changes-{ix}")),
                t::chat::branches_changes(),
                ButtonKind::Secondary,
                PartState::Rest,
                a,
            )
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                let target = target.clone();
                chat.update(cx, |view, cx| view.branch_changes(target, cx))
                    .ok();
            }),
        );
    }
    if let (true, Some(open)) = (row.buttons.open_session, row.open.clone()) {
        let chat = chat.clone();
        line = line.child(
            button(
                SharedString::from(format!("branch-session-{ix}")),
                t::chat::branches_open_session(),
                ButtonKind::Secondary,
                PartState::Rest,
                a,
            )
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                let open = open.clone();
                chat.update(cx, |view, cx| view.branch_open(open, cx)).ok();
            }),
        );
    }
    if let (true, Some(path)) = (row.buttons.finder, row.reveal.clone()) {
        let chat = chat.clone();
        line = line.child(
            button(
                SharedString::from(format!("branch-finder-{ix}")),
                t::chat::show_in_finder(),
                ButtonKind::Secondary,
                PartState::Rest,
                a,
            )
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                chat.update(cx, |view, cx| view.branch_reveal(&path, cx))
                    .ok();
            }),
        );
    }
    line
}

fn branch_row(
    ix: usize,
    row: &PanelRow,
    open: bool,
    chat: &WeakEntity<ChatView>,
    me: &WeakEntity<BranchesPanel>,
    a: Appearance,
) -> AnyElement {
    let shape = if row.cloud { Icon::Cloud } else { Icon::Branch };
    let dot = row.working.then(|| working_dot(a));
    let name = div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .children(dot)
        .child(
            label(&row.name, TypeRole::Meta, Ink::Text, a)
                .line_height(px(ROW_LINE))
                .min_w(px(0.0))
                .elide(),
        );
    let top = div()
        .w_full()
        .flex()
        .items_start()
        .gap(px(7.0))
        .child(
            div()
                .w(px(MARK_BOX))
                .h(px(ROW_LINE))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(icon(shape, 10.0, Ink::TextFaint, a)),
        )
        .child(name)
        .child(counts(row.right, a));
    let mut body = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(top)
        .child(div().pl(px(MARK_BOX + 7.0)).child(sub_line(&row.sub, a)));
    if open && has_buttons(&row.buttons) {
        body = body.child(button_line(ix, row, chat, a));
    }
    let key = row.key.clone();
    let mut shell: Stateful<gpui::Div> = div()
        .id(SharedString::from(format!("branch-row-{ix}")))
        .mx(px(PAD - 4.0))
        .px(px(4.0))
        .py(px(4.0))
        .rounded(px(RADIUS_SM));
    if row.current {
        shell = shell.bg(hsla(Ink::Raised.color(a).with_alpha(0.5)));
    }
    if has_buttons(&row.buttons) {
        let me = me.clone();
        shell = shell
            .cursor_pointer()
            .hover(move |style| style.bg(hsla(Ink::Raised.color(a).with_alpha(0.6))))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                let key = key.clone();
                me.update(cx, |this, cx| this.toggle(&key, cx)).ok();
            });
    }
    shell.child(body).into_any_element()
}

fn heading(group: Group, a: Appearance) -> AnyElement {
    let text = match group {
        Group::Worktrees => t::chat::branches_worktrees(),
        Group::NoWorktree => t::chat::branches_no_worktree(),
    };
    div()
        .w_full()
        .px(px(PAD))
        .pt(px(10.0))
        .pb(px(4.0))
        .child(label(text, TypeRole::Meta, Ink::TextFaint, a))
        .into_any_element()
}

fn note_item(text: &str, a: Appearance) -> AnyElement {
    div()
        .w_full()
        .px(px(PAD))
        .py(px(8.0))
        .child(label(text, TypeRole::Meta, Ink::TextMuted, a))
        .into_any_element()
}

impl BranchesPanel {
    fn header(&self, model: &PanelModel, a: Appearance) -> gpui::Div {
        let chat = self.chat.clone();
        let repo = (!model.repo.is_empty()).then(|| pill(&model.repo, a).min_w(px(0.0)));
        let close = gate::tip_text(
            icon_button(
                "branches-close",
                Icon::Close,
                ButtonKind::Ghost,
                PartState::Rest,
                a,
            ),
            t::chat::changes_close().into(),
            a,
        )
        .flex_shrink_0()
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            chat.update(cx, |view, cx| view.close_branches(cx)).ok();
        });
        div()
            .h(px(HEADER))
            .flex_shrink_0()
            .pl(px(PAD))
            .pr(px(6.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(icon(Icon::Branch, 12.0, Ink::AccentText, a).flex_shrink_0())
            .child(label(
                t::chat::branches_title(),
                TypeRole::Label,
                Ink::Text,
                a,
            ))
            .children(repo)
            .child(div().flex_1())
            .child(close)
    }

    fn tabs(&self, model: &PanelModel, a: Appearance, cx: &mut Context<Self>) -> gpui::Div {
        let mut row = div()
            .px(px(PAD))
            .pb(px(8.0))
            .flex_shrink_0()
            .flex()
            .flex_wrap()
            .gap(px(6.0));
        for (ix, (tab, count)) in model.tabs.iter().enumerate() {
            let tab = *tab;
            let state = if tab == self.shown.tab() {
                PartState::Hover
            } else {
                PartState::Rest
            };
            row = row.child(
                button(
                    SharedString::from(format!("branches-tab-{ix}")),
                    &tab_label(tab, *count),
                    ButtonKind::Ghost,
                    state,
                    a,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_tab(tab, cx))),
            );
        }
        row
    }

    fn search(&self, a: Appearance, cx: &mut Context<Self>) -> Stateful<gpui::Div> {
        div()
            .id("branches-search")
            .h(px(SEARCH))
            .mx(px(PAD))
            .mb(px(8.0))
            .px(px(8.0))
            .flex_shrink_0()
            .rounded(px(RADIUS_SM))
            .fill(Ink::Card, a)
            .border(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .flex()
            .items_center()
            .gap(px(6.0))
            .cursor_text()
            .kit_type(TypeRole::Meta)
            .ink(Ink::Text, a)
            .on_click(cx.listener(|this, _, window, cx| this.editor.read(cx).focus(window)))
            .child(icon(Icon::Search, 11.0, Ink::TextFaint, a).flex_shrink_0())
            .child(div().flex_1().min_w(px(0.0)).child(self.editor.clone()))
    }
}

impl Render for BranchesPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let model = self.model.clone();
        let mut column = div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.header(&model, a));
        if !model.fetched.is_empty() {
            column = column.child(
                label(&model.fetched, TypeRole::Meta, Ink::TextFaint, a)
                    .px(px(PAD))
                    .pb(px(6.0))
                    .flex_shrink_0(),
            );
        }
        column = column
            .child(self.tabs(&model, a, cx))
            .child(self.search(a, cx))
            .child(div().h(px(1.0)).flex_shrink_0().fill(Ink::Hairline, a));
        if model.state != PanelState::Ready {
            let why = match &model.state {
                PanelState::Failed(why) => Some(why.clone()),
                _ => None,
            };
            let note = model.note.clone().unwrap_or_default();
            return column
                .child(div().px(px(PAD)).pt(px(PAD)).child(label(
                    &note,
                    TypeRole::Meta,
                    Ink::TextMuted,
                    a,
                )))
                .children(why.map(|why| {
                    div().px(px(PAD)).pt(px(4.0)).child(label(
                        &why,
                        TypeRole::Meta,
                        Ink::TextFaint,
                        a,
                    ))
                }))
                .into_any_element();
        }
        let (items, shown, open, chat) = (
            self.items.clone(),
            self.model.clone(),
            self.shown.open().map(str::to_owned),
            self.chat.clone(),
        );
        let me = cx.entity().downgrade();
        let body = list(self.list.clone(), move |ix, window, _| {
            let a = window_appearance(window);
            match items.get(ix) {
                Some(Item::Heading(group)) => heading(*group, a),
                Some(Item::Row(group_ix, row_ix)) => {
                    match shown.groups.get(*group_ix).and_then(|g| g.1.get(*row_ix)) {
                        Some(row) => {
                            let is_open = open.as_deref() == Some(row.key.as_str());
                            branch_row(ix, row, is_open, &chat, &me, a)
                        }
                        None => div().into_any_element(),
                    }
                }
                Some(Item::Note) => note_item(shown.note.as_deref().unwrap_or_default(), a),
                None => div().into_any_element(),
            }
        })
        .with_sizing_behavior(ListSizingBehavior::Auto)
        .flex_1()
        .min_h(px(0.0));
        if self.items.is_empty() {
            column = column.child(note_item(model.note.as_deref().unwrap_or_default(), a));
        } else {
            column = column.child(
                scrollable_list("branches-list", body, &self.list).record_thumb(&self.thumb),
            );
        }
        column.into_any_element()
    }
}
