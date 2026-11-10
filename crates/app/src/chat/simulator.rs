//! The iOS Simulator panel's pure part: the booted devices from `simctl list devices booted -j`,
//! their runtimes' names ("iOS 26.5"), the `simctl` commands the panel runs, and the names of the
//! screenshots and recordings it saves (Simulator's own: "Simulator Screenshot - iPhone 17 -
//! 2026-11-10 at 17.10.05.png"). The panel itself is `chat::view::simulator`.

use std::path::Path;

use notchling_engine::control::Command;
use notchling_engine::pages::clock::Civil;
use serde_json::Value;

/// `xcrun`, which finds `simctl` in the selected Xcode.
pub const XCRUN: &str = "/usr/bin/xcrun";

/// One booted simulator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub udid: String,
    pub name: String,
    /// "iOS 26.5".
    pub runtime: String,
}

/// The booted devices in `simctl list devices booted -j`'s answer, by name; nothing for an answer
/// that isn't one.
pub fn booted(json: &str) -> Vec<Device> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(runtimes) = v.get("devices").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut devices: Vec<Device> = runtimes
        .iter()
        .flat_map(|(runtime, list)| {
            list.as_array()
                .into_iter()
                .flatten()
                .filter(|d| d.get("state").and_then(Value::as_str) == Some("Booted"))
                .filter_map(move |d| {
                    Some(Device {
                        udid: d.get("udid")?.as_str()?.to_owned(),
                        name: d.get("name")?.as_str()?.to_owned(),
                        runtime: runtime_name(runtime),
                    })
                })
        })
        .collect();
    devices.sort_by(|a, b| a.name.cmp(&b.name).then(a.udid.cmp(&b.udid)));
    devices
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

pub fn list_command() -> Command {
    Command::new(&[XCRUN, "simctl", "list", "devices", "booted", "-j"])
}

/// One frame of `udid`'s screen, as JPEG (smaller and quicker for the live view) or PNG.
pub fn screenshot_command(udid: &str, path: &Path, jpeg: bool) -> Command {
    let kind = if jpeg { "--type=jpeg" } else { "--type=png" };
    Command::new(&[
        XCRUN,
        "simctl",
        "io",
        udid,
        "screenshot",
        kind,
        &path.to_string_lossy(),
    ])
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
          {"udid" : "8043", "state" : "Booted", "name" : "Lingeva Live"},
          {"udid" : "9999", "state" : "Shutdown", "name" : "Old"}
        ],
        "com.apple.CoreSimulator.SimRuntime.watchOS-26-5" : [
          {"udid" : "77", "state" : "Booted", "name" : "Apple Watch"}
        ]
      }
    }"#;

    #[test]
    fn the_booted_devices_by_name_with_their_runtimes() {
        assert_eq!(
            booted(LIST),
            vec![
                Device {
                    udid: "77".into(),
                    name: "Apple Watch".into(),
                    runtime: "watchOS 26.5".into(),
                },
                Device {
                    udid: "8043".into(),
                    name: "Lingeva Live".into(),
                    runtime: "iOS 26.5".into(),
                },
            ]
        );
        assert!(booted("").is_empty());
        assert!(booted(r#"{"devices": {}}"#).is_empty());
        assert!(booted("error: xcrun not found").is_empty());
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
            [XCRUN, "simctl", "list", "devices", "booted", "-j"]
        );
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
