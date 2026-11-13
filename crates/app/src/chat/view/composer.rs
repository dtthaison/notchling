use gpui::{
    deferred, div, img, point, prelude::*, px, relative, BoxShadow, Context, Div, ExternalPaths,
    KeyDownEvent, ObjectFit, PathPromptOptions, SharedString, Stateful, Window,
};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{
    CONTROL, MENU_GAP, MENU_MAX_HEIGHT, RADIUS_COMPOSER, RADIUS_SM, SIDE_PADDING, SPINNER_MINI,
};
use notchling_kit::part::{button_look, ButtonKind, PartState};
use notchling_kit::typescale::sentence;
use notchling_kit::ui::{
    button_with, hsla, icon, label, menu_note, menu_panel, menu_panel_wide, menu_rule, spinner_in,
    KitPart, KitStyled, MenuItem,
};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};

use super::render_map::WindowRegion;
use super::ChatView;
use crate::chat::claude_picker::{self, Kind};
use crate::chat::composer::{action, add_menu, is_add_files, with_slash, Action, Add, AddItem};
use crate::chat::pickers;
use crate::chat::prompts::error_text;
use crate::mac::gate;
use crate::tools::requests::is_draft;
use crate::tools::view::{agy_card, pick_row};

pub const MAX_LINES: usize = 8;
pub const LINE_HEIGHT: f32 = 16.0;
const EDGE_REST: f32 = 1.0;
const EDGE_FOCUSED: f32 = 1.5;
const PAD_HORIZONTAL: f32 = 8.0;
const PAD_VERTICAL: f32 = 8.0;
const PAD_BOTTOM: f32 = 6.0;
const TEXT_PAD_LEADING: f32 = 6.0;
const TEXT_PAD_VERTICAL: f32 = 6.0;
const STOP_SQUARE: f32 = 12.0;

fn inset(base: f32, edge: f32) -> f32 {
    base - (edge - EDGE_REST)
}

fn placeholder(view: &ChatView) -> &'static str {
    match view.selected_session() {
        Some(s) if is_draft(&s.agent.id) => "Your first message starts the session",
        Some(s) => super::tool::of(s.tool).placeholder(),
        None => "Reply…",
    }
}

fn permission(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let held = view.agy_prompt()?;
    let gate = view.acting(cx).control();
    let permission = held.permission.clone();
    let card = agy_card(&permission, None, gate, a, cx, move |this, option, cx| {
        this.answer_agy(&held, option.to_owned(), cx);
    });
    Some(
        div()
            .mx(px(SIDE_PADDING))
            .mb(px(8.0))
            .p(px(14.0))
            .rounded(px(RADIUS_COMPOSER))
            .fill(Ink::Card, a)
            .border(px(1.0))
            .border_color(hsla(Ink::Accent.color(a).with_alpha(0.45)))
            .child(card)
            .part_at("Permission Card", Corner::TopTrailing, a, cx),
    )
}

fn status(
    view: &ChatView,
    control: crate::acting::Control,
    a: Appearance,
    cx: &Context<ChatView>,
) -> Option<Div> {
    let (text, ink) = if let Some(error) = &view.tools.error {
        (error_text(error, view.prompt.is_shown()), Ink::Needs)
    } else if view.tools.sending {
        return None;
    } else if let Some(n) = view
        .selected
        .as_deref()
        .map(|id| view.tools.live.waiting(id))
        .filter(|n| *n > 0)
    {
        (
            if n == 1 {
                "1 message waits for this turn to end.".to_owned()
            } else {
                format!("{n} messages wait for this turn to end.")
            },
            Ink::TextMuted,
        )
    } else {
        (control.tooltip?.to_owned(), Ink::TextFaint)
    };
    Some(
        label(&sentence(&text), TypeRole::Meta, ink, a)
            .w_full()
            .elide()
            .part_at("Composer Hint", Corner::Trailing, a, cx),
    )
}

