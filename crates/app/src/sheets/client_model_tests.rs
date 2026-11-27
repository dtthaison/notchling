use notchling_engine::versions::{Mark, Tool};
use serde_json::{json, Value};

use super::client_model::{client_part, ClientPart};

fn get() -> Value {
    json!({"tools": [
        {"tool": "claude", "installed": {"version": "2.1.324", "sha256": "ab"}, "method": "native",
         "path": "/Users/me/.local/bin/claude", "problems": [], "running": [], "reference": null,
         "usable": false, "failed": [], "check": null},
        {"tool": "codex", "installed": null, "method": null, "path": null},
        {"tool": "antigravity", "installed": {"version": null, "sha256": null}, "method": "standalone",
         "path": "/Users/me/.local/bin/agy"},
        {"tool": "notchling", "installed": {"version": "0.9.0"}, "method": "unknown", "path": "/Applications/Notchling.app/Contents/MacOS/notchling"},
    ]})
}

fn workers() -> Value {
    json!({"workers": {
        "m1": {"tools": {
            "claude": {"state": "installed", "version": "2.1.324", "method": "native", "path": "/home/w/.local/bin/claude"},
            "codex": {"state": "notInstalled"},
        }, "atMs": 5},
        "ub-3": {"tools": {
            "claude": {"state": "installed", "version": "2.1.320", "method": "native", "path": "/home/w/.local/bin/claude"},
        }, "atMs": 5},
        "bad": "nonsense",
    }})
}

const HOME: &str = "/Users/me";

#[test]
fn found_line_names_path_version_and_method() {
    let part = client_part(Tool::Claude, &get(), &workers(), HOME);
    assert_eq!(
        part.found,
        "Found at ~/.local/bin/claude · 2.1.324 · native installer"
    );
    assert_eq!(
        client_part(Tool::Codex, &get(), &workers(), HOME).found,
        "Not found"
    );
}

#[test]
fn home_is_shown_as_tilde() {
    let part = client_part(Tool::Notchling, &get(), &workers(), HOME);
    assert_eq!(
        part.found,
        "Found at /Applications/Notchling.app/Contents/MacOS/notchling · 0.9.0"
    );
    let other = client_part(Tool::Claude, &get(), &workers(), "/Users/meh");
    assert!(other.found.contains("/Users/me/.local/bin/claude"));
    let root = client_part(Tool::Claude, &get(), &workers(), "/");
    assert!(root.found.contains("/Users/me/.local"));
}

#[test]
fn agy_without_version_says_unknown() {
    let part = client_part(Tool::Agy, &get(), &workers(), HOME);
    assert_eq!(
        part.found,
        "Found at ~/.local/bin/agy · version unknown · standalone installer"
    );
}

#[test]
fn worker_lines_follow_marks() {
    let part = client_part(Tool::Claude, &get(), &workers(), HOME);
    let lines: Vec<(&str, &str, Mark, &str)> = part
        .workers
        .iter()
        .map(|l| (l.machine.as_str(), l.text.as_str(), l.mark, l.state))
        .collect();
    assert_eq!(
        lines,
        [
            ("m1", "2.1.324", Mark::Same, "Same as the Mac"),
            ("ub-3", "2.1.320", Mark::Behind, "Behind the Mac"),
        ]
    );
    let codex = client_part(Tool::Codex, &get(), &workers(), HOME);
    assert_eq!(codex.workers.len(), 1);
    assert_eq!(codex.workers[0].text, "Not installed");
    assert_eq!(codex.workers[0].mark, Mark::NotInstalled);
}

#[test]
fn no_daemon_says_not_running() {
    let part = client_part(Tool::Claude, &json!(null), &json!(null), HOME);
    assert_eq!(part, ClientPart::not_running());
    assert_eq!(part.found, "Notchling isn't running");
    assert!(part.workers.is_empty());
}
