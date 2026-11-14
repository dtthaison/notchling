use notchling_engine::pages::clock::civil_at;
use notchling_engine::versions::update::update_argv;
use notchling_engine::versions::{Method, Tool, Version};
use serde_json::Value;

pub const INSTALL_FIRST: &str = "Turn on Install in Settings first";
pub const NOT_CHECKED: &str = "Not checked yet";
pub const CHECKING: &str = "Checking…";
pub const CANT_TELL: &str = "Couldn't tell yet. Trying again in a couple of minutes.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateButton {
    pub enabled: bool,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckLine {
    pub text: String,
    pub failed: bool,
    pub details: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoBack {
    pub to: String,
    pub label: String,
    pub confirm: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extras {
    pub install_on: bool,
    pub offset_secs: i64,
}

fn version_of(value: Option<&Value>) -> Option<Version> {
    value.and_then(Value::as_str).and_then(Version::parse)
}

pub fn clock_text(at_ms: i64, offset_secs: i64) -> String {
    let civil = civil_at(at_ms as f64 / 1000.0, offset_secs);
    format!("{:02}:{:02}", civil.hour, civil.minute)
}

fn parts_text(tool: Tool) -> &'static str {
    match tool {
        Tool::Claude => "version, doctor, transcripts",
        Tool::Codex => "version, exec flags, transcripts",
        Tool::Agy => "version",
        Tool::Notchling => "its own check",
    }
}

fn unchecked_text(list: &[&str]) -> String {
    match list {
        [] => String::new(),
        [one] => format!("Not checked: {one}"),
        [rest @ .., last] => format!("Not checked: {} and {last}", rest.join(", ")),
    }
}

pub fn installed_version(row: &Value) -> Option<Version> {
    version_of(row.get("installed").and_then(|i| i.get("version")))
}

pub fn busy(row: &Value) -> bool {
    row.get("check")
        .and_then(|c| c.get("running"))
        .and_then(Value::as_bool)
        == Some(true)
        || row.get("updating").and_then(Value::as_bool) == Some(true)
}

pub fn latest_line(row: &Value) -> Option<String> {
    version_of(row.get("latest")).map(|v| format!("Latest {v}"))
}

pub fn update_button(tool: Tool, row: &Value, install_on: bool) -> Option<UpdateButton> {
    let installed = installed_version(row)?;
    let latest = version_of(row.get("latest"))?;
    if latest <= installed {
        return None;
    }
    let method = row
        .get("method")
        .cloned()
        .and_then(|m| serde_json::from_value::<Method>(m).ok())?;
    update_argv(tool, method, None).ok()?;
    Some(UpdateButton {
        enabled: install_on,
        note: (!install_on).then(|| INSTALL_FIRST.to_owned()),
    })
}

pub fn check_line(tool: Tool, row: &Value, offset_secs: i64) -> CheckLine {
    let plain = |text: &str| CheckLine {
        text: text.to_owned(),
        failed: false,
        details: None,
    };
    if busy(row) {
        return plain(CHECKING);
    }
    let Some(installed) = installed_version(row) else {
        return plain(NOT_CHECKED);
    };
    let reference = row.get("reference").filter(|r| r.is_object());
    let held = reference.and_then(|r| version_of(r.get("version")));
    let failed = row
        .get("failed")
        .and_then(Value::as_array)
        .and_then(|list| {
            list.iter()
                .find(|f| version_of(f.get("version")).as_ref() == Some(&installed))
        });
    if let Some(failed) = failed {
        let at = failed.get("at").and_then(Value::as_i64).unwrap_or(0);
        let why = failed
            .get("why")
            .and_then(Value::as_str)
            .map(|w| w.trim_end_matches('.').to_owned())
            .unwrap_or_else(|| "a part of the check didn't pass".to_owned());
        let stay = held
            .filter(|h| *h != installed)
            .map(|h| format!(" Workers stay on {h}."))
            .unwrap_or_default();
        return CheckLine {
            text: format!("Failed {}: {why}.{stay}", clock_text(at, offset_secs)),
            failed: true,
            details: None,
        };
    }
    let usable = row.get("usable").and_then(Value::as_bool) == Some(true);
    if let (Some(reference), true) = (reference, usable) {
        let at = reference.get("at").and_then(Value::as_i64).unwrap_or(0);
        let word = if reference.get("partly").and_then(Value::as_bool) == Some(false) {
            "Checked"
        } else {
            "Partly checked"
        };
        let unchecked: Vec<&str> = reference
            .get("unchecked")
            .and_then(Value::as_array)
            .map(|l| l.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let details = unchecked_text(&unchecked);
        return CheckLine {
            text: format!(
                "{word} {} · {}",
                clock_text(at, offset_secs),
                parts_text(tool)
            ),
            failed: false,
            details: (!details.is_empty()).then_some(details),
        };
    }
    let last = row
        .get("check")
        .and_then(|c| c.get("result"))
        .and_then(Value::as_str);
    if last == Some("cant_tell") {
        return plain(CANT_TELL);
    }
    plain(NOT_CHECKED)
}

pub fn soak_line(row: &Value) -> Option<(String, bool)> {
    let soak = row.get("soak").filter(|s| s.is_object())?;
    let line = soak.get("line").and_then(Value::as_str)?.to_owned();
    let red = soak.get("unknown").and_then(Value::as_u64).unwrap_or(0) > 0;
    Some((line, red))
}

pub fn go_back(tool: Tool, row: &Value) -> Option<GoBack> {
    let to = version_of(row.get("goBack"))?;
    let stay = version_of(row.get("reference").and_then(|r| r.get("version")))
        .map(|v| format!(" Workers stay on {v}."))
        .unwrap_or_default();
    Some(GoBack {
        label: format!("Go Back to {to}"),
        confirm: format!(
            "{} will update itself again at its next check.{stay}",
            tool.title()
        ),
        to: to.as_str().to_owned(),
    })
}
