//! The iOS Simulator panel's pure part: the simulators from `simctl list devices available -j`
//! (booted, and the ones the menu can boot), the runtimes and device types a new one can have,
//! the `simctl` commands the panel runs, the zoom and the phone's frame around the screen, the
//! keys Simulator takes for Home and Rotate, strokes drawn into a screenshot (Annotate), and the
//! names of the files it saves (Simulator's own: "Simulator Screenshot - iPhone 17 - 2026-11-10 at
//! 17.10.05.png"). The panel itself is `chat::view::simulator`.

use std::path::Path;

use notchling_engine::control::Command;
use notchling_engine::pages::clock::Civil;
use serde_json::Value;

/// `xcrun`, which finds `simctl` in the selected Xcode.
pub const XCRUN: &str = "/usr/bin/xcrun";

/// How many shut-down simulators the menu offers to boot.
pub const MENU_AVAILABLE: usize = 8;

/// One simulator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub udid: String,
    pub name: String,
    /// "iOS 26.5".
    pub runtime: String,
    pub booted: bool,
    /// `lastUsedAt` as simctl writes it (ISO 8601, so it sorts as text); empty when never used.
    pub last_used: String,
    /// The device type's id (`…SimDeviceType.iPhone-17`).
    pub kind: String,
}

impl Device {
    pub fn ipad(&self) -> bool {
        self.kind.contains("iPad")
    }

    /// The points one pixel of its screenshot is: 2 on an iPad, 3 on a phone.
    pub fn scale(&self) -> f32 {
        if self.ipad() {
            2.0
        } else {
            3.0
        }
    }
}

/// The simulators in a `simctl list devices … -j` answer, by name; nothing for an answer that
/// isn't one.
pub fn devices(json: &str) -> Vec<Device> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(runtimes) = v.get("devices").and_then(Value::as_object) else {
        return Vec::new();
    };
    let text = |d: &Value, k: &str| d.get(k).and_then(Value::as_str).map(str::to_owned);
    let mut devices: Vec<Device> = runtimes
        .iter()
        .flat_map(|(runtime, list)| {
            list.as_array()
                .into_iter()
                .flatten()
                .filter(|d| d.get("isAvailable").and_then(Value::as_bool) != Some(false))
                .filter_map(move |d| {
                    Some(Device {
                        udid: text(d, "udid")?,
                        name: text(d, "name")?,
                        runtime: runtime_name(runtime),
                        booted: text(d, "state").as_deref() == Some("Booted"),
                        last_used: text(d, "lastUsedAt").unwrap_or_default(),
                        kind: text(d, "deviceTypeIdentifier").unwrap_or_default(),
                    })
                })
        })
        .collect();
    devices.sort_by(|a, b| a.name.cmp(&b.name).then(a.udid.cmp(&b.udid)));
    devices
}

/// The menu's two sections: the booted simulators, and the shut-down ones it can boot (phones and
/// iPads, most recently used first, [`MENU_AVAILABLE`] at most).
pub fn menu(all: &[Device]) -> (Vec<Device>, Vec<Device>) {
    let booted = all.iter().filter(|d| d.booted).cloned().collect();
    let mut available: Vec<Device> = all
        .iter()
        .filter(|d| !d.booted && d.runtime.starts_with("iOS"))
        .cloned()
        .collect();
    available.sort_by(|a, b| b.last_used.cmp(&a.last_used).then(a.name.cmp(&b.name)));
    available.truncate(MENU_AVAILABLE);
    (booted, available)
}

/// `com.apple.CoreSimulator.SimRuntime.iOS-26-5` as "iOS 26.5"; an id of another shape as itself.
pub fn runtime_name(id: &str) -> String {
    let Some(tail) = id.rsplit('.').next().filter(|t| t.contains('-')) else {
        return id.to_owned();
    };
    let mut parts = tail.split('-');
    let platform = match parts.next().unwrap_or("") {
        "xrOS" => "visionOS",
        other => other,
    };
    let version: Vec<&str> = parts.collect();
    format!("{platform} {}", version.join("."))
}

