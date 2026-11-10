//! The iOS Simulator in the Details Panel's place, as in Claude Desktop. The header's Simulator
//! button shows a booted simulator's screen, live (a `simctl io screenshot` about twice a second),
//! in a phone's frame. Along the top: the device, a menu (the booted ones, the ones it can boot,
//! Add Simulator…, Add Photos or Videos, Open in Simulator, Detach, Shut Down), and the zoom (−,
//! Fit, 1×, +). Along the bottom: Home and Rotate (Simulator's own keys, since `simctl` has
//! neither), Screenshot and Record (to the Desktop), and Annotate: draw on a still of the screen
//! and add it to the reply. Taps and typing still go to Simulator itself. The pure part is
//! `chat::simulator`.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{Bounds, Context, ImageFormat, PathPromptOptions, Pixels, RenderImage, Window};
use notchling_engine::control::{Command, Launcher, RealLauncher};
use notchling_engine::pages::clock::{civil_at, local_offset_secs, Civil};

use super::ChatView;
use crate::chat::simulator::{
    add_media_command, boot_command, burn, create_command, devices, frame_for, key_script,
    list_command, record_args, runtimes, runtimes_command, saved_name, screenshot_command,
    shutdown_command, Device, Runtime, Saved, SimKey, Zoom, XCRUN,
};
use crate::phone::bridge::now_ms;

/// Set, a render draws the panel with a sample device; set to a PNG's path, with that screen
/// (`<png>#menu`: the device menu open too).
pub const RENDER_ENV: &str = "NOTCHLING_RENDER_SIMULATOR";

/// The panel's width beside the conversation: a phone's screen at a readable size.
const WIDTH: f32 = 400.0;
const HEADER: f32 = 52.0;
/// The pause between two frames, after the one before has come in.
const FRAME_GAP: Duration = Duration::from_millis(120);
/// How often the list of simulators is read again while the panel shows.
const LIST_EVERY: Duration = Duration::from_secs(4);
/// The space around the phone in its area.
const AREA_PAD: f32 = 20.0;
/// Annotate's pen: red, a share of the screenshot's width.
const PEN: u32 = 0xff3b30;
const PEN_SHARE: f32 = 0.008;

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

/// Add Simulator…: the runtimes, and which runtime and device type are picked.
struct NewSimulator {
    runtimes: Vec<Runtime>,
    runtime: usize,
    kind: usize,
}

/// Annotate: a still of the screen and the lines drawn on it, as shares 0–1 of its size.
struct Annotation {
    image: Arc<RenderImage>,
    strokes: Vec<Vec<(f32, f32)>>,
    drawing: bool,
}

/// The panel's state in the Chat window.
#[derive(Default)]
pub struct State {
    pub open: bool,
    /// Every simulator `simctl` lists, booted or not.
    all: Vec<Device>,
    /// The device shown, by udid, so a new list keeps it.
    active: Option<String>,
    /// The latest frame, and its device.
    frame: Option<(String, Arc<RenderImage>)>,
    /// Bumped when the panel opens, so an older loop stops.
    run: u64,
    /// Whether the list has been read once since the panel opened.
    listed: bool,
    /// The loop reads the list again at once (after a boot or a shutdown).
    relist: bool,
    recording: Option<Recording>,
    /// A line under the toolbar: where a file went, or what failed.
    note: Option<String>,
    /// Something under way that the screen area says ("Booting Lingeva Revamp…").
    busy: Option<String>,
    menu_open: bool,
    zoom: Option<Zoom>,
    adding: Option<NewSimulator>,
    annotation: Option<Annotation>,
    /// Where the screen was drawn last, and the room the phone had, for the pen and for Fit.
    screen_at: Rc<Cell<Bounds<Pixels>>>,
    room: Rc<Cell<(f32, f32)>>,
}

impl State {
    pub fn shown(&self) -> bool {
        self.open
    }

