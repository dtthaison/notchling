//! The reply box: one rounded card (fill card, radius composer, hairline that turns accent while
//! focused, soft shadow). The text spans the card's full width on top; under it, inside the same
//! card, one row: the + menu at the left (Add files or photos ⌘U, Add folder, Slash commands) and
//! at the right the action button, a return arrow while there is text, a solid square (Stop)
//! while the session works, nothing otherwise (`chat::composer::action`). The text is the kit's
//! `TextEditor` (a caret, click and drag, the input method's marked text, ⌘Z); it grows from one
//! line to [`MAX_LINES`], then scrolls. Return sends (the editor's `Submit`, also while working,
//! when it queues), Shift or Option Return adds a line, Esc stops a working session
//! (`view::mod`). Files dropped anywhere on the middle column go in at the caret as paths
//! (`chat::drops`, the drop target is the column in `view::mod`); the card's edge turns accent
//! while one is over it. Under the card, one row: the model or options picker at the left (for a Claude
//! Code session the Model and Effort pickers, `chat::claude_picker`), the
//! status line (the error, "Sending…", what waits, or why the box is off), the context meter, the
//! session's account and the usage-limit ring
//! (`view::limit_ring`). Above it, the question or permission a Claude Code session waits on
//! (`view::prompts`), an Antigravity permission card when the daemon holds one for the row, and the picker's pop-up. Disabled while acting isn't on; the reason is the status
//! line, and only there.