/// A runtime a new simulator can have, with the phones and iPads it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Runtime {
    pub id: String,
    /// "iOS 26.5".
    pub name: String,
    /// (name, id), as Apple lists them.
    pub types: Vec<(String, String)>,
}

/// The iOS runtimes in `simctl list runtimes available -j`, newest first.
pub fn runtimes(json: &str) -> Vec<Runtime> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let text = |d: &Value, k: &str| d.get(k).and_then(Value::as_str).map(str::to_owned);
    let mut list: Vec<Runtime> = v
        .get("runtimes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|r| text(r, "platform").as_deref() == Some("iOS"))
        .filter(|r| r.get("isAvailable").and_then(Value::as_bool) != Some(false))
        .filter_map(|r| {
            let types = r
                .get("supportedDeviceTypes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|t| {
                    matches!(
                        text(t, "productFamily").as_deref(),
                        Some("iPhone") | Some("iPad")
                    )
                })
                .filter_map(|t| Some((text(t, "name")?, text(t, "identifier")?)))
                .collect();
            Some(Runtime {
                id: text(r, "identifier")?,
                name: text(r, "name")?,
                types,
            })
        })
        .collect();
    list.reverse();
    list
}

fn simctl(args: &[&str]) -> Command {
    let mut argv = vec![XCRUN, "simctl"];
    argv.extend_from_slice(args);
    Command::new(&argv)
}

pub fn list_command() -> Command {
    simctl(&["list", "devices", "available", "-j"])
}

pub fn runtimes_command() -> Command {
    simctl(&["list", "runtimes", "available", "-j"])
}

pub fn boot_command(udid: &str) -> Command {
    simctl(&["boot", udid])
}

pub fn shutdown_command(udid: &str) -> Command {
    simctl(&["shutdown", udid])
}

/// Photos and videos into the device's library.
pub fn add_media_command(udid: &str, paths: &[impl AsRef<Path>]) -> Command {
    let paths: Vec<String> = paths
        .iter()
        .map(|p| p.as_ref().to_string_lossy().into_owned())
        .collect();
    let mut args = vec!["addmedia", udid];
    args.extend(paths.iter().map(String::as_str));
    simctl(&args)
}

/// A new simulator; its output is the new udid.
pub fn create_command(name: &str, kind: &str, runtime: &str) -> Command {
    simctl(&["create", name, kind, runtime])
}

/// One frame of `udid`'s screen, as JPEG (smaller and quicker for the live view) or PNG.
pub fn screenshot_command(udid: &str, path: &Path, jpeg: bool) -> Command {
    let kind = if jpeg { "--type=jpeg" } else { "--type=png" };
    simctl(&["io", udid, "screenshot", kind, &path.to_string_lossy()])
}

/// A recording of `udid`'s screen to `path`, until it gets SIGINT.
pub fn record_args(udid: &str, path: &Path) -> Vec<String> {
    [
        "simctl",
        "io",
        udid,
        "recordVideo",
        "--codec=h264",
        "--force",
        &path.to_string_lossy(),
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

/// What Simulator does on a key of its own, since `simctl` can't press Home or turn the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimKey {
    /// ⇧⌘H.
    Home,
    /// ⌘→: Rotate Right.
    Rotate,
}

/// The AppleScript that brings the device's Simulator window forward and presses `key` there.
/// Simulator names its windows "<device> – <runtime>"; a name's quotes and backslashes are escaped.
pub fn key_script(device: &str, key: SimKey) -> String {
    let name: String = device
        .chars()
        .flat_map(|c| match c {
            '"' | '\\' => vec!['\\', c],
            c => vec![c],
        })
        .collect();
    let press = match key {
        SimKey::Home => r#"keystroke "h" using {command down, shift down}"#,
        SimKey::Rotate => "key code 124 using {command down}",
    };
    format!(
        "tell application \"Simulator\" to activate\n\
         tell application \"System Events\" to tell process \"Simulator\"\n\
         \tset found to (windows whose name starts with \"{name}\")\n\
         \tif (count of found) > 0 then perform action \"AXRaise\" of item 1 of found\n\
         \t{press}\n\
         end tell"
    )
}

/// How big the screen is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zoom {
    /// As big as the panel lets it.
    Fit,
    /// A share of the device's own size in points (1 = actual size).
    Times(f32),
}

