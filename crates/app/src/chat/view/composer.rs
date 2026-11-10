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
//! status line (the error, "Sending…", what waits, or why the box is off) and the usage-limit ring
//! (`view::limit_ring`). Above it, the question or permission a Claude Code session waits on
//! (`view::prompts`), an Antigravity permission card when the daemon holds one for the row, and the picker's pop-up. Disabled while acting isn't on; the reason is the status
//! line, and only there.

use gpui::{
    deferred, div, point, prelude::*, px, relative, BoxShadow, Context, Div, ExternalPaths,
    KeyDownEvent, PathPromptOptions, SharedString, Stateful, Window,
};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{CONTROL, RADIUS_COMPOSER, RADIUS_SM, SIDE_PADDING, SPINNER_SMALL};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::typescale::sentence;
use notchling_kit::ui::{
    button_with, card as kit_card, hsla, icon, label, spinner as kit_spinner, KitPart, KitStyled,
};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};

use super::tools::Picker;
use super::ChatView;
use crate::chat::claude_picker::{self, Kind};
use crate::chat::composer::{action, add_menu, is_add_files, with_slash, Action, Add, AddItem};
use crate::chat::data::Tool;
use crate::chat::prompts::error_text;
use crate::mac::gate;
use crate::tools::agy_options;
use crate::tools::requests::is_draft;
use crate::tools::view::{
    agy_card, agy_options_card, codex_picker_card, pick_row, pop_up as menu_card, PickerAction,
};

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
        Some(s) if s.tool == Tool::Codex => "Message Codex…",
        Some(s) if s.tool == Tool::Antigravity => "Message Antigravity…",
        _ => "Reply…",
    }
}

/// The picker button's label: the Codex pick, or Antigravity's current options.
fn picker_label(view: &ChatView) -> Option<String> {
    let session = view.selected_session()?;
    let id = &session.agent.id;
    match session.tool {
        Tool::Codex => Some(match &view.tools.picker {
            Some(Picker::Codex(p)) => p.label(),
            _ => {
                let pick = view.tools.picks.get(id).cloned().unwrap_or_default();
                crate::tools::codex_picker::CodexPicker::new(
                    Vec::new(),
                    pick,
                    &view.conversation.facts,
                )
                .label()
            }
        }),
        Tool::Antigravity => Some(agy_options::label(
            &view.tools.agy_config,
            !is_draft(id),
            view.tools.agy.as_ref().is_some_and(|a| a.loading),
        )),
        Tool::Claude => None,
    }
}

/// The pop-up over the composer, when open: its bottom edge on the composer's top.
fn pop_up(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let gate = view.acting(cx).control();
    let card = match &view.tools.picker {
        Some(Picker::Codex(picker)) => codex_picker_card(picker, a, cx, |this, action, cx| {
            if let Some(Picker::Codex(picker)) = &mut this.tools.picker {
                match action {
                    PickerAction::Model(slug) => picker.choose_model(&slug),
                    PickerAction::Effort(slug, effort) => picker.choose_effort(&slug, &effort),
                    PickerAction::Reset => picker.reset(),
                }
                let picked = picker.clone();
                this.keep_pick(&picked);
            }
            cx.notify();
        }),
        Some(Picker::Agy) => agy_options_card(
            &view.tools.agy_config,
            gate,
            a,
            cx,
            |this, option, value, cx| this.set_agy_option(option, value, cx),
        ),
        // Claude Code's two menus open from their own buttons (`claude_pickers`).
        Some(Picker::ClaudeModel { .. }) | Some(Picker::ClaudeEffort) | None => return None,
    };
    Some(
        div()
            .absolute()
            .bottom(relative(1.0))
            .left(px(SIDE_PADDING))
            .child(card)
            .part_at("Picker Menu", Corner::TopTrailing, a, cx),
    )
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

/// The status line under the card: the error, "Sending…", what waits, or why the box is off.
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
        ("Sending…".to_owned(), Ink::TextMuted)
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
            .line_clamp(2)
            .part_at("Composer Hint", Corner::Trailing, a, cx),
    )
}