fn open_menu(view: &ChatView) -> Option<(String, bool)> {
    match &view.tools.picker {
        Some(p) => Some((p.menu.clone(), p.more)),
        None => view
            .composer
            .preview
            .picker
            .map(|kind| (pickers::id_of(kind).to_owned(), false)),
    }
}

fn menu_row(
    id: String,
    menu: &str,
    row: &pickers::Row,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    let (menu, value) = (menu.to_owned(), row.value.clone());
    pick_row(id, &row.title, &row.detail, row.selected, a)
        .on_click(cx.listener(move |this, _, _, cx| this.pick(&menu, &value, cx)))
}

fn more_row(open: bool, cx: &mut Context<ChatView>, a: Appearance) -> Stateful<Div> {
    MenuItem::new("model-more", claude_picker::MORE_MODELS)
        .chevron(if open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        })
        .build(a)
        .on_click(cx.listener(|this, _, _, cx| {
            if let Some(picker) = &mut this.tools.picker {
                picker.more = !picker.more;
                cx.notify();
            }
        }))
}

fn picker_menu(
    menu: &pickers::Menu,
    more: bool,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> impl IntoElement {
    let wraps = menu.note.is_some()
        || menu.empty.is_some() && menu.rows.is_empty()
        || menu
            .rows
            .iter()
            .chain(&menu.more)
            .any(|r| !r.detail.is_empty());
    let mut card = if wraps {
        menu_panel_wide(a)
    } else {
        menu_panel(a)
    }
    .id("picker-menu")
    .max_h(px(MENU_MAX_HEIGHT))
    .overflow_y_scroll();
    if menu.rows.is_empty() {
        if let Some(empty) = &menu.empty {
            card = card.child(menu_note(empty, a));
        }
    }
    for (i, row) in menu.rows.iter().enumerate() {
        card = card.child(menu_row(format!("{}-{i}", menu.id), &menu.id, row, a, cx));
    }
    if !menu.more.is_empty() {
        card = card.child(menu_rule(a)).child(more_row(more, cx, a));
        if more {
            for (i, row) in menu.more.iter().enumerate() {
                card = card.child(menu_row(
                    format!("{}-more-{i}", menu.id),
                    &menu.id,
                    row,
                    a,
                    cx,
                ));
            }
        }
    }
    if let Some(note) = &menu.note {
        card = card.child(menu_note(note, a));
    }
    deferred(
        div()
            .absolute()
            .bottom(relative(1.0))
            .left(px(0.0))
            .pb(px(MENU_GAP))
            .child(
                div()
                    .id("picker-menu-click-away")
                    .absolute()
                    .top(px(-4000.0))
                    .left(px(-4000.0))
                    .size(px(8000.0))
                    .occlude()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tools.picker = None;
                        cx.notify();
                    })),
            )
            .child(card.part_at(&menu.menu_part(), Corner::TopTrailing, a, cx)),
    )
    .with_priority(1)
}

fn picker_button(
    view: &ChatView,
    menu: &pickers::Menu,
    control: crate::acting::Control,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
    let glyph = match menu.glyph {
        pickers::Glyph::Model => Icon::Cpu,
        pickers::Glyph::Effort => Icon::Gauge,
        pickers::Glyph::Other => Icon::Settings,
    };
    let open = open_menu(view);
    let is_open = open.as_ref().is_some_and(|(id, _)| *id == menu.id);
    let enabled = control.enabled && menu.enabled;
    let ink = if enabled {
        Ink::TextMuted
    } else {
        Ink::TextFaint
    };
    let content = div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(icon(glyph, 9.0, ink, a))
        .child(label(&menu.label, TypeRole::Meta, ink, a))
        .child(icon(Icon::ChevronDown, 7.0, ink, a));
    let b = button_with(
        SharedString::from(format!("picker-{}", menu.id)),
        content,
        if is_open {
            ButtonKind::Secondary
        } else {
            ButtonKind::Ghost
        },
        if enabled {
            PartState::Rest
        } else {
            PartState::Disabled
        },
        a,
    )
    .h(px(20.0))
    .px(px(6.0));
    let tip = control
        .tooltip
        .map(str::to_owned)
        .unwrap_or_else(|| menu.tip.clone());
    let b = gate::tip_text(b, SharedString::from(tip), a);
    let id = menu.id.clone();
    let b = b
        .when(enabled, |b| {
            b.on_click(cx.listener(move |this, _, _, cx| this.toggle_menu(&id, cx)))
        })
        .part_at(&menu.part(), Corner::Top, a, cx);
    let more = open.map(|(_, more)| more).unwrap_or(false);
    div()
        .relative()
        .flex_shrink_0()
        .child(b)
        .children(is_open.then(|| picker_menu(menu, more, a, cx)))
}

