use notchling_engine::transcript::model::{
    BrowserFault, BrowserStep, BrowserVerb, ShotSpot, ToolCall, ToolImage,
};
use notchling_locale as t;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub title: String,
    pub hint: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserRow {
    pub steps: Vec<String>,
    pub fault: Option<Fault>,
    pub shots: Vec<ToolImage>,
    pub spots: Vec<ShotSpot>,
    pub missing: usize,
}

pub fn step_title(step: &BrowserStep) -> String {
    let what = step.subject.as_str();
    let has = !what.is_empty();
    match step.verb {
        BrowserVerb::Open if has => t::chat::browser_open(what),
        BrowserVerb::Open => t::chat::browser_open_page().to_owned(),
        BrowserVerb::NewTab if has => t::chat::browser_new_tab_site(what),
        BrowserVerb::NewTab => t::chat::browser_new_tab().to_owned(),
        BrowserVerb::CloseTab => t::chat::browser_close_tab().to_owned(),
        BrowserVerb::ListTabs => t::chat::browser_tabs().to_owned(),
        BrowserVerb::Click if has => t::chat::browser_click_what(what),
        BrowserVerb::Click => t::chat::browser_click().to_owned(),
        BrowserVerb::Type if has => t::chat::browser_type_what(what),
        BrowserVerb::Type => t::chat::browser_type().to_owned(),
        BrowserVerb::Key if has => t::chat::browser_key(what),
        BrowserVerb::Key => t::chat::browser_key_none().to_owned(),
        BrowserVerb::Scroll => match what {
            "up" => t::chat::browser_scroll_up(),
            "down" => t::chat::browser_scroll_down(),
            "left" => t::chat::browser_scroll_left(),
            "right" => t::chat::browser_scroll_right(),
            _ => t::chat::browser_scroll(),
        }
        .to_owned(),
        BrowserVerb::Hover => t::chat::browser_hover().to_owned(),
        BrowserVerb::Drag => t::chat::browser_drag().to_owned(),
        BrowserVerb::Screenshot => t::chat::browser_screenshot().to_owned(),
        BrowserVerb::Zoom => t::chat::browser_zoom().to_owned(),
        BrowserVerb::Wait => t::chat::browser_wait().to_owned(),
        BrowserVerb::ReadPage => t::chat::browser_read_page().to_owned(),
        BrowserVerb::ReadText => t::chat::browser_read_text().to_owned(),
        BrowserVerb::Find if has => t::chat::browser_find(what),
        BrowserVerb::Find => t::chat::browser_find_none().to_owned(),
        BrowserVerb::Console if step.count > 0 => t::chat::browser_console_errors(step.count),
        BrowserVerb::Console => t::chat::browser_console().to_owned(),
        BrowserVerb::Network => t::chat::browser_network().to_owned(),
        BrowserVerb::Script => t::chat::browser_script().to_owned(),
        BrowserVerb::FillForm => t::chat::browser_fill().to_owned(),
        BrowserVerb::Upload => t::chat::browser_upload().to_owned(),
        BrowserVerb::Resize => t::chat::browser_resize().to_owned(),
        BrowserVerb::Switch => t::chat::browser_switch().to_owned(),
        BrowserVerb::Shortcut => t::chat::browser_shortcut().to_owned(),
        BrowserVerb::Recording => t::chat::browser_recording().to_owned(),
        BrowserVerb::Other => t::chat::browser_other().to_owned(),
    }
}

pub fn fault_words(fault: BrowserFault) -> Fault {
    let (title, hint) = match fault {
        BrowserFault::NotConnected => (
            t::chat::browser_not_connected(),
            t::chat::browser_not_connected_hint(),
        ),
        BrowserFault::NeedsPermission => (
            t::chat::browser_needs_permission(),
            t::chat::browser_needs_permission_hint(),
        ),
        BrowserFault::Denied => (t::chat::browser_denied(), t::chat::browser_denied_hint()),
    };
    Fault {
        title: title.to_owned(),
        hint: hint.to_owned(),
    }
}

pub fn title(call: &ToolCall, fault: Option<&Fault>) -> String {
    if let Some(fault) = fault {
        return fault.title.clone();
    }
    match call.steps() {
        [] => t::chat::browser_other().to_owned(),
        [one] => step_title(one),
        many => t::chat::browser_batch(many.len()),
    }
}

pub fn row(call: &ToolCall) -> BrowserRow {
    let fault = call.fault().map(fault_words);
    let steps = if call.steps().len() > 1 {
        call.steps().iter().map(step_title).collect()
    } else {
        Vec::new()
    };
    let missing = usize::try_from(call.shots())
        .unwrap_or(usize::MAX)
        .saturating_sub(call.images().len())
        .saturating_sub(call.spots().len());
    BrowserRow {
        steps,
        fault,
        shots: call.images().to_vec(),
        spots: call.spots().to_vec(),
        missing,
    }
}

#[cfg(test)]
#[path = "browser_rows_tests.rs"]
mod tests;