/// The picker button under the card, for a Codex or Antigravity session.
fn picker(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<impl IntoElement> {
    let title = picker_label(view)?;
    let has_options = matches!(
        view.selected_session().map(|s| s.tool),
        Some(Tool::Codex) | Some(Tool::Antigravity)
    ) && !(view.tools.agy_config.is_empty()
        && view
            .selected_session()
            .is_some_and(|s| s.tool == Tool::Antigravity));
    let state = if has_options {
        PartState::Rest
    } else {
        PartState::Disabled
    };
    let kind = if view.tools.picker.is_some() {
        ButtonKind::Secondary
    } else {
        ButtonKind::Ghost
    };
    let tip = view
        .selected_session()
        .filter(|s| s.tool == Tool::Antigravity)
        .map(|s| agy_options::tip(!is_draft(&s.agent.id)));
    // Meta type in the muted ink with a chevron, as the Swift pickers under its composer.
    let ink = if has_options {
        Ink::TextMuted
    } else {
        Ink::TextFaint
    };
    // The Swift names: Codex's picker and Antigravity's options are two parts there.
    let part = match view.selected_session().map(|s| s.tool) {
        Some(Tool::Antigravity) => "Antigravity Options",
        _ => "Codex Model Picker",
    };
    let title = div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(label(&title, TypeRole::Meta, ink, a))
        .child(icon(Icon::ChevronDown, 7.0, ink, a));
    let b = button_with("picker", title, kind, state, a)
        .h(px(20.0))
        .px(px(6.0));
    let b = gate::tip(
        b,
        crate::acting::Control {
            enabled: true,
            tooltip: tip,
        },
        a,
    );
    Some(
        b.when(has_options, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.toggle_picker(cx)))
        })
        .part_at(part, Corner::Top, a, cx),
    )
}

/// Which Claude Code menu is open: the one clicked, or the one a render forces.
fn claude_open(view: &ChatView) -> Option<Kind> {
    match &view.tools.picker {
        Some(Picker::ClaudeModel { .. }) => Some(Kind::Model),
        Some(Picker::ClaudeEffort) => Some(Kind::Effort),
        _ => view.composer.preview.picker,
    }
}

/// One row of a Claude Code menu.
fn claude_row(
    id: String,
    row: &claude_picker::Row,
    kind: Kind,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    let value = row.value;
    pick_row(id, &row.title, "", row.selected, a)
        .on_click(cx.listener(move |this, _, _, cx| this.pick_claude(kind, value, cx)))
}

/// The More Models row: opens the older models under it.
fn more_row(open: bool, a: Appearance, cx: &mut Context<ChatView>) -> Stateful<Div> {
    div()
        .id("model-more")
        .w_full()
        .px(px(8.0))
        .py(px(5.0))
        .rounded(px(RADIUS_SM))
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_pointer()
        .hover(move |s| s.bg(hsla(Ink::Raised.color(a))))
        .child(div().w(px(14.0)).flex_shrink_0())
        .child(div().flex_1().child(label(
            claude_picker::MORE_MODELS,
            TypeRole::Label,
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
            if let Some(Picker::ClaudeModel { more }) = &mut this.tools.picker {
                *more = !*more;
                cx.notify();
            }
        }))
}