    /// The booted device shown: the one picked, else the first.
    fn device(&self) -> Option<&Device> {
        let active = self.active.as_deref();
        let booted = || self.all.iter().filter(|d| d.booted);
        booted()
            .find(|d| Some(d.udid.as_str()) == active)
            .or_else(|| booted().next())
    }

    fn zoom(&self) -> Zoom {
        self.zoom.unwrap_or(Zoom::Fit)
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

fn now_civil() -> Civil {
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
            .rfind(|l| !l.trim().is_empty())
            .unwrap_or("simctl failed")
            .trim()
            .to_owned())
    }
}

/// The still with its lines burned in, as a PNG.
fn annotated_png(image: &RenderImage, strokes: &[Vec<(f32, f32)>]) -> Result<Vec<u8>, String> {
    let size = image.size(0);
    let (w, h) = (size.width.0 as usize, size.height.0 as usize);
    let mut bgra = image.as_bytes(0).ok_or("no pixels")?.to_vec();
    let pen = (
        (PEN & 0xff) as u8,
        ((PEN >> 8) & 0xff) as u8,
        (PEN >> 16) as u8,
    );
    burn(&mut bgra, w, h, strokes, w as f32 * PEN_SHARE, pen);
    for px in bgra.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&bgra).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

impl ChatView {
    /// The render's panel: one sample device, with the screen in the PNG `how` names, if any.
    pub(super) fn render_simulator(&mut self, how: &str, cx: &mut Context<Self>) {
        self.simulator.all = vec![Device {
            udid: "render".into(),
            name: "iPhone 17 Pro".into(),
            runtime: "iOS 26.5".into(),
            booted: true,
            last_used: String::new(),
            kind: "com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro".into(),
        }];
        // `<png>#menu` opens the device menu as well, with two simulators it could boot.
        let (how, open_menu) = match how.strip_suffix("#menu") {
            Some(path) => (path, true),
            None => (how, false),
        };
        if open_menu {
            for (i, name) in ["Lingeva Revamp", "fix-grammar-snap"].iter().enumerate() {
                self.simulator.all.push(Device {
                    udid: format!("render-{i}"),
                    name: (*name).into(),
                    runtime: "iOS 26.5".into(),
                    booted: false,
                    last_used: String::new(),
                    kind: String::new(),
                });
            }
            self.simulator.menu_open = true;
        }
        if let Ok(bytes) = std::fs::read(how) {
            if let Ok(image) =
                gpui::Image::from_bytes(ImageFormat::Png, bytes).to_image_data(cx.svg_renderer())
            {
                self.simulator.frame = Some(("render".into(), image));
            }
        }
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
        let s = &mut self.simulator;
        s.open = true;
        s.listed = false;
        s.note = None;
        s.busy = None;
        s.run += 1;
        self.watch_simulator(self.simulator.run, cx);
        cx.notify();
    }

    fn close_simulator(&mut self, cx: &mut Context<Self>) {
        let s = &mut self.simulator;
        s.open = false;
        s.menu_open = false;
        s.adding = None;
        s.annotation = None;
        s.run += 1;
        // A recording ends with the panel.
        self.stop_recording(cx);
        if let Some((_, old)) = self.simulator.frame.take() {
            cx.drop_image(old, None);
        }
        cx.notify();
    }

