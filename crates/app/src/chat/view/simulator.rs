//! The iOS Simulator in the Details Panel's place, as in Claude Desktop: the header's Simulator
//! button shows a booted simulator's screen, live (a `simctl io screenshot` about twice a second),
//! with a toolbar: the device (a click goes to the next when several are booted), Screenshot (a PNG
//! on the Desktop), Record and Stop (a video on the Desktop), Open Simulator and Close. It only
//! shows the screen; taps and keys go to Simulator itself. The pure part is `chat::simulator`.

use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, img, prelude::*, px, AnyElement, Context, Div, ImageFormat, ObjectFit, RenderImage,
    SharedString, Window,
};
use notchling_engine::control::{Command, Launcher, RealLauncher};
use notchling_engine::pages::clock::{civil_at, local_offset_secs};
use notchling_kit::labels::Corner;
use notchling_kit::metric::SIDEBAR_WIDTH;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, icon, icon_button, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};

use super::ChatView;
use crate::chat::page_panel::panel_width;
use crate::chat::simulator::{
    booted, list_command, record_args, saved_name, screenshot_command, Device, Saved, XCRUN,
};
use crate::mac::gate;

/// Set, a render draws the panel with a sample device and no screen yet.
pub const RENDER_ENV: &str = "NOTCHLING_RENDER_SIMULATOR";

/// The panel's width beside the conversation: a phone's screen at a readable size.
const WIDTH: f32 = 380.0;
const HEADER: f32 = 36.0;
/// The pause between two frames, after the one before has come in.
const FRAME_GAP: Duration = Duration::from_millis(120);
/// How often the list of booted devices is read again while the panel shows.
const LIST_EVERY: Duration = Duration::from_secs(4);

/// A recording under way. Dropping it stops it, so closing Chat doesn't leave `simctl` recording.
struct Recording {
    child: Child,
    path: PathBuf,
}