/// The Model or Effort menu, over its button and the layer that closes it on an outside click
/// (both `deferred`, as the + menu).
fn claude_menu(
    view: &ChatView,
    kind: Kind,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> impl IntoElement {
    let more = matches!(view.tools.picker, Some(Picker::ClaudeModel { more: true }));
    let mut card = menu_card(a)
        .id("claude-menu")
        .w(px(200.0))
        .gap(px(2.0))
        .shadow_lg()
        .occlude();
    match kind {
        Kind::Model => {
            let current = view.claude_model();
            for (i, row) in claude_picker::models(current).iter().enumerate() {
                card = card.child(claude_row(format!("model-{i}"), row, kind, a, cx));
            }
            card = card
                .child(div().h(px(1.0)).w_full().bg(hsla(Ink::Hairline.color(a))))
                .child(more_row(more, a, cx));
            if more {
                for (i, row) in claude_picker::older_models(current).iter().enumerate() {
                    card = card.child(claude_row(format!("older-{i}"), row, kind, a, cx));
                }
            }
        }
        Kind::Effort => {
            for (i, row) in claude_picker::efforts(view.claude_effort())
                .iter()
                .enumerate()
            {
                card = card.child(claude_row(format!("effort-{i}"), row, kind, a, cx));
            }
        }
    }
    let name = match kind {
        Kind::Model => "Model Menu",
        Kind::Effort => "Effort Menu",
    };
    deferred(
        div()
            .absolute()
            .bottom(relative(1.0))
            .left(px(0.0))
            .pb(px(MENU_GAP))
            .child(
                div()
                    .id("claude-menu-click-away")
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
            .child(card.part_at(name, Corner::TopTrailing, a, cx)),
    )
    .with_priority(1)
}

/// One Claude Code picker button: the cpu or gauge icon, the label and a chevron in the Meta
/// type and the muted ink, a ghost button (the Swift `ModelPicker` / `EffortPicker`).
fn claude_picker(
    view: &ChatView,
    kind: Kind,
    control: crate::acting::Control,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
    let (glyph, title, part, tip, id) = match kind {
        Kind::Model => (
            Icon::Cpu,
            claude_picker::model_label(view.claude_model()),
            "Model Picker",
            claude_picker::MODEL_TIP,
            "model-picker",
        ),
        Kind::Effort => (
            Icon::Gauge,
            claude_picker::effort_label(view.claude_effort()),
            "Effort Picker",
            claude_picker::EFFORT_TIP,
            "effort-picker",
        ),
    };
    let open = claude_open(view) == Some(kind);
    let enabled = control.enabled;
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
        .child(label(&title, TypeRole::Meta, ink, a))
        .child(icon(Icon::ChevronDown, 7.0, ink, a));
    let b = button_with(
        id,
        content,
        if open {
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
    // The acting reason when off, else what the picker does.
    let b = gate::tip(
        b,
        crate::acting::Control {
            enabled: true,
            tooltip: Some(control.tooltip.unwrap_or(tip)),
        },
        a,
    );
    let b = b
        .when(enabled, |b| {
            b.on_click(cx.listener(move |this, _, _, cx| this.toggle_claude_picker(kind, cx)))
        })
        .part_at(part, Corner::Top, a, cx);
    div()
        .relative()
        .flex_shrink_0()
        .child(b)
        .children(open.then(|| claude_menu(view, kind, a, cx)))
}

/// The Model and Effort pickers under the card, for a Claude Code session.
fn claude_pickers(
    view: &ChatView,
    control: crate::acting::Control,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Option<Div> {
    if view.selected_session()?.tool != Tool::Claude {
        return None;
    }
    Some(
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(claude_picker(view, Kind::Model, control, a, cx))
            .child(claude_picker(view, Kind::Effort, control, a, cx)),
    )
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
            kit_spinner("send-spin", SPINNER_SMALL, Ink::TextMuted, a).into_any_element(),
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

pub fn render(view: &ChatView, a: Appearance, window: &Window, cx: &mut Context<ChatView>) -> Div {
    let mut control = view.composer_control(cx);
    // A render forcing a state has no acting; the box is drawn as if it could take input.
    if view.composer.preview.any() {
        control.enabled = true;
    }
    let id = view.selected.clone().unwrap_or_default();
    let empty = !view.composer.preview.typed && view.tools.draft(&id).trim().is_empty();
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
        .children(picker(view, a, cx))
        .children(claude_pickers(view, control, a, cx))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .children(status(view, control, a, cx)),
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
        .child(card)
        .child(footer)
        .children(pop_up(view, a, cx))
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
