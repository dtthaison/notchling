use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{
    div, list, prelude::*, px, AnyElement, Context, Entity, ListAlignment, ListOffset,
    ListSizingBehavior, ListState, Render, SharedString, Stateful, Subscription, WeakEntity,
    Window,
};
use notchling_kit::metric::{MENU_MAX_HEIGHT, PILL_HEIGHT, RADIUS_SM};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    hsla, icon, icon_button, label, menu_note, menu_panel, menu_section, scrollable_list,
    window_appearance, KitStyled, MenuItem, TextEditor, TextEditorEvent, ThumbRecord,
};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};

use super::ChatView;
use crate::chat::branch::{self, BranchModel, Branches, ChangedFile};
use crate::chat::branch_rows::{self, Row, Slot};
use crate::mac::gate;

pub const HEADER: f32 = 36.0;
const ROW: f32 = 28.0;
const PAD: f32 = 12.0;
const SEARCH: f32 = 28.0;
const OVERDRAW: f32 = 300.0;

pub struct ChangesPanel {
    chat: WeakEntity<ChatView>,
    model: Option<Arc<BranchModel>>,
    branches: Option<Arc<Branches>>,
    open: HashMap<String, Slot>,
    query: String,
    editor: Entity<TextEditor>,
    rows: Arc<Vec<Row>>,
    list: ListState,
    thumb: ThumbRecord,
    menu: bool,
    _editor_events: Subscription,
}

impl ChangesPanel {
    pub fn new(chat: WeakEntity<ChatView>, cx: &mut Context<Self>) -> ChangesPanel {
        let editor = cx.new(|cx| TextEditor::new("Filter files", 1, cx).single_line());
        let events = cx.subscribe(&editor, |this, editor, event: &TextEditorEvent, cx| {
            if matches!(event, TextEditorEvent::Changed) {
                let text = editor.read(cx).text().to_owned();
                if text != this.query {
                    this.query = text;
                    this.rebuild(true, cx);
                }
            }
        });
        ChangesPanel {
            chat,
            model: None,
            branches: None,
            open: HashMap::new(),
            query: String::new(),
            editor,
            rows: Arc::new(Vec::new()),
            list: ListState::new(0, ListAlignment::Top, px(OVERDRAW)),
            thumb: ThumbRecord::default(),
            menu: false,
            _editor_events: events,
        }
    }

    pub fn model(&self) -> Option<&Arc<BranchModel>> {
        self.model.as_ref()
    }

    pub fn set_model(
        &mut self,
        model: Option<Arc<BranchModel>>,
        cx: &mut Context<Self>,
    ) -> Vec<String> {
        if self.model == model {
            return Vec::new();
        }
        let fresh = match (&self.model, &model) {
            (Some(old), Some(new)) => old.base != new.base || old.branch != new.branch,
            _ => true,
        };
        let mut again = Vec::new();
        if fresh {
            self.open.clear();
        } else if let (Some(old), Some(new)) = (&self.model, &model) {
            let plan = branch_rows::refetch(&old.files, &new.files, &self.open);
            for path in &plan.gone {
                self.open.remove(path);
            }
            for path in &plan.again {
                self.open.insert(path.clone(), Slot::Loading);
            }
            again = plan.again;
        }
        self.model = model;
        self.rebuild(fresh, cx);
        again
    }

    pub fn toggle(&mut self, path: &str, cx: &mut Context<Self>) -> bool {
        let opened = if self.open.remove(path).is_some() {
            false
        } else {
            self.open.insert(path.to_owned(), Slot::Loading);
            true
        };
        self.rebuild(false, cx);
        opened
    }

    pub fn set_slot(&mut self, path: &str, base: &str, slot: Slot, cx: &mut Context<Self>) {
        let current = self.model.as_ref().is_some_and(|m| m.base == base);
        match self.open.get_mut(path) {
            Some(entry) if current && *entry != slot => *entry = slot,
            _ => return,
        }
        self.rebuild(false, cx);
    }

    pub fn set_branches(&mut self, branches: Option<Arc<Branches>>, cx: &mut Context<Self>) {
        if self.branches != branches {
            self.branches = branches;
            cx.notify();
        }
    }

    pub fn toggle_menu(&mut self, cx: &mut Context<Self>) -> bool {
        self.menu = !self.menu;
        cx.notify();
        self.menu
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu {
            self.menu = false;
            cx.notify();
        }
    }

    pub fn expand(&mut self, path: &str, slot: Slot, cx: &mut Context<Self>) {
        self.open.insert(path.to_owned(), slot);
        self.rebuild(false, cx);
    }

    fn rebuild(&mut self, reset: bool, cx: &mut Context<Self>) {
        let files: &[ChangedFile] = self.model.as_ref().map_or(&[], |m| &m.files);
        let shown = branch::visible(files, &self.query);
        let next = branch_rows::rows(files, &shown, &self.open);
        let saved = self.list.logical_scroll_top();
        if reset {
            self.list.reset(next.len());
            self.list.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.0),
            });
        } else {
            let (start, old, new) = branch_rows::splice(&self.rows, &next);
            self.list.splice(start..start + old, new);
            if saved.item_ix >= start {
                self.list.scroll_to(saved);
            }
        }
        self.rows = Arc::new(next);
        cx.notify();
    }
}