/// The zoom steps − and + go through.
pub const STEPS: [f32; 9] = [0.25, 0.33, 0.5, 0.67, 0.75, 1.0, 1.25, 1.5, 2.0];

impl Zoom {
    /// The screen's size in the panel: `pixels` is the screenshot's, `scale` its points per pixel,
    /// `room` the space the phone has (its frame included, `frame` points a side).
    pub fn size(self, pixels: (f32, f32), scale: f32, room: (f32, f32), frame: f32) -> (f32, f32) {
        let points = (pixels.0 / scale, pixels.1 / scale);
        let times = match self {
            Zoom::Fit => fit_times(points, room, frame),
            Zoom::Times(t) => t,
        };
        (points.0 * times, points.1 * times)
    }

    /// The next step bigger (`up`) or smaller, from what it shows now.
    pub fn step(
        self,
        up: bool,
        pixels: (f32, f32),
        scale: f32,
        room: (f32, f32),
        frame: f32,
    ) -> Zoom {
        let now = match self {
            Zoom::Fit => fit_times((pixels.0 / scale, pixels.1 / scale), room, frame),
            Zoom::Times(t) => t,
        };
        let next = if up {
            STEPS.iter().copied().find(|s| *s > now + 0.005)
        } else {
            STEPS.iter().rev().copied().find(|s| *s < now - 0.005)
        };
        Zoom::Times(next.unwrap_or(if up { STEPS[8] } else { STEPS[0] }))
    }
}

fn fit_times(points: (f32, f32), room: (f32, f32), frame: f32) -> f32 {
    let w = (room.0 - 2.0 * frame).max(1.0) / points.0.max(1.0);
    let h = (room.1 - 2.0 * frame).max(1.0) / points.1.max(1.0);
    w.min(h).max(0.05)
}

/// The phone's frame for a screen `width` wide: the bezel's thickness and the screen's corner.
pub fn frame_for(width: f32, ipad: bool) -> (f32, f32) {
    if ipad {
        ((width * 0.035).clamp(6.0, 22.0), width * 0.03)
    } else {
        ((width * 0.04).clamp(5.0, 16.0), width * 0.135)
    }
}

/// Burns `strokes` (each a line through points, as shares 0–1 of the image's width and height)
/// into a BGRA image `width` × `height`, `thick` pixels wide, in `color` (B, G, R).
pub fn burn(
    bgra: &mut [u8],
    width: usize,
    height: usize,
    strokes: &[Vec<(f32, f32)>],
    thick: f32,
    color: (u8, u8, u8),
) {
    let r = (thick / 2.0).max(0.5);
    let mut dot = |cx: f32, cy: f32| {
        let (x0, x1) = ((cx - r).floor().max(0.0) as usize, (cx + r).ceil() as usize);
        let (y0, y1) = ((cy - r).floor().max(0.0) as usize, (cy + r).ceil() as usize);
        for y in y0..y1.min(height) {
            for x in x0..x1.min(width) {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= r * r {
                    let i = (y * width + x) * 4;
                    bgra[i] = color.0;
                    bgra[i + 1] = color.1;
                    bgra[i + 2] = color.2;
                    bgra[i + 3] = 255;
                }
            }
        }
    };
    for stroke in strokes {
        let pts: Vec<(f32, f32)> = stroke
            .iter()
            .map(|(x, y)| (x * width as f32, y * height as f32))
            .collect();
        if let [only] = pts.as_slice() {
            dot(only.0, only.1);
        }
        for pair in pts.windows(2) {
            let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
            let steps = ((bx - ax).hypot(by - ay) / (r / 2.0).max(0.5))
                .ceil()
                .max(1.0) as usize;
            for s in 0..=steps {
                let t = s as f32 / steps as f32;
                dot(ax + (bx - ax) * t, ay + (by - ay) * t);
            }
        }
    }
}

