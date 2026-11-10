//! The terminal in the Details Panel's place: the header's Terminal button opens it on a login
//! shell in the selected session's folder. Tabs along its top (+ opens another, × ends one), Expand
//! gives it the conversation's room as well, Minimize puts it back, and Close hides it with its
//! shells still running. Each shell is the engine's `Pty` with a `TerminalScreen` fed from it by a
//! thread of its own; the view redraws when the screen changed. The pure part is `chat::terminal`.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{
    canvas, div, prelude::*, px, rgb, AnyElement, Context, Div, FocusHandle, KeyDownEvent,
    MouseButton, SharedString, Window,
};
use notchling_engine::terminal::pty::Signal;
use notchling_engine::terminal::{Pty, PtyCommand, ReadOutcome, TerminalScreen};
use notchling_kit::labels::Corner;
use notchling_kit::metric::SIDEBAR_WIDTH;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{hsla, icon, icon_button, label, KitPart, KitStyled, MONO_FONT};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};

use super::ChatView;
use crate::chat::format::short_path;
use crate::chat::page_panel::panel_width;
use crate::chat::terminal::{bytes, grid, paste, rgb as cell_rgb, runs, Tabs, CELL, LINE};
use crate::mac::gate;

/// Set, a render draws the panel over a sample screen (`expanded` for the wide one).
pub const RENDER_ENV: &str = "NOTCHLING_RENDER_TERMINAL";

/// What the render's screen shows: a prompt in the colors a shell theme uses, and a command's output.
const SAMPLE: &[u8] = b"\x1b[38;5;141mnotchling\x1b[0m \x1b[90mgit:(\x1b[31mmain\x1b[90m)\x1b[0m \x1b[32m\xe2\x9d\xaf\x1b[0m ls\r\nCargo.toml  \x1b[1;34mcrates\x1b[0m  \x1b[1;34mdocs\x1b[0m  README.md\r\n\x1b[38;5;141mnotchling\x1b[0m \x1b[90mgit:(\x1b[31mmain\x1b[90m)\x1b[0m \x1b[32m\xe2\x9d\xaf\x1b[0m ";

/// The panel's width beside the conversation, before the window narrows it.
const WIDTH: f32 = 480.0;
/// The tab row's height, as the page panel's header.
const HEADER: f32 = 36.0;
/// The space around the grid.
const PAD_X: f32 = 8.0;
const PAD_Y: f32 = 6.0;

/// One tab's shell. Dropping it hangs the shell up; its reader thread then ends and the last
/// handle on the pty ends the child.
pub struct Shell {
    screen: Arc<Mutex<TerminalScreen>>,
    pty: Arc<Pty>,
    stop: Arc<AtomicBool>,
    ended: Arc<AtomicBool>,
    dirty: Arc<AtomicBool>,
    /// The folder, short (`~/go/src/app`) and as its last part for the tab.
    folder: String,
    name: String,
    /// The rows and columns drawn last, so a change of size resizes the pty once.
    grid: Rc<Cell<(u16, u16)>>,
    focus: FocusHandle,
}

impl Drop for Shell {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.pty.signal(Signal::Hup);
    }
}

/// The terminal's state in the Chat window.
#[derive(Default)]
pub struct State {
    pub tabs: Tabs<Shell>,
    pub open: bool,
    pub expanded: bool,
    /// The redraw loop, running while any shell is.
    ticking: bool,
}

impl State {
    /// Whether the panel takes the Details Panel's place (a page panel wins over it).
    pub fn shown(&self) -> bool {
        self.open && !self.tabs.is_empty()
    }
}

/// The user's login shell in `cwd`, read by a thread of its own.
fn start(cwd: &str, home: &Path, cx: &mut Context<ChatView>) -> Result<Shell, String> {
    let program = std::env::var("SHELL")
        .ok()
        .filter(|s| s.starts_with('/'))
        .unwrap_or_else(|| "/bin/zsh".to_owned());
    start_program(&program, &["-l"], cwd, home, cx)
}