impl Recording {
    /// SIGINT, which makes `simctl` finish the file.
    fn stop(&self) {
        let _ = std::process::Command::new("/bin/kill")
            .args(["-INT", &self.child.id().to_string()])
            .status();
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The panel's state in the Chat window.
#[derive(Default)]
pub struct State {
    pub open: bool,
    devices: Vec<Device>,
    /// The device shown, by udid, so a new list keeps it.
    active: Option<String>,
    /// The latest frame, and its device.
    frame: Option<(String, Arc<RenderImage>)>,
    /// Bumped when the panel opens, so an older loop stops.
    run: u64,
    /// Whether the list has been read once since the panel opened.
    listed: bool,
    recording: Option<Recording>,
    /// A line under the toolbar: where a file went, or what failed.
    note: Option<String>,
}

impl State {
    pub fn shown(&self) -> bool {
        self.open
    }

    fn device(&self) -> Option<&Device> {
        let active = self.active.as_deref();
        self.devices
            .iter()
            .find(|d| Some(d.udid.as_str()) == active)
            .or(self.devices.first())
    }
}

/// The frame's file: one per app, overwritten each time.
fn frame_path() -> PathBuf {
    std::env::temp_dir().join(format!("notchling-simulator-{}.jpg", std::process::id()))
}

fn desktop(home: &Path) -> PathBuf {
    let d = home.join("Desktop");
    if d.is_dir() {
        d
    } else {
        home.to_path_buf()
    }
}

fn now_civil() -> notchling_engine::pages::clock::Civil {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    civil_at(ts, local_offset_secs(ts))
}

/// Runs `command` to its end; its output, or what went wrong.
fn run(home: &Path, command: &Command, timeout: Duration) -> Result<String, String> {
    let out = RealLauncher::new(home)
        .run(command, timeout)
        .map_err(|e| e.to_string())?;
    if out.code == 0 {
        Ok(out.output)
    } else {
        let said = out.combined();
        Err(said
            .lines()
            .last()
            .unwrap_or("simctl failed")
            .trim()
            .to_owned())
    }
}

impl ChatView {
    /// The render's panel: one sample device, waiting for its screen.
    pub(super) fn render_simulator(&mut self) {
        self.simulator.devices = vec![Device {
            udid: "render".into(),
            name: "iPhone 17 Pro".into(),
            runtime: "iOS 26.5".into(),
        }];
        self.simulator.listed = true;
        self.simulator.open = true;
    }

    /// The header's Simulator button: shows the panel and starts its live view, or hides it.
    pub(super) fn toggle_simulator(&mut self, cx: &mut Context<Self>) {
        if self.simulator.open {
            self.close_simulator(cx);
            return;
        }
        if self.page.is_open() {
            self.close_page(cx);
        }
        self.terminal.open = false;
        self.terminal.expanded = false;
        self.simulator.open = true;
        self.simulator.listed = false;
        self.simulator.note = None;
        self.simulator.run += 1;
        self.watch_simulator(self.simulator.run, cx);
        cx.notify();
    }

    fn close_simulator(&mut self, cx: &mut Context<Self>) {
        self.simulator.open = false;
        self.simulator.run += 1;
        // A recording ends with the panel.
        self.stop_recording(cx);
        if let Some((_, old)) = self.simulator.frame.take() {
            cx.drop_image(old, None);
        }
        cx.notify();
    }

    /// While the panel shows: the booted devices every few seconds, and the shown one's screen as
    /// fast as `simctl` gives it. Ends when the panel closes or opens again (`run` changed).
    fn watch_simulator(&mut self, run_id: u64, cx: &mut Context<Self>) {
        let home = self.root.home.clone();
        let renderer = cx.svg_renderer();
        cx.spawn(async move |this, cx| {
            let mut listed_at: Option<std::time::Instant> = None;
            loop {
                let open = this
                    .update(cx, |v, _| v.simulator.open && v.simulator.run == run_id)
                    .unwrap_or(false);
                if !open {
                    break;
                }
                if listed_at.is_none_or(|t| t.elapsed() >= LIST_EVERY) {
                    let h = home.clone();
                    let answer = cx
                        .background_executor()
                        .spawn(async move { run(&h, &list_command(), Duration::from_secs(10)) })
                        .await;
                    listed_at = Some(std::time::Instant::now());
                    let ok = this
                        .update(cx, |v, cx| {
                            if v.simulator.run != run_id {
                                return false;
                            }
                            match answer {
                                Ok(json) => v.simulator.devices = booted(&json),
                                Err(e) => {
                                    v.simulator.devices.clear();
                                    v.simulator.note =
                                        Some(format!("Couldn't list simulators: {e}"));
                                }
                            }
                            v.simulator.listed = true;
                            cx.notify();
                            true
                        })
                        .unwrap_or(false);
                    if !ok {
                        break;
                    }
                }
                let udid = this
                    .update(cx, |v, _| v.simulator.device().map(|d| d.udid.clone()))
                    .ok()
                    .flatten();
                let Some(udid) = udid else {
                    cx.background_executor().timer(LIST_EVERY).await;
                    continue;
                };
                let (h, r, u) = (home.clone(), renderer.clone(), udid.clone());
                let frame = cx
                    .background_executor()
                    .spawn(async move {
                        let path = frame_path();
                        run(
                            &h,
                            &screenshot_command(&u, &path, true),
                            Duration::from_secs(10),
                        )?;
                        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                        gpui::Image::from_bytes(ImageFormat::Jpeg, bytes)
                            .to_image_data(r)
                            .map_err(|e| e.to_string())
                    })
                    .await;
                let go_on = this
                    .update(cx, |v, cx| {
                        if !(v.simulator.open && v.simulator.run == run_id) {
                            return false;
                        }
                        match frame {
                            Ok(image) => {
                                // The old frame leaves the GPU's atlas, or each one would stay.
                                if let Some((_, old)) = v.simulator.frame.replace((udid, image)) {
                                    cx.drop_image(old, None);
                                }
                            }
                            // The device shut down between two lists: the next list says so.
                            Err(_) => listed_at = None,
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !go_on {
                    break;
                }
                cx.background_executor().timer(FRAME_GAP).await;
            }
        })
        .detach();
    }

    /// The device row: the next booted device.
    fn next_simulator(&mut self, cx: &mut Context<Self>) {
        let list = &self.simulator.devices;
        if list.len() < 2 {
            return;
        }
        let at = self
            .simulator
            .device()
            .and_then(|d| list.iter().position(|x| x.udid == d.udid))
            .unwrap_or(0);
        self.simulator.active = Some(list[(at + 1) % list.len()].udid.clone());
        cx.notify();
    }

    /// Screenshot: a PNG of the shown device on the Desktop.
    fn save_screenshot(&mut self, cx: &mut Context<Self>) {
        let Some(device) = self.simulator.device().cloned() else {
            return;
        };
        let home = self.root.home.clone();
        let path = desktop(&home).join(saved_name(Saved::Screenshot, &device.name, now_civil()));
        cx.spawn(async move |this, cx| {
            let p = path.clone();
            let done = cx
                .background_executor()
                .spawn(async move {
                    run(
                        &home,
                        &screenshot_command(&device.udid, &p, false),
                        Duration::from_secs(15),
                    )
                })
                .await;
            this.update(cx, |v, cx| {
                v.simulator.note = Some(match done {
                    Ok(_) => format!("Saved {}", file_name(&path)),
                    Err(e) => format!("Couldn't take the screenshot: {e}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Record: a video of the shown device on the Desktop, until Stop.
    fn start_recording(&mut self, cx: &mut Context<Self>) {
        let Some(device) = self.simulator.device().cloned() else {
            return;
        };
        if self.simulator.recording.is_some() {
            return;
        }
        let path =
            desktop(&self.root.home).join(saved_name(Saved::Recording, &device.name, now_civil()));
        match std::process::Command::new(XCRUN)
            .args(record_args(&device.udid, &path))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => {
                self.simulator.recording = Some(Recording { child, path });
                self.simulator.note = Some("Recording…".into());
            }
            Err(e) => self.simulator.note = Some(format!("Couldn't record: {e}")),
        }
        cx.notify();
    }

    /// Stop: `simctl` finishes the file, then the note names it.
    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        let Some(mut recording) = self.simulator.recording.take() else {
            return;
        };
        recording.stop();
        let path = recording.path.clone();
        self.simulator.note = Some("Saving the recording…".into());
        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_executor()
                .spawn(async move {
                    // Up to 15 s for simctl to write the file's end.
                    for _ in 0..150 {
                        if let Ok(Some(_)) = recording.child.try_wait() {
                            return true;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    false
                })
                .await;
            this.update(cx, |v, cx| {
                // A note only when nothing newer replaced it.
                if v.simulator.recording.is_none() {
                    v.simulator.note = Some(if saved && path.exists() {
                        format!("Saved {}", file_name(&path))
                    } else {
                        "The recording didn't finish".into()
                    });
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn open_simulator_app(&mut self, cx: &mut Context<Self>) {
        let home = self.root.home.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = run(
                    &home,
                    &Command::new(&["/usr/bin/open", "-a", "Simulator"]),
                    Duration::from_secs(15),
                );
            })
            .detach();
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn tool(id: &'static str, shape: Icon, tip: &'static str, a: Appearance) -> gpui::Stateful<Div> {
    gate::tip_text(
        icon_button(
            id,
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

/// The top row: the device, Open Simulator and Close.
fn header(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let device = view.simulator.device();
    let several = view.simulator.devices.len() > 1;
    let name = device
        .map(|d| d.name.clone())
        .unwrap_or_else(|| "Simulator".into());
    let runtime = device.map(|d| d.runtime.clone()).unwrap_or_default();
    let who = div()
        .id("sim-device")
        .flex_1()
        .min_w(px(0.0))
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(icon(Icon::Phone, 12.0, Ink::TextMuted, a))
        .child(
            label(&name, TypeRole::Label, Ink::Text, a)
                .flex_shrink()
                .elide(),
        )
        .child(label(&runtime, TypeRole::Meta, Ink::TextFaint, a).flex_shrink_0())
        .when(several, |row| {
            row.cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.next_simulator(cx)))
        });
    let who = if several {
        gate::tip_text(who, "Show the next booted simulator".into(), a)
    } else {
        who
    };
    let open = tool("sim-open", Icon::Expand, "Open Simulator", a)
        .on_click(cx.listener(|this, _, _, cx| this.open_simulator_app(cx)));
    let close = tool("sim-close", Icon::Close, "Close the simulator view", a)
        .on_click(cx.listener(|this, _, _, cx| this.close_simulator(cx)));
    div()
        .h(px(HEADER))
        .flex_shrink_0()
        .pl(px(10.0))
        .pr(px(6.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(who)
        .child(open)
        .child(close)
}

/// The bar under the screen: Screenshot, Record or Stop, and the note (where a file went).
fn controls(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let has_device = view.simulator.device().is_some();
    let recording = view.simulator.recording.is_some();
    let state = if has_device {
        PartState::Rest
    } else {
        PartState::Disabled
    };
    let shot = gate::tip_text(
        button("sim-shot", "Screenshot", ButtonKind::Ghost, state, a),
        "Save a screenshot to the Desktop".into(),
        a,
    )
    .when(has_device, |b| {
        b.on_click(cx.listener(|this, _, _, cx| this.save_screenshot(cx)))
    });
    let record = if recording {
        gate::tip_text(
            button(
                "sim-record",
                "Stop",
                ButtonKind::Secondary,
                PartState::Rest,
                a,
            ),
            "Stop recording and save the video to the Desktop".into(),
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| this.stop_recording(cx)))
    } else {
        gate::tip_text(
            button("sim-record", "Record", ButtonKind::Ghost, state, a),
            "Record the screen to a video on the Desktop".into(),
            a,
        )
        .when(has_device, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.start_recording(cx)))
        })
    };
    let note = view
        .simulator
        .note
        .as_ref()
        .map(|note| label(note, TypeRole::Meta, Ink::TextMuted, a).elide());
    div()
        .flex_shrink_0()
        .px(px(12.0))
        .pb(px(10.0))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.0))
        .child(div().flex().gap(px(8.0)).child(shot).child(record))
        .children(note)
}

/// The screen, fitted to the room; or why there's none.
fn screen(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> AnyElement {
    let s = &view.simulator;
    let device = s.device();
    let frame = s
        .frame
        .as_ref()
        .filter(|(udid, _)| device.is_some_and(|d| &d.udid == udid))
        .map(|(_, image)| image.clone());
    let body = div()
        .flex_1()
        .min_h(px(0.0))
        .p(px(16.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.0));
    if let Some(image) = frame {
        return body
            .child(
                img(image)
                    .size_full()
                    .object_fit(ObjectFit::Contain)
                    .rounded(px(18.0)),
            )
            .into_any_element();
    }
    let (line, offer) = match (s.listed, device) {
        (false, _) => ("Looking for a simulator…", false),
        (true, Some(_)) => ("Waiting for the screen…", false),
        (true, None) => ("No simulator is running", true),
    };
    body.child(label(line, TypeRole::Body, Ink::TextMuted, a))
        .when(offer, |b| {
            b.child(
                button(
                    "sim-open-empty",
                    "Open Simulator",
                    ButtonKind::Secondary,
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(|this, _, _, cx| this.open_simulator_app(cx))),
            )
        })
        .into_any_element()
}

/// The panel while it shows, beside the conversation.
pub fn render(
    view: &ChatView,
    a: Appearance,
    window: &Window,
    cx: &mut Context<ChatView>,
) -> Option<AnyElement> {
    if !view.simulator.shown() || view.page.is_open() {
        return None;
    }
    let sidebar = if view.sidebar_hidden {
        0.0
    } else {
        SIDEBAR_WIDTH
    };
    let available = f32::from(window.viewport_size().width) - sidebar;
    Some(
        div()
            .h_full()
            .w(px(panel_width(WIDTH, available)))
            .flex_shrink_0()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .fill(Ink::Window, a)
            .border_l(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(header(view, a, cx))
            .child(div().h(px(1.0)).flex_shrink_0().fill(Ink::Hairline, a))
            .child(screen(view, a, cx))
            .child(controls(view, a, cx))
            .part_at("Simulator Panel", Corner::TopTrailing, a, cx)
            .into_any_element(),
    )
}

/// The header's Simulator button, raised while the panel shows.
pub fn button_in_header(
    view: &ChatView,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> impl IntoElement {
    let kind = if view.simulator.shown() {
        ButtonKind::Secondary
    } else {
        ButtonKind::Ghost
    };
    gate::tip_text(
        icon_button(
            "simulator-button",
            Icon::Phone,
            IconSize::Button.points(),
            kind,
            PartState::Rest,
            a,
        ),
        SharedString::from("iOS Simulator: its screen, live"),
        a,
    )
    .on_click(cx.listener(|this, _, _, cx| this.toggle_simulator(cx)))
    .part_at("Simulator Button", Corner::BottomTrailing, a, cx)
}
