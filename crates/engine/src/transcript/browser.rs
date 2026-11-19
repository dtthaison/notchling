use super::model::{
    BrowserFault, BrowserStep, BrowserVerb, ShotSpot, ToolCall, ToolImage, ToolKind,
};
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

pub fn plain_title(_steps: &[BrowserStep]) -> String {
    "Used Chrome".to_string()
}

pub fn plain_step(step: &BrowserStep) -> String {
    let what = step.subject.as_str();
    let has = !what.is_empty();
    let with = |base: &str| {
        if has {
            format!("{base}: {what}")
        } else {
            base.to_string()
        }
    };
    match step.verb {
        BrowserVerb::Open if has => format!("Opened {what}"),
        BrowserVerb::Open => "Opened a page".to_string(),
        BrowserVerb::NewTab => with("New tab"),
        BrowserVerb::CloseTab => "Closed tab".to_string(),
        BrowserVerb::ListTabs => "Checked open tabs".to_string(),
        BrowserVerb::Click => with("Clicked"),
        BrowserVerb::Type => with("Typed text"),
        BrowserVerb::Key if has => format!("Pressed {what}"),
        BrowserVerb::Key => "Pressed a key".to_string(),
        BrowserVerb::Scroll if has => format!("Scrolled {what}"),
        BrowserVerb::Scroll => "Scrolled the page".to_string(),
        BrowserVerb::Hover => "Moved the pointer".to_string(),
        BrowserVerb::Drag => "Dragged".to_string(),
        BrowserVerb::Screenshot => "Took a screenshot".to_string(),
        BrowserVerb::Zoom => "Zoomed in on the page".to_string(),
        BrowserVerb::Wait => "Waited".to_string(),
        BrowserVerb::ReadPage => "Read the page layout".to_string(),
        BrowserVerb::ReadText => "Read the page".to_string(),
        BrowserVerb::Find if has => format!("Looked for \u{201c}{what}\u{201d}"),
        BrowserVerb::Find => "Looked for an element".to_string(),
        BrowserVerb::Console => "Read console".to_string(),
        BrowserVerb::Network => "Read network requests".to_string(),
        BrowserVerb::Script => "Ran a script".to_string(),
        BrowserVerb::FillForm => "Filled in a form field".to_string(),
        BrowserVerb::Upload => "Uploaded a file".to_string(),
        BrowserVerb::Resize => "Resized the window".to_string(),
        BrowserVerb::Switch => "Switched browser".to_string(),
        BrowserVerb::Shortcut => "Used a shortcut".to_string(),
        BrowserVerb::Recording => "Made a recording".to_string(),
        BrowserVerb::Other => "Used Chrome".to_string(),
    }
}

pub fn plain_summary(name: &str, input: &Map<String, Value>) -> String {
    match steps(name, input).as_slice() {
        [] => "Used Chrome".to_string(),
        [one] => plain_step(one),
        many => format!("{} browser steps", many.len()),
    }
}

pub fn plain_fault(text: &str) -> Option<&'static str> {
    Some(match fault_of(text)? {
        BrowserFault::NotConnected => "Chrome isn't connected",
        BrowserFault::NeedsPermission => "Waiting for permission in the Chrome extension",
        BrowserFault::Denied => "Permission was refused in Chrome",
    })
}

pub fn call(id: &str, name: &str, input: &Map<String, Value>) -> ToolCall {
    let steps = steps(name, input);
    let mut call = ToolCall::new(
        id,
        name,
        ToolKind::Browser,
        plain_title(&steps),
        String::new(),
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

const ERROR_CHARS: usize = 300;
const TAB_CONTEXT: &str = "Tab Context:";

pub fn console_errors(text: &str) -> u32 {
    let text = text.split(TAB_CONTEXT).next().unwrap_or("");
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

fn console_blocks(content: Option<&Value>) -> Vec<String> {
    let Some(blocks) = content.and_then(Value::as_array) else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter(|b| str_of(b, "type") == Some("text"))
        .filter_map(|b| str_of(b, "text"))
        .filter(|t| t.starts_with("[read_console_messages"))
        .map(str::to_string)
        .collect()
}

pub fn finish(call: &mut ToolCall, text: &str, content: Option<&Value>, is_error: bool) {
    let images = images_of(content);
    let consoles = console_blocks(content);
    let state = call.browser_mut();
    state.shots = u32::try_from(images.len()).unwrap_or(u32::MAX);
    state.images = images;
    state.fault = if is_error { fault_of(text) } else { None };
    if !is_error {
        if let [only] = state.steps.as_mut_slice() {
            if only.verb == BrowserVerb::Console {
                only.count = console_errors(text);
            }
        } else {
            let mut blocks = consoles.iter();
            for step in state
                .steps
                .iter_mut()
                .filter(|s| s.verb == BrowserVerb::Console)
            {
                if let Some(block) = blocks.next() {
                    step.count = console_errors(block);
                }
            }
        }
    }
    call.output = (is_error && call.fault().is_none())
        .then(|| prefix_chars(text.trim(), ERROR_CHARS).to_string())
        .filter(|t| !t.is_empty());
}

pub fn spot_for(path: &str, at: u64) -> ShotSpot {
    ShotSpot {
        path: path.to_string(),
        at,
        index: 0,
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