fn start_program(
    program: &str,
    args: &[&str],
    cwd: &str,
    home: &Path,
    cx: &mut Context<ChatView>,
) -> Result<Shell, String> {
    let folder = if Path::new(cwd).is_dir() {
        cwd.to_owned()
    } else {
        home.to_string_lossy().into_owned()
    };
    let (rows, cols) = (24, 80);
    let mut command = PtyCommand::new(program);
    command
        .args(args)
        .cwd(&folder)
        .env("COLORTERM", "truecolor")
        .size(rows, cols);
    let pty = Arc::new(Pty::spawn(&command).map_err(|e| format!("Couldn't start {program}: {e}"))?);
    let screen = Arc::new(Mutex::new(TerminalScreen::new(
        usize::from(rows),
        usize::from(cols),
    )));
    let stop = Arc::new(AtomicBool::new(false));
    let ended = Arc::new(AtomicBool::new(false));
    let dirty = Arc::new(AtomicBool::new(true));
    {
        let (pty, screen, stop, ended, dirty) = (
            pty.clone(),
            screen.clone(),
            stop.clone(),
            ended.clone(),
            dirty.clone(),
        );
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 16 * 1024];
            while !stop.load(Ordering::Relaxed) {
                match pty.read_timeout(&mut buf, Duration::from_millis(200)) {
                    Ok(ReadOutcome::Data(n)) => {
                        let replies = {
                            let mut screen = screen.lock().unwrap_or_else(|e| e.into_inner());
                            screen.feed(&buf[..n]);
                            screen.take_replies()
                        };
                        if !replies.is_empty() {
                            let _ = pty.write_all_timeout(&replies, Duration::from_secs(1));
                        }
                        dirty.store(true, Ordering::Relaxed);
                    }
                    Ok(ReadOutcome::Timeout) => {}
                    Ok(ReadOutcome::Closed) | Err(_) => break,
                }
            }
            ended.store(true, Ordering::Relaxed);
            dirty.store(true, Ordering::Relaxed);
        });
    }
    let short = short_path(&folder, home);
    let name = Path::new(&folder)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| short.clone());
    Ok(Shell {
        screen,
        pty,
        stop,
        ended,
        dirty,
        folder: short,
        name,
        grid: Rc::new(Cell::new((rows, cols))),
        focus: cx.focus_handle(),
    })
}

impl ChatView {
    /// The render's panel: two tabs on `/bin/cat`, which prints nothing, over the sample screen.
    pub(super) fn render_terminal(&mut self, expanded: bool, cx: &mut Context<Self>) {
        let home = self.root.home.clone();
        let cwd = home.to_string_lossy().into_owned();
        for _ in 0..2 {
            if let Ok(shell) = start_program("/bin/cat", &[], &cwd, &home, cx) {
                shell
                    .screen
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .feed(SAMPLE);
                self.terminal.tabs.add(shell);
            }
        }
        self.terminal.open = true;
        self.terminal.expanded = expanded;
    }

    /// The header's Terminal button: shows the panel (opening a first shell), or hides it.
    pub(super) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal.shown() {
            self.terminal.open = false;
            cx.notify();
            return;
        }
        if self.terminal.tabs.is_empty() {
            self.add_terminal(window, cx);
        }
        self.terminal.open = !self.terminal.tabs.is_empty();
        if self.page.is_open() {
            self.close_page(cx);
        }
        self.focus_terminal(window);
        cx.notify();
    }

    /// + : another shell, in the selected session's folder.
    pub(super) fn add_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self
            .selected_session()
            .map(|s| s.agent.cwd.clone())
            .unwrap_or_default();
        let home = self.root.home.clone();
        match start(&cwd, &home, cx) {
            Ok(shell) => {
                self.terminal.tabs.add(shell);
                self.terminal.open = true;
                self.tools.error = None;
            }
            Err(message) => self.tools.error = Some(message),
        }
        self.tick_terminals(cx);
        self.focus_terminal(window);
        cx.notify();
    }

    fn close_terminal(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal.tabs.close(i);
        if self.terminal.tabs.is_empty() {
            self.terminal.open = false;
            self.terminal.expanded = false;
        } else {
            self.focus_terminal(window);
        }
        cx.notify();
    }

    fn focus_terminal(&self, window: &mut Window) {
        if let Some(shell) = self.terminal.tabs.active() {
            shell.focus.focus(window);
        }
    }

    /// Whether the keyboard is in a terminal, so the window's own keys (Esc stops a turn) keep out.
    pub(super) fn terminal_has_keys(&self, window: &Window) -> bool {
        self.terminal.shown()
            && self
                .terminal
                .tabs
                .active()
                .is_some_and(|s| s.focus.is_focused(window))
    }

    /// Redraws while the shells write, 30 times a second at most; stops once none is left.
    fn tick_terminals(&mut self, cx: &mut Context<Self>) {
        if self.terminal.ticking || self.terminal.tabs.is_empty() {
            return;
        }
        self.terminal.ticking = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(33))
                .await;
            let go_on = this
                .update(cx, |view, cx| {
                    let mut changed = false;
                    for (_, shell) in view.terminal.tabs.iter() {
                        changed |= shell.dirty.swap(false, Ordering::Relaxed);
                    }
                    if changed {
                        cx.notify();
                    }
                    let more = !view.terminal.tabs.is_empty();
                    view.terminal.ticking = more;
                    more
                })
                .unwrap_or(false);
            if !go_on {
                break;
            }
        })
        .detach();
    }
}