use gpui::{
    deferred, div, img, point, prelude::*, px, relative, BoxShadow, Context, Div, ExternalPaths,
    KeyDownEvent, ObjectFit, PathPromptOptions, SharedString, Stateful, Window,
};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{CONTROL, RADIUS_COMPOSER, RADIUS_SM, SIDE_PADDING, SPINNER_MINI};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::typescale::sentence;
use notchling_kit::ui::{
    button_with, card as kit_card, hsla, icon, label, spinner as kit_spinner, KitPart, KitStyled,
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
use crate::tools::view::{agy_card, pick_row, pop_up as menu_card};

/// The text grows to this many lines, then scrolls (the Swift `ReplyMeasurer.maxLines`).
pub const MAX_LINES: usize = 8;
/// One line of body text in the box, in points: the system font at 13 lays out at 16.
pub const LINE_HEIGHT: f32 = 16.0;
/// The card's edge: 1 pt hairline at rest, 1.5 pt accent while focused (the spec's Composer).
const EDGE_REST: f32 = 1.0;
const EDGE_FOCUSED: f32 = 1.5;
/// Inside the card: 8 at the sides and above, 6 below the bottom row, plus the text's own 6
/// before it (14 in all) and above and below it.
const PAD_HORIZONTAL: f32 = 8.0;
const PAD_VERTICAL: f32 = 8.0;
const PAD_BOTTOM: f32 = 6.0;
const TEXT_PAD_LEADING: f32 = 6.0;
const TEXT_PAD_VERTICAL: f32 = 6.0;
/// The Stop square, in points.
const STOP_SQUARE: f32 = 12.0;
/// The + menu's width, and the gap between it and the button.
const MENU_WIDTH: f32 = 260.0;
const MENU_GAP: f32 = 6.0;

/// The card's padding at an edge width, so focusing (a thicker edge) moves nothing inside.
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

/// The Antigravity permission the daemon holds for the row, above the box.
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

/// The status line under the card: the error, what waits, or why the box is off.
/// Shown once, here, as a sentence; the box itself carries no tooltip for it.
fn status(
    view: &ChatView,
    control: crate::acting::Control,
    a: Appearance,
    cx: &Context<ChatView>,
) -> Option<Div> {
    let (text, ink) = if let Some(error) = &view.tools.error {
        // The daemon's "asking you something" refusal points at the card when one is up.
        (error_text(error, view.prompt.is_shown()), Ink::Needs)
    } else if view.tools.sending {
        // The message's own bubble says "Sending…"; the line here would only repeat it.
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

/// Which picker menu is open: the one clicked, or the one a render forces.
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

/// One row of a picker menu.
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

/// The More Models row: opens the rows under it.
fn more_row(open: bool, a: Appearance, cx: &mut Context<ChatView>) -> Stateful<Div> {
    div()
        .id("model-more")
        .w_full()
        .px(px(8.0))
        .py(px(3.0))
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_pointer()
        .hover(move |s| s.bg(hsla(Ink::Raised.color(a))))
        .child(div().flex_1().child(label(
            claude_picker::MORE_MODELS,
            TypeRole::Meta,
            Ink::Text,
            a,
        )))
        .child(icon(
            if open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            9.0,
            Ink::TextMuted,
            a,
        ))
        .on_click(cx.listener(|this, _, _, cx| {
            if let Some(picker) = &mut this.tools.picker {
                picker.more = !picker.more;
                cx.notify();
            }
        }))
}

/// A picker's menu, over its button and the layer that closes it on an outside click (both
/// `deferred`, as the + menu). The same for every tool (`chat::pickers`).
fn picker_menu(
    menu: &pickers::Menu,
    more: bool,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> impl IntoElement {
    let mut card = menu_card(a)
        .id("picker-menu")
        .p(px(4.0))
        .w(px(if menu.rows.iter().any(|r| !r.detail.is_empty()) {
            260.0
        } else {
            190.0
        }))
        .max_h(px(420.0))
        .overflow_y_scroll()
        .gap(px(0.0))
        .shadow_lg()
        .occlude();
    if menu.rows.is_empty() {
        if let Some(empty) = &menu.empty {
            card = card.child(label(empty, TypeRole::Meta, Ink::TextMuted, a).px(px(8.0)));
        }
    }
    for (i, row) in menu.rows.iter().enumerate() {
        card = card.child(menu_row(format!("{}-{i}", menu.id), &menu.id, row, a, cx));
    }
    if !menu.more.is_empty() {
        card = card
            .child(div().h(px(1.0)).w_full().bg(hsla(Ink::Hairline.color(a))))
            .child(more_row(more, a, cx));
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
        card = card.child(label(note, TypeRole::Meta, Ink::TextFaint, a).px(px(8.0)));
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

/// One picker button: its icon, the current choice and a chevron in the Meta type and the muted
/// ink, a ghost button; the same for every tool.
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
    // The acting reason when acting is off, else what the picker does (or why it can't yet).
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

/// The pickers under the card: Model, Effort and any other choice the session's tool offers.
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

/// What a render can force, since nothing types, works or clicks in a picture.
#[derive(Clone, Copy, Debug, Default)]
pub struct Preview {
    /// The box holds text (`NOTCHLING_RENDER_TYPED=1`).
    pub typed: bool,
    /// The session works, so Stop shows (`NOTCHLING_RENDER_WORKING=1`).
    pub working: bool,
    /// The + menu is open (`NOTCHLING_RENDER_ADD=1`).
    pub menu: bool,
    /// A Claude Code picker's menu is open (`NOTCHLING_RENDER_PICKER=model|effort`).
    pub picker: Option<Kind>,
}

/// What Chat keeps for the box's controls.
#[derive(Default)]
pub struct State {
    preview: Preview,
    /// The + menu is open.
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

    /// Esc closes the menu: whether there was one to close.
    pub fn close_menu(&mut self) -> bool {
        std::mem::take(&mut self.add_menu)
    }

    fn menu_open(&self) -> bool {
        self.add_menu || self.preview.menu
    }

    /// Whether the + menu is up, for the window's "something is open" (Esc then isn't Stop).
    pub fn is_open(&self) -> bool {
        self.menu_open()
    }
}

/// The text a typed render shows.
const SAMPLE: &str = "Can you check why the build fails on CI?\nIt passes here.";

impl ChatView {
    /// Applies a render's switches (the constructor's only).
    pub(super) fn preview_composer(&mut self, preview: Preview, cx: &mut Context<Self>) {
        self.composer = State::preview(preview);
        if preview.typed {
            self.tools.editor.update(cx, |e, cx| e.set_text(SAMPLE, cx));
        }
    }

    /// Opens a path picker (files or folders, several) and puts what is chosen into the box at the
    /// caret, as dropped files go.
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

/// The action button: a return arrow while there is text to send, a solid square while the session
/// works (Stop), nothing otherwise.
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
        // A message is going out: the small spinner where the arrow goes, as the Swift disc.
        Action::Sending => (
            "sending",
            kit_spinner("send-spin", SPINNER_MINI, Ink::TextMuted, a).into_any_element(),
            PartState::Disabled,
            "Send Button",
        ),
        Action::Stop { dim } => (
            "stop",
            // A drawn square, not an icon: the kit keeps filled icons for status.
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

/// One row of the + menu: the icon, the title and the key at the right.
fn add_row(
    index: usize,
    item: AddItem,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("add-item-{index}")))
        .px(px(10.0))
        .py(px(7.0))
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .gap(px(10.0))
        .cursor_pointer()
        .hover(move |s| s.bg(hsla(Ink::Raised.color(a))))
        .child(icon(item.icon, 13.0, Ink::TextMuted, a))
        .child(
            div()
                .flex_1()
                .child(label(item.title, TypeRole::Label, Ink::Text, a)),
        )
        .children(
            item.shortcut
                .map(|k| label(k, TypeRole::Meta, Ink::TextFaint, a)),
        )
        .on_click(cx.listener(move |this, _, window, cx| this.pick_add(item.add, window, cx)))
}

/// The + button at the card's bottom left, with its menu above it while open. The menu and the
/// layer that closes it on an outside click are drawn over everything (`deferred`).
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
        let card = kit_card(a)
            .id("add-menu")
            .w(px(MENU_WIDTH))
            .p(px(6.0))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .shadow_lg()
            .occlude()
            .children(rows)
            .part_at("Add Menu", Corner::TopTrailing, a, cx);
        deferred(
            div()
                .absolute()
                .bottom(relative(1.0))
                .left(px(0.0))
                .pb(px(MENU_GAP))
                .child(
                    // Anywhere else, a click closes it; the layer blocks the window under it.
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

/// How full the session's context is (the latest request's tokens against the model's window).
/// How tall an attached file's tile is over the text.
const THUMB_HEIGHT: f32 = 64.0;
/// A file that isn't a picture: its tile's width.
const FILE_TILE: f32 = 180.0;

/// The files attached to the reply (dropped, or picked with +), a tile each over the text: a
/// picture shows as itself, anything else as its name; a click opens it, × takes it off. Pictures
/// whose paths are typed in the text show too, without ×. A file that's gone shows nothing.
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
    // A borrowed window says so on hover.
    let chip = match h.context_note {
        Some(note) => gate::tip_text(chip, note.into(), a),
        None => chip,
    };
    Some(chip.part_at("Context Meter", Corner::Top, a, cx))
}

pub fn render(view: &ChatView, a: Appearance, window: &Window, cx: &mut Context<ChatView>) -> Div {
    let mut control = view.composer_control(cx);
    // A render forcing a state has no acting; the box is drawn as if it could take input.
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
    // The editor follows the row and the gate (both no-ops when nothing changed).
    let hint = placeholder(view);
    let typed = view.composer.preview.typed;
    view.tools.editor.update(cx, |editor, cx| {
        editor.set_enabled(control.enabled, cx);
        editor.set_placeholder(hint, cx);
        // A typed render keeps its sample through the loads that reset the box.
        if typed && editor.text() != SAMPLE {
            editor.set_text(SAMPLE, cx);
        }
    });
    // The text itself, the card's full width, wrapping there: one line to MAX_LINES, then it
    // scrolls (the editor's own height and scroll); the type and the ink come from here.
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
    // Under the text, inside the card: + at the left, send or stop at the right.
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
            // A click anywhere on the card puts the caret in the box; keys are the editor's.
            c.cursor_text()
                .on_click(cx.listener(|this, _, window, cx| {
                    window.focus(&this.tools.focus);
                    cx.notify();
                }))
                // ⌘U adds files, caught on the way down because the editor takes every key.
                .capture_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                    let (k, m) = (&e.keystroke, &e.keystroke.modifiers);
                    if is_add_files(&k.key, m.platform, m.shift, m.alt, m.control) {
                        this.pick_add(Add::Files, window, cx);
                        cx.stop_propagation();
                    }
                }))
                // Files from Finder: an accent edge while one is over the card. The drop itself
                // lands on the whole middle column (`ChatView::render`), the card included.
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
        // The session's context, then its account and limits: click the account to switch or add one.
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
        // What the session is doing, right over the box rather than at the end of the messages.
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
