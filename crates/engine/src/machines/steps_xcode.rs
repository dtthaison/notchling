//! Step 8 of the catalog: Xcode (Swift `MachineSteps`, Xcode). Split from `steps_machine`.

use super::registry::MachineAbilities;
use super::step::{CheckResult, Detected, RemoteOutput, StepContext};
use super::steps_machine::{is_newline, shown, trim_ws};
use std::cmp::Ordering;

// MARK: 8 Xcode

/// Without the Command Line Tools `xcodebuild` and `xcrun` are stubs that open an install dialog,
/// so nothing runs then. `simctl list devices iPhone` keeps only the iPhones in the JSON (the full
/// list can pass the 64 KiB cap with several runtimes); each device's `isAvailable` is still read
/// from it.
pub(super) const XCODE_SCRIPT: &str = r#"if ! xcode-select -p >/dev/null 2>&1; then echo @@noclt; exit 0; fi
xcodebuild -version | head -1
echo @@runtimes
xcrun simctl list runtimes available 2>/dev/null | grep '^iOS '
echo @@devices
xcrun simctl list devices iPhone -j 2>/dev/null"#;

/// The engine's batch wrapper puts a `head -c 65536` on each probe's output on the machine; output
/// that reaches it was cut there.
const OUTPUT_CAP: usize = 65_536;
const CUT_MARKER: &str = "[earlier output cut]";

/// Compares as Foundation's `.numeric`: runs of digits by value, the rest by text.
fn numeric_cmp(a: &str, b: &str) -> Ordering {
    fn chunks(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() {
                Some((d, text)) if *d == digit => text.push(c),
                _ => out.push((digit, c.to_string())),
            }
        }
        out
    }
    let (left, right) = (chunks(a), chunks(b));
    for (x, y) in left.iter().zip(right.iter()) {
        let order = if x.0 && y.0 {
            let (xs, ys) = (x.1.trim_start_matches('0'), y.1.trim_start_matches('0'));
            xs.len().cmp(&ys.len()).then_with(|| xs.cmp(ys))
        } else {
            x.1.cmp(&y.1)
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    left.len().cmp(&right.len())
}

/// `26.5` or `26.5.1`: digits and dots only.
fn major_minor_or_more(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// Output that was cut, or sections that can't be read, say nothing about the machine: "can't
/// tell", and the abilities already saved stay as they are.
pub(super) fn parse_xcode(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    let unreadable =
        || CheckResult::cant_tell("the Xcode check's output was too long or not readable");
    if out.text.starts_with(CUT_MARKER) || out.text.len() >= OUTPUT_CAP {
        return unreadable();
    }
    let text = out.text.replace("\r\n", "\n");
    let all_lines: Vec<&str> = text.split(is_newline).collect();
    if all_lines.contains(&"@@noclt") {
        return CheckResult::fail("Xcode is not installed: the Command Line Tools are missing");
    }
    let mask = |s: &str| ctx.mask(s);

    let mut xcode_line: Option<String> = None;
    let mut runtime_lines: Vec<&str> = Vec::new();
    let mut devices_json = String::new();
    let mut section = 0; // 0 head, 1 runtimes, 2 devices
    let (mut saw_runtimes, mut saw_devices) = (false, false);
    for line in &all_lines {
        if *line == "@@runtimes" {
            section = 1;
            saw_runtimes = true;
            continue;
        }
        if *line == "@@devices" {
            section = 2;
            saw_devices = true;
            continue;
        }
        match section {
            0 => {
                if xcode_line.is_none() && line.starts_with("Xcode ") {
                    xcode_line = Some(shown(trim_ws(line), &mask));
                }
            }
            1 => runtime_lines.push(line),
            _ => {
                devices_json.push_str(line);
                devices_json.push('\n');
            }
        }
    }
    if !saw_runtimes || !saw_devices {
        // no marker at all: the probe did not run; markers missing next to an Xcode line: part of
        // it is gone
        return unreadable();
    }

    // iOS runtimes from `simctl list runtimes available | grep '^iOS '`:
    // `iOS 26.5 (26.5 - 23F77) - com.apple...`
    let mut runtimes: Vec<(String, String)> = Vec::new();
    for line in runtime_lines.iter().filter(|l| l.starts_with("iOS ")) {
        let Some(open) = line.find(" (") else {
            return unreadable();
        };
        let version = line[open + 2..]
            .split([' ', ')'])
            .find(|w| !w.is_empty())
            .unwrap_or("");
        if !major_minor_or_more(version) {
            return unreadable();
        }
        runtimes.push((shown(&line[..open], &mask), version.to_string()));
    }
    runtimes.sort_by(|a, b| numeric_cmp(&b.1, &a.1));

    // iPhone names under iOS runtimes, the newest runtime first, without repeats
    let mut phones: Vec<String> = Vec::new();
    if !devices_json.trim().is_empty() {
        let Ok(root) = serde_json::from_str::<serde_json::Value>(&devices_json) else {
            return unreadable();
        };
        let Some(by_runtime) = root.get("devices").and_then(|d| d.as_object()) else {
            return unreadable();
        };
        let mut keys: Vec<&String> = by_runtime.keys().filter(|k| k.contains("iOS")).collect();
        keys.sort_by(|a, b| numeric_cmp(b, a));
        for key in keys {
            let Some(items) = by_runtime[key].as_array() else {
                continue;
            };
            if !items.iter().all(|i| i.is_object()) {
                continue;
            }
            for item in items {
                let Some(name) = item.get("name").and_then(|n| n.as_str()) else {
                    continue;
                };
                if !name.starts_with("iPhone")
                    || item.get("isAvailable").and_then(|a| a.as_bool()) == Some(false)
                {
                    continue;
                }
                let phone = shown(name, &mask);
                if !phones.contains(&phone) {
                    phones.push(phone);
                }
            }
        }
    }

    let mut abilities: MachineAbilities = ctx.machine.abilities.clone().unwrap_or_default();
    abilities.xcode = xcode_line.clone();
    abilities.ios_runtimes = runtimes.iter().map(|r| r.0.clone()).collect();
    abilities.simulators = phones.clone();
    let detected = Detected {
        abilities: Some(abilities),
        ..Detected::default()
    };

    let Some(xcode_line) = xcode_line else {
        return CheckResult::fail("Xcode is not installed").detected(detected);
    };
    let Some(runtime) = runtimes.first() else {
        return CheckResult::fail("no iOS runtime").detected(detected);
    };
    let Some(phone) = phones.first() else {
        return CheckResult::fail("no iPhone simulator").detected(detected);
    };
    let more = if phones.len() > 1 {
        format!(" and {} more", phones.len() - 1)
    } else {
        String::new()
    };
    CheckResult::pass(format!("{xcode_line} · {} · {phone}{more}", runtime.0)).detected(detected)
}
