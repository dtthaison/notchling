use gpui::{div, prelude::*, px, App, Div};
use notchling_kit::ui::{label, tool_icon, KitPart, ToolMark};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::agents::{Section, Tool};
use super::capabilities_view::capabilities_section;
use super::client_model::ClientPart;
use super::client_view::{client_section, ClientHandlers};
use super::hook_rows::HooksShown;
use super::settings_hooks_view::hooks_section;

fn mark(tool: Tool) -> ToolMark {
    match tool {
        Tool::ClaudeCode => ToolMark::Claude,
        Tool::Codex => ToolMark::Codex,
        Tool::Antigravity => ToolMark::Antigravity,
    }
}

fn header(tool: Tool, a: Appearance) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .child(tool_icon(mark(tool), 24.0, a))
        .child(label(tool.title(), TypeRole::Heading, Ink::Text, a))
}

pub struct OwnSections {
    pub settings: Vec<Div>,
    pub chrome: Div,
}

pub fn agents_page(
    tool: Tool,
    own: OwnSections,
    hooks: &HooksShown,
    client: Option<&ClientPart>,
    handlers: &ClientHandlers,
    a: Appearance,
    cx: &App,
) -> Div {
    let mut chrome = Some(own.chrome);
    let mut page = div().flex().flex_col().gap(px(14.0)).child(header(tool, a));
    let mut settings = Some(own.settings);
    for section in tool.sections() {
        page = match section {
            Section::Settings => page.children(settings.take().unwrap_or_default()),
            Section::Client => page.child(client_section(client, handlers, a, cx)),
            Section::Chrome => page.child(chrome.take().unwrap_or_else(div)),
            Section::Hooks => page.child(hooks_section(hooks, a, cx)),
            Section::Capabilities => page.child(capabilities_section(tool.contract(), a, cx)),
        };
    }
    page.part("Agents Page", a, cx)
}