fn tool(
    id: impl Into<SharedString>,
    shape: Icon,
    tip: &'static str,
    a: Appearance,
) -> gpui::Stateful<Div> {
    gate::tip_text(
        icon_button(
            gpui::ElementId::Name(id.into()),
            shape,
            IconSize::Button.points(),
            ButtonKind::Ghost,
            PartState::Rest,
            a,
        ),
        tip.into(),
        a,
    )
    .flex_shrink_0()
}

/// The tab row: each tab's name and folder with its ×, then +, then Expand or Minimize and Close.
fn header(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let active = view.terminal.tabs.active_index();
    let mut tabs = div()
        .flex_1()
        .min_w(px(0.0))
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(4.0));
    for (i, (title, shell)) in view.terminal.tabs.iter().enumerate() {
        let close = tool(format!("term-close-{i}"), Icon::Close, "End this shell", a)
            .h(px(20.0))
            .w(px(20.0))
            .px(px(0.0))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.close_terminal(i, window, cx);
            }));
        let tab = div()
            .id(SharedString::from(format!("term-tab-{i}")))
            .flex_shrink()
            .min_w(px(0.0))
            .max_w(px(200.0))
            .h(px(26.0))
            .pl(px(10.0))
            .pr(px(3.0))
            .rounded(px(6.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .cursor_pointer()
            .when(i == active, |t| t.fill(Ink::Raised, a))
            .when(i != active, |t| {
                t.hover(move |s| s.bg(hsla(Ink::Raised.color(a).with_alpha(0.5))))
            })
            .child(label(&title, TypeRole::Label, Ink::Text, a).flex_shrink_0())
            .child(
                label(&shell.name, TypeRole::Label, Ink::TextFaint, a)
                    .flex_shrink()
                    .min_w(px(0.0))
                    .elide(),
            )
            .child(close)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.terminal.tabs.select(i);
                this.focus_terminal(window);
                cx.notify();
            }));
        let tab = gate::tip_text(tab, shell.folder.clone().into(), a);
        tabs = tabs.child(tab);
    }
    let add = tool("term-add", Icon::Plus, "New Terminal", a)
        .on_click(cx.listener(|this, _, window, cx| this.add_terminal(window, cx)));
    let (size_icon, size_tip) = if view.terminal.expanded {
        (Icon::Shrink, "Minimize: back beside the conversation")
    } else {
        (Icon::Expand, "Expand: over the conversation too")
    };
    let size =
        tool("term-size", size_icon, size_tip, a).on_click(cx.listener(|this, _, window, cx| {
            this.terminal.expanded = !this.terminal.expanded;
            this.focus_terminal(window);
            cx.notify();
        }));
    let hide = tool(
        "term-hide",
        Icon::Close,
        "Close the terminal (its shells keep running)",
        a,
    )
    .on_click(cx.listener(|this, _, _, cx| {
        this.terminal.open = false;
        this.terminal.expanded = false;
        cx.notify();
    }));
    div()
        .h(px(HEADER))
        .flex_shrink_0()
        .pl(px(8.0))
        .pr(px(6.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(icon(Icon::Terminal, 12.0, Ink::TextMuted, a))
        .child(tabs)
        .child(add)
        .child(size)
        .child(hide)
}

/// The active shell's screen: rows of styled runs, the cursor over them, and a canvas that sizes
/// the pty to the room it has.
fn screen(shell: &Shell, a: Appearance, window: &Window) -> AnyElement {
    let fg = Ink::Text.color(a);
    let bg = Ink::Window.color(a);
    let (cells, cursor, cursor_on, ended) = {
        let screen = shell.screen.lock().unwrap_or_else(|e| e.into_inner());
        (
            screen.cells().to_vec(),
            screen.cursor(),
            screen.cursor_visible(),
            shell.ended.load(Ordering::Relaxed),
        )
    };
    let rows = cells.iter().map(|row| {
        div()
            .h(px(LINE))
            .flex()
            .flex_shrink_0()
            .whitespace_nowrap()
            .overflow_hidden()
            .children(runs(row).into_iter().map(|(text, style)| {
                let (mut f, mut b) = (
                    style
                        .fg
                        .map(|c| rgb(cell_rgb(c)).into())
                        .unwrap_or(hsla(fg)),
                    style.bg.map(|c| rgb(cell_rgb(c)).into()),
                );
                if style.inverse {
                    let back = b.unwrap_or(hsla(bg));
                    b = Some(f);
                    f = back;
                }
                div()
                    .flex_shrink_0()
                    .text_color(f)
                    .when_some(b, |d, b| d.bg(b))
                    .when(style.bold, |d| d.font_weight(gpui::FontWeight::BOLD))
                    .when(style.italic, |d| d.italic())
                    .when(style.underline, |d| d.underline())
                    .when(style.dim, |d| d.opacity(0.6))
                    .child(text)
            }))
    });
    let focused = shell.focus.is_focused(window);
    let caret = (cursor_on && !ended).then(|| {
        let block = div()
            .absolute()
            .left(px(PAD_X + cursor.1 as f32 * CELL))
            .top(px(PAD_Y + cursor.0 as f32 * LINE))
            .w(px(CELL))
            .h(px(LINE));
        if focused {
            block.bg(hsla(Ink::Working.color(a))).opacity(0.85)
        } else {
            block.border_1().border_color(hsla(Ink::Working.color(a)))
        }
    });
    let (pty, screen, size) = (shell.pty.clone(), shell.screen.clone(), shell.grid.clone());
    let fit = canvas(
        |bounds, _, _| bounds,
        move |_, bounds, _, _| {
            let wanted = grid(
                f32::from(bounds.size.width) - 2.0 * PAD_X,
                f32::from(bounds.size.height) - 2.0 * PAD_Y,
            );
            if wanted != size.get() {
                size.set(wanted);
                screen
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .resize(usize::from(wanted.0), usize::from(wanted.1));
                let _ = pty.resize(wanted.0, wanted.1);
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    let (pty, screen) = (shell.pty.clone(), shell.screen.clone());
    let focus = shell.focus.clone();
    div()
        .id("terminal-screen")
        .track_focus(&shell.focus)
        .flex_1()
        .min_h(px(0.0))
        .relative()
        .overflow_hidden()
        .bg(hsla(bg))
        .px(px(PAD_X))
        .py(px(PAD_Y))
        .font_family(MONO_FONT)
        .text_size(px(crate::chat::terminal::FONT))
        .line_height(px(LINE))
        .cursor_text()
        .on_mouse_down(MouseButton::Left, move |_, window, _| focus.focus(window))
        .on_key_down(move |event: &KeyDownEvent, _, cx| {
            let keys = &event.keystroke;
            let m = &keys.modifiers;
            let (app_cursor, bracketed) = {
                let s = screen.lock().unwrap_or_else(|e| e.into_inner());
                (s.app_cursor_keys(), s.bracketed_paste())
            };
            let out = if m.platform {
                // ⌘V pastes; the other ⌘ keys stay the window's and the menu's.
                match keys.key.as_str() {
                    "v" => cx
                        .read_from_clipboard()
                        .and_then(|item| item.text())
                        .map(|text| paste(&text, bracketed)),
                    _ => return,
                }
            } else {
                bytes(
                    &keys.key,
                    keys.key_char.as_deref(),
                    m.control,
                    m.alt,
                    m.shift,
                    app_cursor,
                )
            };
            if let Some(out) = out {
                let _ = pty.write_all_timeout(out.as_bytes(), Duration::from_millis(500));
                cx.stop_propagation();
            }
        })
        .children(rows)
        .children(ended.then(|| {
            div()
                .pt(px(LINE / 2.0))
                .font_family(notchling_kit::ui::SYSTEM_FONT)
                .child(label("Process ended", TypeRole::Meta, Ink::TextFaint, a))
        }))
        .children(caret)
        .child(fit)
        .into_any_element()
}

/// The panel while it shows: beside the conversation, or across it when expanded.
pub fn render(
    view: &ChatView,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Option<AnyElement> {
    if !view.terminal.shown() || view.page.is_open() {
        return None;
    }
    let shell = view.terminal.tabs.active()?;
    let expanded = view.terminal.expanded;
    let sidebar = if view.sidebar_hidden {
        0.0
    } else {
        SIDEBAR_WIDTH
    };
    let available = f32::from(window.viewport_size().width) - sidebar;
    let panel = div()
        .h_full()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .fill(Ink::Window, a)
        .child(header(view, a, cx))
        .child(div().h(px(1.0)).flex_shrink_0().fill(Ink::Hairline, a))
        .child(screen(shell, a, window));
    let panel = if expanded {
        panel.flex_1()
    } else {
        panel
            .w(px(panel_width(WIDTH, available)))
            .flex_shrink_0()
            .border_l(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
    };
    Some(
        panel
            .part_at("Terminal Panel", Corner::TopTrailing, a, cx)
            .into_any_element(),
    )
}

/// The header's Terminal button, raised while the panel shows.
pub fn button(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> impl IntoElement {
    let kind = if view.terminal.shown() {
        ButtonKind::Secondary
    } else {
        ButtonKind::Ghost
    };
    gate::tip_text(
        icon_button(
            "terminal-button",
            Icon::Terminal,
            IconSize::Button.points(),
            kind,
            PartState::Rest,
            a,
        ),
        "Terminal: a shell in this session's folder".into(),
        a,
    )
    .on_click(cx.listener(|this, _, window, cx| this.toggle_terminal(window, cx)))
    .part_at("Terminal Button", Corner::BottomTrailing, a, cx)
}