fn pickers_row(
    view: &ChatView,
    control: crate::acting::Control,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Option<Div> {
    let menus = view.menus(cx);
    if menus.is_empty() {
        return None;
    }
    let mut row = div().flex().items_center().gap(px(8.0));
    for menu in &menus {
        row = row.child(picker_button(view, menu, control, a, cx));
    }
    Some(row)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Preview {
    pub typed: bool,
    pub working: bool,
    pub menu: bool,
    pub picker: Option<Kind>,
}

#[derive(Default)]
pub struct State {
    preview: Preview,
    add_menu: bool,
}

impl Preview {
    fn any(self) -> bool {
        self.typed || self.working || self.menu || self.picker.is_some()
    }
}

impl State {
    pub fn preview(preview: Preview) -> State {
        State {
            preview,
            add_menu: false,
        }
    }

    pub fn close_menu(&mut self) -> bool {
        std::mem::take(&mut self.add_menu)
    }

    fn menu_open(&self) -> bool {
        self.add_menu || self.preview.menu
    }

    pub fn is_open(&self) -> bool {
        self.menu_open()
    }
}

const SAMPLE: &str = "Can you check why the build fails on CI?\nIt passes here.";

impl ChatView {
    pub(super) fn preview_composer(&mut self, preview: Preview, cx: &mut Context<Self>) {
        self.composer = State::preview(preview);
        if preview.typed {
            self.tools.editor.update(cx, |e, cx| e.set_text(SAMPLE, cx));
        }
    }

    fn pick_paths(&mut self, folders: bool, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: !folders,
            directories: folders,
            multiple: true,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                this.update_in(cx, |view, window, cx| view.drop_paths(&paths, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    fn pick_add(&mut self, add: Add, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.add_menu = false;
        match add {
            Add::Files => self.pick_paths(false, window, cx),
            Add::Folder => self.pick_paths(true, window, cx),
            Add::Slash => self.tools.editor.update(cx, |editor, cx| {
                let text = with_slash(editor.text());
                editor.set_text(&text, cx);
                editor.select(1, 1, cx);
                editor.focus(window);
            }),
        }
        cx.notify();
    }

    fn toggle_add_menu(&mut self, cx: &mut Context<Self>) {
        self.composer.add_menu = !self.composer.add_menu;
        cx.notify();
    }
}

fn sending_spinner(a: Appearance) -> Div {
    let ink = Ink::TextMuted.color(a);
    let faded = button_look(ButtonKind::Ghost, PartState::Disabled, a).opacity;
    spinner_in("send-spin", SPINNER_MINI, ink.with_alpha(ink.a * faded))
}

fn action_button(
    action: Action,
    enabled: bool,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Option<Stateful<Div>> {
    let (id, content, state, part): (&str, gpui::AnyElement, PartState, &str) = match action {
        Action::None => return None,
        Action::Send => (
            "send",
            icon(
                Icon::Return,
                IconSize::Button.points(),
                if enabled { Ink::Text } else { Ink::TextFaint },
                a,
            )
            .into_any_element(),
            if enabled {
                PartState::Rest
            } else {
                PartState::Disabled
            },
            "Send Button",
        ),
        Action::Sending => (
            "sending",
            sending_spinner(a).into_any_element(),
            PartState::Disabled,
            "Send Button",
        ),
        Action::Stop { dim } => (
            "stop",
            div()
                .size(px(STOP_SQUARE))
                .rounded(px(3.0))
                .fill(Ink::Text, a)
                .opacity(if dim { 0.4 } else { 1.0 })
                .into_any_element(),
            if dim {
                PartState::Disabled
            } else {
                PartState::Rest
            },
            "Stop Button",
        ),
    };
    let b = button_with(id, content, ButtonKind::Ghost, state, a)
        .w(px(CONTROL))
        .h(px(CONTROL))
        .px(px(0.0));
    let b = match action {
        Action::Send if enabled => b.on_click(cx.listener(|this, _, _, cx| this.send_text(cx))),
        Action::Stop { dim: false } => b.on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
        _ => b,
    };
    Some(b.part_at(part, Corner::Top, a, cx))
}

fn add_row(
    index: usize,
    item: AddItem,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    let row = MenuItem::new(format!("add-item-{index}"), item.title).icon(item.icon);
    let row = match item.shortcut {
        Some(key) => row.shortcut(key),
        None => row,
    };
    row.build(a)
        .on_click(cx.listener(move |this, _, window, cx| this.pick_add(item.add, window, cx)))
}

fn add_button(
    view: &ChatView,
    enabled: bool,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Option<Div> {
    let session = view.selected_session()?;
    let open = enabled && view.composer.menu_open();
    let kind = if open {
        ButtonKind::Secondary
    } else {
        ButtonKind::Ghost
    };
    let state = if enabled {
        PartState::Rest
    } else {
        PartState::Disabled
    };
    let ink = if enabled {
        Ink::TextMuted
    } else {
        Ink::TextFaint
    };
    let b = button_with("add", icon(Icon::Plus, 13.0, ink, a), kind, state, a)
        .w(px(CONTROL))
        .h(px(CONTROL))
        .px(px(0.0))
        .when(enabled, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.toggle_add_menu(cx)))
        })
        .part_at("Add Button", Corner::Top, a, cx);
    let menu = open.then(|| {
        let rows = add_menu(session.tool)
            .into_iter()
            .enumerate()
            .map(|(i, item)| add_row(i, item, a, cx))
            .collect::<Vec<_>>();
        let card = menu_panel(a).id("add-menu").children(rows).part_at(
            "Add Menu",
            Corner::TopTrailing,
            a,
            cx,
        );
        deferred(
            div()
                .absolute()
                .bottom(relative(1.0))
                .left(px(0.0))
                .pb(px(MENU_GAP))
                .child(
                    div()
                        .id("add-menu-click-away")
                        .absolute()
                        .top(px(-4000.0))
                        .left(px(-4000.0))
                        .size(px(8000.0))
                        .occlude()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.composer.add_menu = false;
                            cx.notify();
                        })),
                )
                .child(card),
        )
        .with_priority(1)
    });
    Some(div().relative().flex_shrink_0().child(b).children(menu))
}

const THUMB_HEIGHT: f32 = 64.0;
const FILE_TILE: f32 = 180.0;

fn thumbnails(view: &ChatView, id: &str, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let attached = view.attached(id);
    let mut tiles: Vec<(std::path::PathBuf, bool)> =
        attached.iter().map(|p| (p.clone(), true)).collect();
    for path in crate::chat::drops::image_words(view.tools.editor.read(cx).text()) {
        if !attached.contains(&path) {
            tiles.push((path, false));
        }
    }
    tiles.retain(|(p, _)| p.exists());
    if tiles.is_empty() {
        return None;
    }
    let row = div()
        .flex()
        .flex_wrap()
        .gap(px(6.0))
        .px(px(TEXT_PAD_LEADING))
        .pt(px(4.0))
        .children(
            tiles
                .into_iter()
                .map(|(path, removable)| tile(path, removable, a, cx)),
        );
    Some(row)
}

fn tile(
    path: std::path::PathBuf,
    removable: bool,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> impl IntoElement {
    let text = path.to_string_lossy().into_owned();
    let picture = crate::chat::drops::is_image(&path);
    let width = if picture {
        let (w, h) = super::markdown::image_shape(&text).unwrap_or((4, 3));
        (THUMB_HEIGHT * w as f32 / h.max(1) as f32).clamp(40.0, 200.0)
    } else {
        FILE_TILE
    };
    let to = text.clone();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| text.clone());
    let body = if picture {
        div()
            .size_full()
            .child(img(path.clone()).size_full().object_fit(ObjectFit::Cover))
    } else {
        div()
            .size_full()
            .px(px(10.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(icon(
                if path.is_dir() {
                    Icon::Folder
                } else {
                    Icon::File
                },
                16.0,
                Ink::TextMuted,
                a,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(label(&name, TypeRole::Meta, Ink::Text, a).elide()),
            )
    };
    let remove = removable.then(|| {
        div()
            .id(SharedString::from(format!("composer-unattach-{text}")))
            .absolute()
            .top(px(3.0))
            .right(px(3.0))
            .size(px(18.0))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .fill(Ink::Card, a)
            .border(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .cursor_pointer()
            .child(icon(Icon::Close, 10.0, Ink::TextMuted, a))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.remove_attached(&path, cx);
            }))
    });
    div()
        .id(SharedString::from(format!("composer-thumb-{text}")))
        .relative()
        .w(px(width))
        .h(if picture { px(THUMB_HEIGHT) } else { px(36.0) })
        .rounded(px(RADIUS_SM))
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .fill(Ink::Card, a)
        .overflow_hidden()
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            super::page_panel::follow_link(&to, window, cx)
        })
        .child(body)
        .children(remove)
        .part("Attachment", a, cx)
}

