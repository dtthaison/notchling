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
    canvas, div, prelude::*, px, rgb, AnyElement, ClipboardItem, Context, DispatchPhase, Div,
    FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, ScrollWheelEvent, SharedString, Window,
};
use notchling_engine::terminal::pty::Signal;
use notchling_engine::terminal::{Pty, PtyCommand, ReadOutcome, TermCell, TerminalScreen};
use notchling_kit::labels::Corner;
use notchling_kit::metric::SIDEBAR_WIDTH;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{hsla, icon, icon_button, label, KitPart, KitStyled, MONO_FONT};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};

use super::sidebar::HEADER_BUTTON;
use super::ChatView;
use crate::chat::format::short_path;
use crate::chat::page_panel::panel_width;
use crate::chat::terminal::{
    boundary_at, bytes, grid, paste, rgb as cell_rgb, runs, selected_text, shown_rows, word_at,
    Selection, Tabs, CELL, LINE,
};
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
    /// Text picked with the mouse, for ⌘C; a key typed into the shell clears it.
    selection: Rc<Cell<Option<Selection>>>,
    /// A drag that started on the grid is going on.
    dragging: Rc<Cell<bool>>,
    /// The grid's top left in the window, from the last paint, to turn mouse points into cells.
    origin: Rc<Cell<Point<Pixels>>>,
    /// How many rows the view is scrolled back into the scrollback; typing goes back to 0.
    back: Rc<Cell<usize>>,
    /// Wheel movement not yet a whole row, in points.
    wheel: Rc<Cell<f32>>,
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
    /// The active tab takes the keyboard at the next draw (set where no window is at hand).
    pub focus_next: bool,
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
    start_program(&program, &["-l"], &[], cwd, home, cx)
}

fn start_program(
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
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
    for (key, value) in env {
        command.env(key, value);
    }
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
        selection: Rc::new(Cell::new(None)),
        dragging: Rc::new(Cell::new(false)),
        origin: Rc::new(Cell::new(Point::default())),
        back: Rc::new(Cell::new(0)),
        wheel: Rc::new(Cell::new(0.0)),
    })
}