/// What a file the panel saves holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saved {
    Screenshot,
    Recording,
}

/// The file's name, as Simulator names its own: the device's name with no `/` or `:` in it.
pub fn saved_name(kind: Saved, device: &str, at: Civil) -> String {
    let (what, ext) = match kind {
        Saved::Screenshot => ("Screenshot", "png"),
        Saved::Recording => ("Recording", "mov"),
    };
    let device: String = device
        .chars()
        .map(|c| if matches!(c, '/' | ':') { '-' } else { c })
        .collect();
    format!(
        "Simulator {what} - {device} - {:04}-{:02}-{:02} at {:02}.{:02}.{:02}.{ext}",
        at.year, at.month, at.day, at.hour, at.minute, at.second
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"{
      "devices" : {
        "com.apple.CoreSimulator.SimRuntime.iOS-27-0" : [],
        "com.apple.CoreSimulator.SimRuntime.iOS-26-5" : [
          {"udid" : "8043", "state" : "Booted", "name" : "Lingeva Live",
           "lastUsedAt" : "2026-11-09T06:09:57Z", "isAvailable" : true,
           "deviceTypeIdentifier" : "com.apple.CoreSimulator.SimDeviceType.iPhone-17"},
          {"udid" : "9999", "state" : "Shutdown", "name" : "Lingeva Revamp",
           "lastUsedAt" : "2026-11-10T09:19:15Z", "isAvailable" : true,
           "deviceTypeIdentifier" : "com.apple.CoreSimulator.SimDeviceType.iPhone-17"},
          {"udid" : "1111", "state" : "Shutdown", "name" : "fix-grammar-snap",
           "lastUsedAt" : "2026-11-09T05:59:58Z",
           "deviceTypeIdentifier" : "com.apple.CoreSimulator.SimDeviceType.iPad-Air-11-inch-M3"},
          {"udid" : "0000", "state" : "Shutdown", "name" : "Gone", "isAvailable" : false}
        ],
        "com.apple.CoreSimulator.SimRuntime.watchOS-26-5" : [
          {"udid" : "77", "state" : "Shutdown", "name" : "Apple Watch"}
        ]
      }
    }"#;

    #[test]
    fn the_simulators_by_name_with_their_runtimes_and_state() {
        let all = devices(LIST);
        let names: Vec<&str> = all.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Apple Watch",
                "Lingeva Live",
                "Lingeva Revamp",
                "fix-grammar-snap"
            ],
            "an unavailable one is left out"
        );
        let live = &all[1];
        assert!(live.booted && live.runtime == "iOS 26.5" && live.udid == "8043");
        assert_eq!(all[0].runtime, "watchOS 26.5");
        assert!(devices("").is_empty());
        assert!(devices(r#"{"devices": {}}"#).is_empty());
        assert!(devices("error: xcrun not found").is_empty());
    }

    #[test]
    fn the_menu_has_the_booted_then_the_recent_ones_it_can_boot() {
        let (booted, available) = menu(&devices(LIST));
        assert_eq!(booted.len(), 1);
        assert_eq!(booted[0].name, "Lingeva Live");
        let names: Vec<&str> = available.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            ["Lingeva Revamp", "fix-grammar-snap"],
            "most recent first, no watch"
        );
        let many: Vec<Device> = (0..20)
            .map(|i| Device {
                udid: i.to_string(),
                name: format!("d{i}"),
                runtime: "iOS 26.5".into(),
                booted: false,
                last_used: String::new(),
                kind: String::new(),
            })
            .collect();
        assert_eq!(menu(&many).1.len(), MENU_AVAILABLE);
    }

    #[test]
    fn phones_and_ipads_are_told_apart() {
        let all = devices(LIST);
        let phone = &all[1];
        let ipad = &all[3];
        // The screenshot has the Dynamic Island in it already: only the scale differs.
        assert!(!phone.ipad() && phone.scale() == 3.0);
        assert!(ipad.ipad() && ipad.scale() == 2.0);
    }

    #[test]
    fn runtime_ids_read_as_names() {
        assert_eq!(
            runtime_name("com.apple.CoreSimulator.SimRuntime.iOS-26-5"),
            "iOS 26.5"
        );
        assert_eq!(
            runtime_name("com.apple.CoreSimulator.SimRuntime.xrOS-3-0"),
            "visionOS 3.0"
        );
        assert_eq!(runtime_name("weird"), "weird");
    }

    #[test]
    fn a_new_simulator_can_have_the_ios_runtimes_phones_and_ipads() {
        let json = r#"{"runtimes": [
          {"name": "iOS 26.5", "identifier": "rt.iOS-26-5", "platform": "iOS", "isAvailable": true,
           "supportedDeviceTypes": [
             {"name": "iPhone 17 Pro", "identifier": "t.iPhone-17-Pro", "productFamily": "iPhone"},
             {"name": "iPad Air", "identifier": "t.iPad-Air", "productFamily": "iPad"},
             {"name": "Apple TV", "identifier": "t.tv", "productFamily": "Apple TV"}]},
          {"name": "iOS 27.0", "identifier": "rt.iOS-27-0", "platform": "iOS", "isAvailable": true,
           "supportedDeviceTypes": []},
          {"name": "watchOS 26.5", "identifier": "rt.w", "platform": "watchOS",
           "supportedDeviceTypes": []}]}"#;
        let list = runtimes(json);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "iOS 27.0", "newest first");
        assert_eq!(
            list[1].types,
            vec![
                ("iPhone 17 Pro".to_owned(), "t.iPhone-17-Pro".to_owned()),
                ("iPad Air".to_owned(), "t.iPad-Air".to_owned()),
            ]
        );
        assert!(runtimes("nope").is_empty());
    }

    #[test]
    fn the_commands_name_the_device_and_file() {
        let shot = screenshot_command("8043", Path::new("/tmp/f.jpg"), true);
        assert_eq!(
            shot.argv,
            [
                XCRUN,
                "simctl",
                "io",
                "8043",
                "screenshot",
                "--type=jpeg",
                "/tmp/f.jpg"
            ]
        );
        assert_eq!(
            screenshot_command("8043", Path::new("/d/a.png"), false).argv[5],
            "--type=png"
        );
        assert_eq!(
            record_args("8043", Path::new("/d/r.mov")),
            [
                "simctl",
                "io",
                "8043",
                "recordVideo",
                "--codec=h264",
                "--force",
                "/d/r.mov"
            ]
        );
        assert_eq!(
            list_command().argv,
            [XCRUN, "simctl", "list", "devices", "available", "-j"]
        );
        assert_eq!(boot_command("9").argv, [XCRUN, "simctl", "boot", "9"]);
        assert_eq!(
            shutdown_command("9").argv,
            [XCRUN, "simctl", "shutdown", "9"]
        );
        assert_eq!(
            add_media_command("9", &[Path::new("/a b.png"), Path::new("/c.mov")]).argv,
            [XCRUN, "simctl", "addmedia", "9", "/a b.png", "/c.mov"]
        );
        assert_eq!(
            create_command("My Phone", "t.iPhone", "rt.iOS").argv,
            [XCRUN, "simctl", "create", "My Phone", "t.iPhone", "rt.iOS"]
        );
        assert_eq!(
            runtimes_command().argv[2..],
            ["list", "runtimes", "available", "-j"]
        );
    }

    #[test]
    fn home_and_rotate_are_simulators_own_keys_in_the_devices_window() {
        let home = key_script("Lingeva \"Live\"", SimKey::Home);
        assert!(home.contains(r#"starts with "Lingeva \"Live\"""#), "{home}");
        assert!(home.contains(r#"keystroke "h" using {command down, shift down}"#));
        assert!(home.starts_with("tell application \"Simulator\" to activate"));
        let rotate = key_script("A\\B", SimKey::Rotate);
        assert!(rotate.contains(r#""A\\B""#), "{rotate}");
        assert!(rotate.contains("key code 124 using {command down}"));
    }

    #[test]
    fn fit_fills_the_room_and_steps_go_from_what_shows() {
        // A 1206 × 2622 phone is 402 × 874 points.
        let px = (1206.0, 2622.0);
        let (w, h) = Zoom::Fit.size(px, 3.0, (420.0, 1000.0), 10.0);
        assert!(
            (w - 400.0).abs() < 0.5 && (h - 869.6).abs() < 1.0,
            "{w} {h}"
        );
        let (w, h) = Zoom::Fit.size(px, 3.0, (1000.0, 457.0), 10.0);
        assert!((h - 437.0).abs() < 0.5 && w < 1000.0, "{w} {h}");
        assert_eq!(
            Zoom::Times(1.0).size(px, 3.0, (10.0, 10.0), 0.0),
            (402.0, 874.0)
        );
        // From a fit of about 0.995: + is 1.25 (1 is what shows), − is 0.75.
        let fit = Zoom::Fit;
        assert_eq!(
            fit.step(true, px, 3.0, (420.0, 1000.0), 10.0),
            Zoom::Times(1.25)
        );
        assert_eq!(
            fit.step(false, px, 3.0, (420.0, 1000.0), 10.0),
            Zoom::Times(0.75)
        );
        assert_eq!(
            Zoom::Times(2.0).step(true, px, 3.0, (1.0, 1.0), 0.0),
            Zoom::Times(2.0)
        );
        assert_eq!(
            Zoom::Times(0.25).step(false, px, 3.0, (1.0, 1.0), 0.0),
            Zoom::Times(0.25)
        );
        assert_eq!(
            Zoom::Times(0.5).step(true, px, 3.0, (1.0, 1.0), 0.0),
            Zoom::Times(0.67)
        );
    }

    #[test]
    fn the_frame_follows_the_screen_size() {
        let (bezel, corner) = frame_for(400.0, false);
        assert_eq!(bezel, 16.0);
        assert!((corner - 54.0).abs() < 0.01);
        assert_eq!(frame_for(50.0, false).0, 5.0, "never thinner than 5");
        assert!(frame_for(800.0, true).1 < frame_for(800.0, false).1);
    }

    #[test]
    fn strokes_burn_into_the_pixels_they_cross_only() {
        let (w, h) = (20, 10);
        let mut img = vec![0u8; w * h * 4];
        burn(
            &mut img,
            w,
            h,
            &[vec![(0.0, 0.5), (1.0, 0.5)]],
            2.0,
            (1, 2, 3),
        );
        let at = |x: usize, y: usize| &img[(y * w + x) * 4..(y * w + x) * 4 + 4];
        assert_eq!(at(10, 5), [1, 2, 3, 255]);
        assert_eq!(at(0, 4), [1, 2, 3, 255]);
        assert_eq!(at(10, 0), [0, 0, 0, 0], "far from the line");
        assert_eq!(at(10, 9), [0, 0, 0, 0]);
        // One tap is a dot.
        let mut dot = vec![0u8; w * h * 4];
        burn(&mut dot, w, h, &[vec![(0.5, 0.5)]], 4.0, (9, 9, 9));
        assert_eq!(dot[(5 * w + 10) * 4], 9);
        // Off the image draws nothing and doesn't panic.
        let mut off = vec![0u8; w * h * 4];
        burn(
            &mut off,
            w,
            h,
            &[vec![(2.0, 2.0), (3.0, 3.0)]],
            4.0,
            (9, 9, 9),
        );
        assert!(off.iter().all(|b| *b == 0));
    }

    #[test]
    fn saved_files_are_named_as_simulator_names_them() {
        let at = Civil {
            year: 2026,
            month: 11,
            day: 10,
            hour: 17,
            minute: 4,
            second: 5,
        };
        assert_eq!(
            saved_name(Saved::Screenshot, "Lingeva Live", at),
            "Simulator Screenshot - Lingeva Live - 2026-11-10 at 17.04.05.png"
        );
        assert_eq!(
            saved_name(Saved::Recording, "A/B: C", at),
            "Simulator Recording - A-B- C - 2026-11-10 at 17.04.05.mov"
        );
    }
}