fn on_chat(
    chat: &WeakEntity<ChatView>,
    act: impl Fn(&mut ChatView, &mut Context<ChatView>) + 'static,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static {
    let chat = chat.clone();
    move |_, _, cx| {
        cx.stop_propagation();
        chat.update(cx, |view, cx| act(view, cx)).ok();
    }
}

fn counts(added: Option<u64>, removed: Option<u64>, a: Appearance) -> gpui::Div {
    let mut row = div().flex_shrink_0().flex().items_center().gap(px(6.0));
    if let Some(n) = added {
        row = row.child(label(&format!("+{n}"), TypeRole::Meta, Ink::Done, a));
    }
    if let Some(n) = removed {
        row = row.child(label(
            &format!("\u{2212}{n}"),
            TypeRole::Meta,
            Ink::Expired,
            a,
        ));
    }
    row
}

fn file_row(
    ix: usize,
    file: &ChangedFile,
    open: bool,
    chat: &WeakEntity<ChatView>,
    a: Appearance,
) -> Stateful<gpui::Div> {
    let path = file.path.clone();
    let toggle = {
        let path = path.clone();
        on_chat(chat, move |view, cx| view.toggle_change_file(&path, cx))
    };
    let from = file.from.clone();
    let reveal = {
        let path = path.clone();
        on_chat(chat, move |view, cx| {
            view.open_branch_diff(&path, from.as_deref(), cx)
        })
    };
    let chevron = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    div()
        .id(SharedString::from(format!("change-file-{ix}")))
        .w_full()
        .h(px(ROW))
        .px(px(PAD))
        .flex()
        .items_center()
        .gap(px(6.0))
        .cursor_pointer()
        .hover(move |style| style.bg(hsla(Ink::Raised.color(a).with_alpha(0.6))))
        .on_click(toggle)
        .child(
            div()
                .w(px(12.0))
                .flex_shrink_0()
                .child(icon(chevron, 9.0, Ink::TextFaint, a)),
        )
        .child(icon(Icon::File, 11.0, Ink::TextMuted, a).flex_shrink_0())
        .child(
            label(file.name(), TypeRole::Meta, file.name_ink(), a)
                .flex_shrink_0()
                .max_w(px(260.0))
                .elide(),
        )
        .child(
            label(file.folder(), TypeRole::Meta, Ink::TextFaint, a)
                .flex_1()
                .min_w(px(0.0))
                .elide(),
        )
        .child(counts(file.added, file.removed, a))
        .children(open.then(|| {
            gate::tip_text(
                icon_button(
                    SharedString::from(format!("change-open-{ix}")),
                    Icon::Expand,
                    IconSize::Row.points(),
                    ButtonKind::Ghost,
                    PartState::Rest,
                    a,
                ),
                "Open in File Panel".into(),
                a,
            )
            .flex_shrink_0()
            .on_click(reveal)
        }))
}

fn row_at(
    ix: usize,
    rows: &[Row],
    model: &BranchModel,
    open: &HashSet<String>,
    chat: &WeakEntity<ChatView>,
    a: Appearance,
) -> AnyElement {
    match rows.get(ix) {
        Some(Row::File(file_ix)) => match model.files.get(*file_ix) {
            Some(file) => file_row(ix, file, open.contains(&file.path), chat, a).into_any_element(),
            None => div().into_any_element(),
        },
        Some(Row::Chunk(ink, text)) => div()
            .w_full()
            .px(px(PAD))
            .fill(Ink::Sidebar, a)
            .kit_type(TypeRole::Mono)
            .ink(*ink, a)
            .child(SharedString::from(text.to_string()))
            .into_any_element(),
        Some(Row::Note(text, ink)) => div()
            .w_full()
            .px(px(PAD))
            .py(px(6.0))
            .fill(Ink::Sidebar, a)
            .child(label(text, TypeRole::Meta, *ink, a))
            .into_any_element(),
        None => div().into_any_element(),
    }
}