fn context_meter(
    view: &ChatView,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Option<Stateful<Div>> {
    let session = view.selected_session()?;
    let h = crate::chat::header::header(
        session,
        &view.conversation,
        &view.root.home,
        view.claude_catalog(),
    );
    let context = h.context?;
    let chip = super::header::chip(&format!("Context {context}"), a).id("context-meter");
    let chip = match h.context_note {
        Some(note) => gate::tip_text(chip, note.into(), a),
        None => chip,
    };
    Some(chip.part_at("Context Meter", Corner::Top, a, cx))
}

pub fn render(view: &ChatView, a: Appearance, window: &Window, cx: &mut Context<ChatView>) -> Div {
    let mut control = view.composer_control(cx);
    if view.composer.preview.any() {
        control.enabled = true;
    }
    let id = view.selected.clone().unwrap_or_default();
    let empty = !view.composer.preview.typed
        && view.tools.draft(&id).trim().is_empty()
        && view.attached(&id).is_empty();
    let focused = control.enabled && view.tools.focus.is_focused(window);
    let edge = if focused { EDGE_FOCUSED } else { EDGE_REST };
    let edge_color = if focused {
        Ink::Accent.color(a).with_alpha(0.6)
    } else {
        Ink::Hairline.color(a)
    };
    let hint = placeholder(view);
    let typed = view.composer.preview.typed;
    view.tools.editor.update(cx, |editor, cx| {
        editor.set_enabled(control.enabled, cx);
        editor.set_placeholder(hint, cx);
        if typed && editor.text() != SAMPLE {
            editor.set_text(SAMPLE, cx);
        }
    });
    let text_area = div()
        .id("composer-text")
        .w_full()
        .min_w(px(0.0))
        .px(px(TEXT_PAD_LEADING))
        .py(px(TEXT_PAD_VERTICAL))
        .kit_type(TypeRole::Body)
        .line_height(px(LINE_HEIGHT))
        .ink(Ink::Text, a)
        .child(view.tools.editor.clone())
        .part("Reply Box", a, cx);
    let working = view.selected_working() || view.composer.preview.working;
    let act = action(!empty, working, view.stopping(), view.tools.sending);
    let bottom = div()
        .h(px(CONTROL))
        .flex()
        .items_center()
        .justify_between()
        .child(add_button(view, control.enabled, a, cx).unwrap_or_else(div))
        .children(action_button(act, control.enabled, a, cx));
    let accent_edge = hsla(Ink::Accent.color(a));
    let card = div()
        .id("composer")
        .mx(px(SIDE_PADDING))
        .px(px(inset(PAD_HORIZONTAL, edge)))
        .pt(px(inset(PAD_VERTICAL, edge)))
        .pb(px(inset(PAD_BOTTOM, edge)))
        .flex()
        .flex_col()
        .rounded(px(RADIUS_COMPOSER))
        .fill(Ink::Card, a)
        .border(px(edge))
        .border_color(hsla(edge_color))
        .shadow(vec![BoxShadow {
            color: hsla(Ink::Shadow.color(a)),
            offset: point(px(0.0), px(6.0)),
            blur_radius: px(14.0),
            spread_radius: px(0.0),
        }])
        .children(thumbnails(view, &id, a, cx))
        .child(text_area)
        .child(bottom)
        .when(control.enabled, |c| {
            c.cursor_text()
                .on_click(cx.listener(|this, _, window, cx| {
                    window.focus(&this.tools.focus);
                    cx.notify();
                }))
                .capture_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                    let (k, m) = (&e.keystroke, &e.keystroke.modifiers);
                    if is_add_files(&k.key, m.platform, m.shift, m.alt, m.control) {
                        this.pick_add(Add::Files, window, cx);
                        cx.stop_propagation();
                    }
                }))
                .drag_over::<ExternalPaths>(move |style, _, _, _| style.border_color(accent_edge))
        })
        .part_at("Composer", Corner::BottomLeading, a, cx);
    let footer = div()
        .mx(px(SIDE_PADDING))
        .pt(px(6.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .children(pickers_row(view, control, a, cx))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .children(status(view, control, a, cx)),
        )
        .children(context_meter(view, a, cx))
        .children(
            super::accounts::account_control(view, a, cx)
                .map(|b| b.part_at("Account Menu", Corner::Top, a, cx)),
        )
        .children(super::limit_ring::ring(view, a, cx));
    div()
        .relative()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .pt(px(8.0))
        .pb(px(SIDE_PADDING))
        .children(super::prompts::card(view, a, window, cx))
        .children(permission(view, a, cx))
        .children(view.activity_line().map(|activity| {
            view.activity_drawn.set(Some(super::now_ms()));
            let stalled = view.stalled_prompt().is_some();
            let shell = crate::chat::activity::newest_unfinished_shell(&view.conversation);
            div()
                .mx(px(SIDE_PADDING))
                .pb(px(6.0))
                .child(super::conversation::activity_row(
                    &activity, stalled, shell, a, cx,
                ))
        }))
        .child(card)
        .child(footer)
        .children(super::slash_menu::render(view, a, cx))
        .window_region(a, cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_grows_to_eight_lines_of_body_text() {
        assert_eq!(MAX_LINES, 8);
        assert_eq!(LINE_HEIGHT, 16.0);
    }

    #[test]
    fn a_thicker_focused_edge_moves_nothing_inside() {
        assert_eq!(
            inset(PAD_HORIZONTAL, EDGE_REST) + EDGE_REST,
            PAD_HORIZONTAL + 1.0
        );
        assert_eq!(
            inset(PAD_HORIZONTAL, EDGE_FOCUSED) + EDGE_FOCUSED,
            PAD_HORIZONTAL + 1.0
        );
        assert_eq!(
            inset(PAD_VERTICAL, EDGE_FOCUSED) + EDGE_FOCUSED,
            inset(PAD_VERTICAL, EDGE_REST) + EDGE_REST
        );
    }
}
