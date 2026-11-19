use super::model::{BrowserFault, BrowserStep, BrowserVerb, ToolCall, ToolImage, ToolKind};
use super::text::prefix_chars;
use super::util::str_of;
use base64::Engine as _;
use serde_json::{Map, Value};

pub const PREFIX: &str = "mcp__claude-in-chrome__";
pub const KEEP_IMAGE_CALLS: usize = 8;
const SUBJECT_CHARS: usize = 80;
const BATCH_MAX: usize = 60;

pub fn is_browser(name: &str) -> bool {
    name.starts_with(PREFIX)
}

pub fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next()?;
    let host = host_port.split(':').next()?;
    if host.is_empty() || url.contains(' ') {
        None
    } else {
        Some(host.to_string())
    }
}

fn text_in(input: &Map<String, Value>, key: &str) -> String {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(|s| prefix_chars(s.trim(), SUBJECT_CHARS).to_string())
        .unwrap_or_default()
}

fn step(verb: BrowserVerb, subject: String) -> BrowserStep {
    BrowserStep {
        verb,
        subject,
        count: 0,
    }
}

fn address(input: &Map<String, Value>) -> String {
    let url = text_in(input, "url");
    url_host(&url).unwrap_or(url)
}

fn computer(input: &Map<String, Value>) -> BrowserStep {
    let summary = text_in(input, "action_summary");
    let (verb, subject) = match input.get("action").and_then(Value::as_str).unwrap_or("") {
        "left_click" | "right_click" | "double_click" | "triple_click" => {
            (BrowserVerb::Click, summary)
        }
        "type" => (BrowserVerb::Type, summary),
        "key" | "hold_key" => (BrowserVerb::Key, text_in(input, "text")),
        "scroll" | "scroll_to" => (
            BrowserVerb::Scroll,
            text_in(input, "scroll_direction").to_lowercase(),
        ),
        "hover" | "mouse_move" => (BrowserVerb::Hover, summary),
        "left_click_drag" => (BrowserVerb::Drag, summary),
        "screenshot" => (BrowserVerb::Screenshot, String::new()),
        "zoom" => (BrowserVerb::Zoom, String::new()),
        "wait" => (BrowserVerb::Wait, String::new()),
        _ => (BrowserVerb::Other, String::new()),
    };
    step(verb, subject)
}

pub fn step_of(tool: &str, input: &Map<String, Value>) -> BrowserStep {
    match tool {
        "computer" => computer(input),
        "navigate" => step(BrowserVerb::Open, address(input)),
        "tabs_create_mcp" => step(BrowserVerb::NewTab, address(input)),
        "tabs_close_mcp" => step(BrowserVerb::CloseTab, String::new()),
        "tabs_context_mcp" | "list_connected_browsers" => {
            step(BrowserVerb::ListTabs, String::new())
        }
        "find" => step(BrowserVerb::Find, text_in(input, "query")),
        "read_page" => step(BrowserVerb::ReadPage, String::new()),
        "get_page_text" => step(BrowserVerb::ReadText, String::new()),
        "read_console_messages" => step(BrowserVerb::Console, String::new()),
        "read_network_requests" => step(BrowserVerb::Network, String::new()),
        "javascript_tool" => step(BrowserVerb::Script, String::new()),
        "form_input" => step(BrowserVerb::FillForm, String::new()),
        "file_upload" | "upload_image" => step(BrowserVerb::Upload, String::new()),
        "resize_window" => step(BrowserVerb::Resize, String::new()),
        "switch_browser" | "select_browser" => step(BrowserVerb::Switch, String::new()),
        "shortcuts_execute" | "shortcuts_list" => step(BrowserVerb::Shortcut, String::new()),
        "gif_creator" => step(BrowserVerb::Recording, String::new()),
        other => step(
            BrowserVerb::Other,
            prefix_chars(other, SUBJECT_CHARS).to_string(),
        ),
    }
}

pub fn steps(name: &str, input: &Map<String, Value>) -> Vec<BrowserStep> {
    let tool = name.strip_prefix(PREFIX).unwrap_or(name);
    if tool != "browser_batch" {
        return vec![step_of(tool, input)];
    }
    input
        .get("actions")
        .and_then(Value::as_array)
        .map(|actions| {
            actions
                .iter()
                .take(BATCH_MAX)
                .map(|action| {
                    let empty = Map::new();
                    let inner = action
                        .get("input")
                        .and_then(Value::as_object)
                        .unwrap_or(&empty);
                    step_of(str_of(action, "name").unwrap_or(""), inner)
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn plain_title(steps: &[BrowserStep]) -> String {
    match steps.len() {
        0 | 1 => "Used Chrome".to_string(),
        n => format!("Used Chrome ({n} steps)"),
    }
}

pub fn call(id: &str, name: &str, input: &Map<String, Value>) -> ToolCall {
    let steps = steps(name, input);
    let mut call = ToolCall::new(
        id,
        name,
        ToolKind::Browser,
        plain_title(&steps),
        super::conversation::pretty(input),
    );
    call.browser_mut().steps = steps;
    call
}

const NOT_CONNECTED: [&str; 7] = [
    "extension is not connected",
    "claude in chrome is turned off",
    "never delivered to the chrome extension",
    "no chrome extension connected",
    "chrome extension disconnected",
    "extension disconnected mid",
    "extension may need to be re-authenticated",
];

const NEEDS_PERMISSION: [&str; 6] = [
    "permission required",
    "no response to the claude in chrome permission prompt",
    "permission prompt window couldn't be opened",
    "permission denied for reading pages on this domain",
    "navigation to this domain is not allowed",
    "requires site-level permissions",
];

pub fn fault_of(text: &str) -> Option<BrowserFault> {
    let lower = text.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    if any(&NOT_CONNECTED) {
        Some(BrowserFault::NotConnected)
    } else if lower.contains("permission denied by user") {
        Some(BrowserFault::Denied)
    } else if any(&NEEDS_PERMISSION) {
        Some(BrowserFault::NeedsPermission)
    } else {
        None
    }
}

pub fn console_errors(text: &str) -> u32 {
    let n = text
        .lines()
        .filter(|line| {
            let line = line.trim().to_lowercase();
            line.starts_with("[error]") || line.starts_with("error") || line.contains("[error]")
        })
        .count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

pub fn images_of(content: Option<&Value>) -> Vec<ToolImage> {
    let Some(blocks) = content.and_then(Value::as_array) else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter(|b| str_of(b, "type") == Some("image"))
        .filter_map(|b| {
            let source = b.get("source")?;
            Some(ToolImage {
                media_type: str_of(source, "media_type")
                    .unwrap_or("image/png")
                    .to_string(),
                data: str_of(source, "data")?.to_string(),
            })
        })
        .collect()
}

pub fn finish(call: &mut ToolCall, text: &str, content: Option<&Value>, is_error: bool) {
    let images = images_of(content);
    let state = call.browser_mut();
    state.shots = u32::try_from(images.len()).unwrap_or(u32::MAX);
    state.images = images;
    state.fault = if is_error { fault_of(text) } else { None };
    if let [only] = state.steps.as_mut_slice() {
        if only.verb == BrowserVerb::Console && !is_error {
            only.count = console_errors(text);
        }
    }
}

impl ToolImage {
    pub fn bytes(&self) -> Option<Vec<u8>> {
        base64::engine::general_purpose::STANDARD
            .decode(self.data.as_bytes())
            .ok()
    }
}

#[cfg(test)]
#[path = "browser_tests.rs"]
mod tests;