impl ChangesPanel {
    fn header(&self, model: Option<&BranchModel>, a: Appearance) -> gpui::Div {
        let chat = self.chat.clone();
        let mut title = div()
            .flex_1()
            .min_w(px(0.0))
            .flex()
            .items_center()
            .gap(px(6.0));
        if let Some(m) = model {
            title = title
                .child(
                    label(&m.branch, TypeRole::Label, Ink::Text, a)
                        .min_w(px(0.0))
                        .elide(),
                )
                .child(label("\u{2192}", TypeRole::Label, Ink::TextFaint, a).flex_shrink_0())
                .child(
                    div()
                        .id("changes-base")
                        .h(px(22.0))
                        .px(px(6.0))
                        .flex_shrink_0()
                        .rounded(px(RADIUS_SM))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .cursor_pointer()
                        .hover(move |style| style.bg(hsla(Ink::Raised.color(a))))
                        .on_click(on_chat(&chat, |view, cx| view.toggle_change_menu(cx)))
                        .child(label(&m.base, TypeRole::Label, Ink::AccentText, a))
                        .child(icon(Icon::ChevronDown, 9.0, Ink::TextMuted, a)),
                );
        } else {
            title = title.child(label("Changes", TypeRole::Label, Ink::Text, a));
        }
        let badge = model.filter(|m| m.is_large()).map(|_| {
            div()
                .h(px(PILL_HEIGHT))
                .px(px(8.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .rounded_full()
                .bg(hsla(Ink::Needs.color(a).with_alpha(0.16)))
                .child(label("Large diff", TypeRole::Label, Ink::AccentText, a))
        });
        let close = gate::tip_text(
            icon_button(
                "changes-close",
                Icon::Close,
                IconSize::Button.points(),
                ButtonKind::Ghost,
                PartState::Rest,
                a,
            ),
            "Close".into(),
            a,
        )
        .flex_shrink_0()
        .on_click(on_chat(&chat, |view, cx| view.close_changes(cx)));
        div()
            .h(px(HEADER))
            .flex_shrink_0()
            .pl(px(PAD))
            .pr(px(6.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(icon(Icon::Diff, 12.0, Ink::AccentText, a).flex_shrink_0())
            .child(title)
            .children(model.map(|m| counts(Some(m.added), Some(m.removed), a)))
            .children(badge)
            .child(close)
    }

    fn search(&self, a: Appearance, cx: &mut Context<Self>) -> Stateful<gpui::Div> {
        div()
            .id("changes-search")
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

    fn menu_layer(&self, model: &BranchModel, a: Appearance) -> AnyElement {
        let chat = self.chat.clone();
        let mut items = div()
            .id("changes-menu-items")
            .max_h(px(MENU_MAX_HEIGHT))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .child(menu_section("Compare Against", a));
        match &self.branches {
            None => items = items.child(menu_note("Reading the branches\u{2026}", a)),
            Some(list) => {
                let choices = list.choices();
                if choices.is_empty() {
                    items = items.child(menu_note("There are no other branches.", a));
                }
                for (i, name) in choices.into_iter().enumerate() {
                    let pick = name.clone();
                    let row = MenuItem::new(format!("changes-base-{i}"), name.clone())
                        .checked(name == model.base)
                        .build(a)
                        .on_click(on_chat(&chat, move |view, cx| {
                            view.set_change_base(pick.clone(), cx)
                        }));
                    items = items.child(row);
                }
            }
        }
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(
                div()
                    .id("changes-menu-away")
                    .absolute()
                    .size_full()
                    .occlude()
                    .on_click(on_chat(&chat, |view, cx| view.close_change_menu(cx))),
            )
            .child(
                div()
                    .absolute()
                    .top(px(HEADER - 2.0))
                    .left(px(PAD))
                    .child(menu_panel(a).child(items)),
            )
            .into_any_element()
    }
}

impl Render for ChangesPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let model = self.model.clone();
        let header = self.header(model.as_deref(), a);
        let mut column = div().size_full().flex().flex_col().child(header);
        let Some(model) = model else {
            return column
                .child(div().p(px(PAD)).child(label(
                    "Nothing to compare.",
                    TypeRole::Meta,
                    Ink::TextMuted,
                    a,
                )))
                .into_any_element();
        };
        column = column
            .child(
                label(&model.counts_line(), TypeRole::Meta, Ink::TextFaint, a)
                    .px(px(PAD))
                    .pb(px(6.0))
                    .flex_shrink_0(),
            )
            .child(self.search(a, cx));
        column = column.child(div().h(px(1.0)).flex_shrink_0().fill(Ink::Hairline, a));
        if self.rows.is_empty() {
            let text = if model.files.is_empty() {
                "No changes against the base."
            } else {
                "No files match."
            };
            column = column.child(div().p(px(PAD)).child(label(
                text,
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            )));
        } else {
            let (rows, shown, open, chat) = (
                self.rows.clone(),
                model.clone(),
                self.open.keys().cloned().collect::<HashSet<String>>(),
                self.chat.clone(),
            );
            let body = list(self.list.clone(), move |ix, window, _| {
                row_at(ix, &rows, &shown, &open, &chat, window_appearance(window))
            })
            .with_sizing_behavior(ListSizingBehavior::Auto)
            .flex_1()
            .min_h(px(0.0));
            column = column
                .child(scrollable_list("changes-list", body, &self.list).record_thumb(&self.thumb));
        }
        let menu = self.menu.then(|| self.menu_layer(&model, a));
        div()
            .relative()
            .size_full()
            .child(column)
            .children(menu)
            .into_any_element()
    }
}