    /// While the panel shows: the simulators every few seconds, and the shown one's screen as fast
    /// as `simctl` gives it. Ends when the panel closes or opens again (`run` changed).
    fn watch_simulator(&mut self, run_id: u64, cx: &mut Context<Self>) {
        let home = self.root.home.clone();
        let renderer = cx.svg_renderer();
        cx.spawn(async move |this, cx| {
            let mut listed_at: Option<std::time::Instant> = None;
            loop {
                let (open, relist) = this
                    .update(cx, |v, _| {
                        let s = &mut v.simulator;
                        (s.open && s.run == run_id, std::mem::take(&mut s.relist))
                    })
                    .unwrap_or((false, false));
                if !open {
                    break;
                }
                if relist || listed_at.is_none_or(|t| t.elapsed() >= LIST_EVERY) {
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
                                Ok(json) => v.simulator.all = devices(&json),
                                Err(e) => {
                                    v.simulator.all.clear();
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
                    cx.background_executor()
                        .timer(Duration::from_millis(500))
                        .await;
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
                                // The old frame leaves the GPU's atlas, or each one would stay. A
                                // still being annotated holds its own handle and is kept.
                                if let Some((_, old)) = v.simulator.frame.replace((udid, image)) {
                                    let held = v
                                        .simulator
                                        .annotation
                                        .as_ref()
                                        .is_some_and(|n| Arc::ptr_eq(&n.image, &old));
                                    if !held {
                                        cx.drop_image(old, None);
                                    }
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

    /// Runs a `simctl` step in the background with `busy` said meanwhile, then `done` with its
    /// answer, and reads the list again.
    fn simctl_step(
        &mut self,
        busy: String,
        command: Command,
        done: impl FnOnce(&mut ChatView, Result<String, String>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.simulator.busy = Some(busy);
        self.simulator.menu_open = false;
        let home = self.root.home.clone();
        cx.spawn(async move |this, cx| {
            let answer = cx
                .background_executor()
                .spawn(async move { run(&home, &command, Duration::from_secs(120)) })
                .await;
            this.update(cx, |v, cx| {
                v.simulator.busy = None;
                v.simulator.relist = true;
                done(v, answer);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn pick_simulator(&mut self, device: &Device, cx: &mut Context<Self>) {
        self.simulator.menu_open = false;
        if device.booted {
            self.simulator.active = Some(device.udid.clone());
            cx.notify();
            return;
        }
        let udid = device.udid.clone();
        self.simctl_step(
            format!("Booting {}…", device.name),
            boot_command(&udid),
            move |v, answer| match answer {
                Ok(_) => v.simulator.active = Some(udid),
                Err(e) => v.simulator.note = Some(format!("Couldn't boot it: {e}")),
            },
            cx,
        );
    }

    fn shut_down_simulator(&mut self, cx: &mut Context<Self>) {
        let Some(device) = self.simulator.device().cloned() else {
            return;
        };
        self.stop_recording(cx);
        self.simctl_step(
            format!("Shutting down {}…", device.name),
            shutdown_command(&device.udid),
            move |v, answer| {
                if let Err(e) = answer {
                    v.simulator.note = Some(format!("Couldn't shut it down: {e}"));
                }
                v.simulator.active = None;
            },
            cx,
        );
    }

    /// Add Simulator…: reads the runtimes, then shows the form in the screen's place.
    fn begin_new_simulator(&mut self, cx: &mut Context<Self>) {
        self.simulator.menu_open = false;
        let home = self.root.home.clone();
        self.simulator.busy = Some("Reading the runtimes…".into());
        cx.spawn(async move |this, cx| {
            let answer = cx
                .background_executor()
                .spawn(async move { run(&home, &runtimes_command(), Duration::from_secs(30)) })
                .await;
            this.update(cx, |v, cx| {
                v.simulator.busy = None;
                let list = answer.map(|json| runtimes(&json)).unwrap_or_default();
                if list.iter().all(|r| r.types.is_empty()) {
                    v.simulator.note =
                        Some("No iOS runtime is installed (Xcode > Settings > Components)".into());
                } else {
                    let runtime = list.iter().position(|r| !r.types.is_empty()).unwrap_or(0);
                    v.simulator.adding = Some(NewSimulator {
                        runtimes: list,
                        runtime,
                        kind: 0,
                    });
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Create: the picked device type on the picked runtime, named as the type, then booted.
    fn create_simulator(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.simulator.adding.take() else {
            return;
        };
        let Some(runtime) = form.runtimes.get(form.runtime) else {
            return;
        };
        let Some((name, kind)) = runtime.types.get(form.kind).cloned() else {
            return;
        };
        let home = self.root.home.clone();
        let runtime_id = runtime.id.clone();
        self.simulator.busy = Some(format!("Making {name}…"));
        cx.spawn(async move |this, cx| {
            let made = cx
                .background_executor()
                .spawn(async move {
                    let udid = run(
                        &home,
                        &create_command(&name, &kind, &runtime_id),
                        Duration::from_secs(120),
                    )?;
                    let udid = udid.trim().to_owned();
                    run(&home, &boot_command(&udid), Duration::from_secs(120))?;
                    Ok::<String, String>(udid)
                })
                .await;
            this.update(cx, |v, cx| {
                v.simulator.busy = None;
                v.simulator.relist = true;
                match made {
                    Ok(udid) => v.simulator.active = Some(udid),
                    Err(e) => v.simulator.note = Some(format!("Couldn't make it: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Add Photos or Videos: a file picker, then into the device's Photos.
    fn pick_media(&mut self, cx: &mut Context<Self>) {
        self.simulator.menu_open = false;
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                this.update(cx, |v, cx| v.add_media(paths, cx)).ok();
            }
        })
        .detach();
        cx.notify();
    }

    fn add_media(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(device) = self.simulator.device().cloned() else {
            return;
        };
        if paths.is_empty() {
            return;
        }
        let n = paths.len();
        self.simctl_step(
            format!("Adding to {}'s Photos…", device.name),
            add_media_command(&device.udid, &paths),
            move |v, answer| {
                v.simulator.note = Some(match answer {
                    Ok(_) if n == 1 => "Added to Photos".into(),
                    Ok(_) => format!("Added {n} to Photos"),
                    Err(e) => format!("Couldn't add them: {e}"),
                });
            },
            cx,
        );
    }

    /// Home and Rotate: Simulator's own keys in the device's window, then Chat comes back.
    fn press_simulator_key(&mut self, key: SimKey, cx: &mut Context<Self>) {
        let Some(device) = self.simulator.device().cloned() else {
            return;
        };
        let home = self.root.home.clone();
        let script = key_script(&device.name, key);
        cx.spawn(async move |this, cx| {
            let answer = cx
                .background_executor()
                .spawn(async move {
                    run(
                        &home,
                        &Command::new(&["/usr/bin/osascript", "-e", &script]),
                        Duration::from_secs(20),
                    )
                })
                .await;
            this.update(cx, |v, cx| {
                match answer {
                    Ok(_) => v.simulator.note = None,
                    Err(_) => {
                        v.simulator.note = Some(
                            "Home and Rotate press Simulator's keys: allow Notchling Next in \
                             System Settings > Privacy & Security > Accessibility"
                                .into(),
                        )
                    }
                }
                cx.activate(true);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn set_zoom(&mut self, zoom: Zoom, cx: &mut Context<Self>) {
        self.simulator.zoom = Some(zoom);
        cx.notify();
    }

    fn step_zoom(&mut self, up: bool, cx: &mut Context<Self>) {
        let Some((pixels, scale, ipad)) = self.frame_pixels() else {
            return;
        };
        let room = self.simulator.room.get();
        let frame = frame_for(room.0, ipad).0;
        let next = self.simulator.zoom().step(up, pixels, scale, room, frame);
        self.set_zoom(next, cx);
    }

    /// The shown frame's size in pixels, its points per pixel, and whether it is an iPad's.
    fn frame_pixels(&self) -> Option<((f32, f32), f32, bool)> {
        let device = self.simulator.device()?;
        let (_, image) = self.simulator.frame.as_ref()?;
        let size = image.size(0);
        Some((
            (size.width.0 as f32, size.height.0 as f32),
            device.scale(),
            device.ipad(),
        ))
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

    fn toggle_recording(&mut self, cx: &mut Context<Self>) {
        if self.simulator.recording.is_some() {
            self.stop_recording(cx);
        } else {
            self.start_recording(cx);
        }
    }

    /// Record: a video of the shown device on the Desktop, until Stop.
    fn start_recording(&mut self, cx: &mut Context<Self>) {
        let Some(device) = self.simulator.device().cloned() else {
            return;
        };
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
        self.simulator.menu_open = false;
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
        cx.notify();
    }

    /// Annotate: the screen as it is now, still, to draw on.
    fn begin_annotation(&mut self, cx: &mut Context<Self>) {
        let device = self.simulator.device().map(|d| d.udid.clone());
        let Some((udid, image)) = self.simulator.frame.clone() else {
            return;
        };
        if device.as_deref() != Some(udid.as_str()) {
            return;
        }
        self.simulator.annotation = Some(Annotation {
            image,
            strokes: Vec::new(),
            drawing: false,
        });
        self.simulator.note = Some("Draw on the screen, then Add to Reply".into());
        cx.notify();
    }

    fn end_annotation(&mut self, cx: &mut Context<Self>) {
        if let Some(n) = self.simulator.annotation.take() {
            let current = self
                .simulator
                .frame
                .as_ref()
                .is_some_and(|(_, f)| Arc::ptr_eq(f, &n.image));
            if !current {
                cx.drop_image(n.image, None);
            }
        }
        self.simulator.note = None;
        cx.notify();
    }

    /// Where `at` (in the window) is on the screen, as shares of its size.
    fn on_screen(&self, at: gpui::Point<Pixels>) -> (f32, f32) {
        let b = self.simulator.screen_at.get();
        let w = f32::from(b.size.width).max(1.0);
        let h = f32::from(b.size.height).max(1.0);
        (
            ((f32::from(at.x) - f32::from(b.origin.x)) / w).clamp(0.0, 1.0),
            ((f32::from(at.y) - f32::from(b.origin.y)) / h).clamp(0.0, 1.0),
        )
    }

    fn pen_down(&mut self, at: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let p = self.on_screen(at);
        if let Some(n) = self.simulator.annotation.as_mut() {
            n.strokes.push(vec![p]);
            n.drawing = true;
            cx.notify();
        }
    }

    fn pen_move(&mut self, at: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let p = self.on_screen(at);
        if let Some(n) = self.simulator.annotation.as_mut().filter(|n| n.drawing) {
            if let Some(stroke) = n.strokes.last_mut() {
                stroke.push(p);
                cx.notify();
            }
        }
    }

    fn pen_up(&mut self) {
        if let Some(n) = self.simulator.annotation.as_mut() {
            n.drawing = false;
        }
    }

    /// Add to Reply: the still with its lines, saved under the data folder's `drops/`, its path
    /// in the reply box as a dropped file's.
    fn add_annotation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(n) = self.simulator.annotation.as_ref() else {
            return;
        };
        let name = self
            .simulator
            .device()
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "Simulator".into());
        let file = saved_name(Saved::Screenshot, &name, now_civil()).replacen(
            "Screenshot",
            "Annotation",
            1,
        );
        let dir = self.root.data_dir.join("drops").join(now_ms().to_string());
        let path = dir.join(file);
        let saved = annotated_png(&n.image, &n.strokes).and_then(|bytes| {
            std::fs::create_dir_all(&dir)
                .and_then(|_| std::fs::write(&path, bytes))
                .map_err(|e| e.to_string())
        });
        match saved {
            Ok(()) => {
                self.end_annotation(cx);
                self.drop_paths(&[path], window, cx);
                self.simulator.note = Some("Added to the reply".into());
            }
            Err(e) => self.simulator.note = Some(format!("Couldn't save it: {e}")),
        }
        cx.notify();
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

mod parts;
pub use parts::{button_in_header, render};
