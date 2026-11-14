use gpui::{div, prelude::*, px, App, Div};
use notchling_kit::ui::{label, tool_icon, KitPart, ToolMark};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::agents::{Section, Tool};
use super::capabilities_view::capabilities_section;
use super::client_model::ClientPart;
use super::client_view::client_section;
use super::hook_rows::HooksShown;
use super::settings_hooks_view::hooks_section;

fn mark(tool: Tool) -> ToolMark {
    match tool {
        Tool::ClaudeCode => ToolMark::Claude,
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

pub fn agents_page(
    tool: Tool,
    hooks: &HooksShown,
    client: Option<&ClientPart>,
    a: Appearance,
    cx: &App,
) -> Div {
    let mut page = div().flex().flex_col().gap(px(14.0)).child(header(tool, a));
    for section in tool.sections() {
        page = page.child(match section {
            Section::Client => client_section(client, a, cx),
            Section::Hooks => hooks_section(hooks, a, cx),
        });
    }
    page.child(capabilities_section(a, cx))
        .part("Agents Page", a, cx)
}