impl ChatView {
    /// The render's panel: two tabs on `/bin/cat`, which prints nothing, over the sample screen.
    pub(super) fn render_terminal(&mut self, expanded: bool, cx: &mut Context<Self>) {
        let home = self.root.home.clone();
        let cwd = home.to_string_lossy().into_owned();
        for _ in 0..2 {
            if let Ok(shell) = start_program("/bin/cat", &[], &[], &cwd, &home, cx) {
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

    /// A menu command (`/mcp`, `/plugin`…) for a background job: a tab on `claude attach <job>`,
    /// with the job's lane, that types the command once the session has drawn. The menu is then
    /// the user's, with the keyboard, as in Claude Code's own terminal; closing the tab detaches.
    pub(super) fn open_menu_command(
        &mut self,
        job: &str,
        config_dir: Option<&str>,
        cwd: &str,
        command: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let home = self.root.home.clone();
        let claude = notchling_engine::control::launcher::claude_path(&home);
        let claude = claude.to_string_lossy().into_owned();
        let env: Vec<(&str, &str)> = config_dir
            .map(|dir| vec![("CLAUDE_CONFIG_DIR", dir)])
            .unwrap_or_default();
        let mut shell = start_program(&claude, &["attach", job], &env, cwd, &home, cx)?;
        shell.name = command.trim().to_owned();
        let pty = shell.pty.clone();
        let stop = shell.stop.clone();
        let keys = crate::chat::terminal::menu_keys(command);
        std::thread::spawn(move || {
            for (pause, text) in keys {
                std::thread::sleep(Duration::from_millis(pause));
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                if pty
                    .write_all_timeout(text.as_bytes(), Duration::from_secs(1))
                    .is_err()
                {
                    return;
                }
            }
        });
        if self.page.is_open() {
            self.close_page(cx);
        }
        self.terminal.tabs.add(shell);
        self.terminal.open = true;
        self.terminal.focus_next = true;
        self.tick_terminals(cx);
        cx.notify();
        Ok(())
    }

    /// A background shell's output in a terminal tab: `tail -f` on the file Claude Code writes,
    /// from its first line and live while the command runs. Closing the tab stops only the tail.
    pub(super) fn open_shell_output(&mut self, name: &str, path: &str, cx: &mut Context<Self>) {
        let home = self.root.home.clone();
        let cwd = self
            .selected_session()
            .map(|s| s.agent.cwd.clone())
            .unwrap_or_default();
        match start_program(
            "/usr/bin/tail",
            &["-n", "+1", "-f", path],
            &[],
            &cwd,
            &home,
            cx,
        ) {
            Ok(mut shell) => {
                shell.name = name.trim().to_owned();
                if self.page.is_open() {
                    self.close_page(cx);
                }
                self.terminal.tabs.add(shell);
                self.terminal.open = true;
                self.terminal.focus_next = true;
                self.tools.error = None;
                self.tick_terminals(cx);
            }
            Err(message) => self.tools.error = Some(message),
        }
        cx.notify();
    }

    /// The keyboard to the active tab, when a call without a window asked for it.
    pub(super) fn take_terminal_focus(&mut self, window: &mut Window) {
        if std::mem::take(&mut self.terminal.focus_next) {
            self.focus_terminal(window);
        }
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
        // They share the Details Panel's place.
        if self.simulator.shown() {
            self.toggle_simulator(cx);
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
        )
        // Square, as the Chat header's buttons.
        .flex_shrink_0()
        .size(px(HEADER_BUTTON))
        .px(px(0.0)),
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
        // A tab is its name only, at most 120 pt wide. Its × sits over the end of the name, on a
        // patch of the tab's own fill so the name runs under it: always on the active tab, on the
        // others while the pointer is over them.
        let group = SharedString::from(format!("term-tab-{i}"));
        let is_active = i == active;
        let button = tool(format!("term-close-{i}"), Icon::Close, "End this shell", a)
            .h(px(20.0))
            .w(px(20.0))
            .hover(move |s| s.bg(hsla(Ink::Text.color(a).with_alpha(0.14))))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.close_terminal(i, window, cx);
            }));
        // Rounded as the tab: GPUI clips children to the tab's rectangle, not its corners.
        let patch = div()
            .h_full()
            .rounded_r(px(6.0))
            .pl(px(2.0))
            .pr(px(3.0))
            .flex()
            .items_center()
            .map(|p| {
                if is_active {
                    p.fill(Ink::Raised, a)
                } else {
                    p.bg(hsla(Ink::Raised.color(a).with_alpha(0.5)))
                }
            })
            .child(button);
        let close = div()
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .rounded_r(px(6.0))
            // Under an inactive tab's half fill, the panel's own color, so the name stays hidden.
            .when(!is_active, |c| {
                c.fill(Ink::Window, a)
                    .invisible()
                    .group_hover(group.clone(), |s| s.visible())
            })
            .child(patch);
        let tab = div()
            .id(group.clone())
            .group(group)
            .relative()
            .flex_shrink()
            .min_w(px(48.0))
            .max_w(px(120.0))
            .h(px(26.0))
            .px(px(10.0))
            .rounded(px(6.0))
            .overflow_hidden()
            .whitespace_nowrap()
            .flex()
            .items_center()
            .cursor_pointer()
            .when(i == active, |t| t.fill(Ink::Raised, a))
            .when(i != active, |t| {
                t.hover(move |s| s.bg(hsla(Ink::Raised.color(a).with_alpha(0.5))))
            })
            .child(label(&title, TypeRole::Label, Ink::Text, a).flex_shrink_0())
            .child(close)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.terminal.tabs.select(i);
                this.focus_terminal(window);
                cx.notify();
            }));
        let tab = gate::tip_detail(
            tab,
            title.clone().into(),
            format!("{}:{}", who(), shell.folder).into(),
            a,
        );
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
            on_show(&screen, &shell.back),
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
    let selection = shell.selection.get().filter(|s| !s.is_empty());
    let highlights = selection.into_iter().flat_map(|s| {
        cells.iter().enumerate().filter_map(move |(r, row)| {
            s.columns(r, row.len()).map(|(from, to)| {
                div()
                    .absolute()
                    .left(px(PAD_X + from as f32 * CELL))
                    .top(px(PAD_Y + r as f32 * LINE))
                    .w(px((to - from) as f32 * CELL))
                    .h(px(LINE))
                    .bg(hsla(Ink::Selection.color(a)))
            })
        })
    });
    let focused = shell.focus.is_focused(window);
    let caret = (cursor_on && !ended && shell.back.get() == 0).then(|| {
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
    let (origin, dragging, picked) = (
        shell.origin.clone(),
        shell.dragging.clone(),
        shell.selection.clone(),
    );
    let fit = canvas(
        |bounds, _, _| bounds,
        move |_, bounds, window, _| {
            origin.set(bounds.origin + Point::new(px(PAD_X), px(PAD_Y)));
            // A drag goes on outside the grid too, so these listen to the whole window.
            {
                let (origin, dragging, picked, size) = (
                    origin.clone(),
                    dragging.clone(),
                    picked.clone(),
                    size.clone(),
                );
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                    if phase != DispatchPhase::Bubble || !dragging.get() {
                        return;
                    }
                    if event.pressed_button != Some(MouseButton::Left) {
                        dragging.set(false);
                        return;
                    }
                    let at = event.position - origin.get();
                    let (rows, cols) = size.get();
                    let head = boundary_at(
                        f32::from(at.x),
                        f32::from(at.y),
                        usize::from(rows),
                        usize::from(cols),
                    );
                    if let Some(mut s) = picked.get() {
                        if s.head != head {
                            s.head = head;
                            picked.set(Some(s));
                            window.refresh();
                        }
                    }
                });
            }
            {
                let dragging = dragging.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
                    if phase == DispatchPhase::Bubble {
                        dragging.set(false);
                    }
                });
            }
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
    let press = {
        let (screen, origin, dragging, picked, back) = (
            shell.screen.clone(),
            shell.origin.clone(),
            shell.dragging.clone(),
            shell.selection.clone(),
            shell.back.clone(),
        );
        move |event: &MouseDownEvent, window: &mut Window, _: &mut gpui::App| {
            focus.focus(window);
            let at = event.position - origin.get();
            let cells = on_show(&screen.lock().unwrap_or_else(|e| e.into_inner()), &back);
            let cols = cells.first().map_or(0, Vec::len);
            let (row, col) = boundary_at(f32::from(at.x), f32::from(at.y), cells.len(), cols);
            let line = cells.get(row).map(Vec::as_slice).unwrap_or(&[]);
            // A second click takes the word under it, a third the whole line.
            let (from, to) = match event.click_count {
                1 => (col, col),
                2 => {
                    let x = (f32::from(at.x) / CELL).floor().max(0.0) as usize;
                    word_at(line, x)
                }
                _ => (0, line.len()),
            };
            picked.set(Some(Selection {
                anchor: (row, from),
                head: (row, to),
            }));
            dragging.set(event.click_count == 1);
            window.refresh();
        }
    };
    let (picked, cells_for_copy, back) = (
        shell.selection.clone(),
        shell.screen.clone(),
        shell.back.clone(),
    );
    let scroll = {
        let (screen, back, wheel, picked) = (
            shell.screen.clone(),
            shell.back.clone(),
            shell.wheel.clone(),
            shell.selection.clone(),
        );
        move |event: &ScrollWheelEvent, window: &mut Window, cx: &mut gpui::App| {
            let screen = screen.lock().unwrap_or_else(|e| e.into_inner());
            // A full-screen program (an editor, `less`) has no scrollback of its own here.
            if screen.alt_screen() {
                return;
            }
            let moved = wheel.get() + f32::from(event.delta.pixel_delta(px(LINE)).y);
            let rows = (moved / LINE).trunc();
            wheel.set(moved - rows * LINE);
            let most = screen.scrollback().len() as i64;
            let to = (back.get() as i64 + rows as i64).clamp(0, most) as usize;
            if to != back.get() {
                back.set(to);
                picked.set(None);
                window.refresh();
            }
            cx.stop_propagation();
        }
    };
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
        .on_mouse_down(MouseButton::Left, press)
        .on_scroll_wheel(scroll)
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            let keys = &event.keystroke;
            let m = &keys.modifiers;
            let (app_cursor, bracketed) = {
                let s = screen.lock().unwrap_or_else(|e| e.into_inner());
                (s.app_cursor_keys(), s.bracketed_paste())
            };
            let out = if m.platform {
                // ⌘C copies the selection, ⌘V pastes; the other ⌘ keys stay the window's and the menu's.
                match keys.key.as_str() {
                    "c" => {
                        if let Some(s) = picked.get().filter(|s| !s.is_empty()) {
                            let text = {
                                let screen =
                                    cells_for_copy.lock().unwrap_or_else(|e| e.into_inner());
                                selected_text(&on_show(&screen, &back), s)
                            };
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                            cx.stop_propagation();
                        }
                        return;
                    }
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
                if picked.take().is_some() || back.replace(0) != 0 {
                    window.refresh();
                }
                let _ = pty.write_all_timeout(out.as_bytes(), Duration::from_millis(500));
                cx.stop_propagation();
            }
        })
        .children(highlights)
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

/// `user@host`, as a shell prompt shows it, for a tab's tip.
fn who() -> &'static str {
    static WHO: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    WHO.get_or_init(|| {
        let user = std::env::var("USER").unwrap_or_default();
        let host = notchling_engine::bridge::net::bridge::host_name().unwrap_or_default();
        let host = host.split('.').next().unwrap_or_default();
        format!("{user}@{host}")
    })
}

/// The rows on show: the live screen, or older rows when scrolled back.
fn on_show(screen: &TerminalScreen, back: &Cell<usize>) -> Vec<Vec<TermCell>> {
    if back.get() == 0 {
        return screen.cells().to_vec();
    }
    back.set(back.get().min(screen.scrollback().len()));
    shown_rows(screen.scrollback().iter(), screen.cells(), back.get())
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
        )
        .size(px(super::sidebar::HEADER_BUTTON))
        .px(px(0.0)),
        "Terminal: a shell in this session's folder".into(),
        a,
    )
    .on_click(cx.listener(|this, _, window, cx| this.toggle_terminal(window, cx)))
    .part_at("Terminal Button", Corner::BottomTrailing, a, cx)
}
