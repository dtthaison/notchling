use gpui::{div, prelude::*, px, App, Div};
use notchling_kit::part::ButtonKind;
use notchling_kit::ui::{card, hsla, label, status_pill, KitPart, KitStyled};
use notchling_kit::{Appearance, Ink, Status, TypeRole};
use notchling_locale as t;

use super::client_view::{action, row_shell, Click};
use super::models_rows::{ChoiceLine, ModelsShown, RoleLine, Source};
use super::settings_hooks_view::tag;
use super::skills_rows::{beside_button, SkillLine, SkillState, SkillsShown, ToolSkills};

#[derive(Clone)]
pub struct SkillsHandlers {
    pub reinstall: Click,
    pub open_override: Click,
    pub reload: Click,
}

/// What the page shows besides the daemon's answers.
pub struct SkillsPageState<'a> {
    pub skills: &'a SkillsShown,
    pub models: &'a ModelsShown,
    /// A reinstall is running.
    pub busy: bool,
    /// The last reinstall's result, or why the override file couldn't be made.
    pub result: Option<&'a (String, bool)>,
}

fn section(title: &str, a: Appearance) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(label(title, TypeRole::Section, Ink::TextFaint, a))
}

fn status_line(text: &str, a: Appearance) -> Div {
    div().child(label(text, TypeRole::Meta, Ink::TextMuted, a))
}

fn note(text: &str, a: Appearance) -> Div {
    div().child(label(text, TypeRole::Meta, Ink::TextFaint, a))
}

fn how_section(a: Appearance, cx: &App) -> Div {
    let lines = [
        t::settings::how_skills(),
        t::settings::how_commands(),
        t::settings::how_models(),
        t::settings::how_override(),
    ];
    let rows = lines.iter().enumerate().map(|(i, line)| {
        let row = div().px(px(14.0)).py(px(8.0));
        let row = if i == 0 {
            row
        } else {
            row.border_t(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
        };
        row.child(label(line, TypeRole::Body, Ink::Text, a))
    });
    section(t::settings::how_title(), a)
        .child(card(a).flex().flex_col().children(rows))
        .part("Skills How It Works", a, cx)
}

fn state_tag(state: SkillState, a: Appearance) -> Div {
    match state {
        SkillState::Installed => status_pill(Status::Done, state.word(), a),
        SkillState::OutOfDate | SkillState::NotInstalled => {
            status_pill(Status::Needs, state.word(), a)
        }
        SkillState::ToolMissing | SkillState::LeftAlone => tag(state.word(), a),
    }
}

fn skill_row(line: &SkillLine, a: Appearance) -> Div {
    row_shell(a, false)
        .child(label(&line.name, TypeRole::Mono, Ink::Text, a).flex_1())
        .child(state_tag(line.state, a))
}

fn tool_rows(group: &ToolSkills, first: bool, a: Appearance) -> Div {
    let header = div()
        .px(px(14.0))
        .py(px(8.0))
        .fill(Ink::Raised, a)
        .child(label(group.tool.title(), TypeRole::Heading, Ink::Text, a));
    let header = if first {
        header
    } else {
        header
            .border_t(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
    };
    div()
        .flex()
        .flex_col()
        .child(header)
        .children(group.lines.iter().map(|l| skill_row(l, a)))
}

fn skills_section(page: &SkillsPageState, h: &SkillsHandlers, a: Appearance, cx: &App) -> Div {
    let mut out = section(t::settings::skills_title(), a);
    if let Some(status) = page.skills.status() {
        out = out.child(status_line(&status, a));
    }
    if let SkillsShown::Found { tools, .. } = page.skills {
        out = out.child(
            card(a).flex().flex_col().overflow_hidden().children(
                tools
                    .iter()
                    .enumerate()
                    .map(|(i, g)| tool_rows(g, i == 0, a)),
            ),
        );
    }
    let enabled = page.skills.can_reinstall() && !page.busy;
    let title = if page.busy {
        t::settings::skills_reinstalling()
    } else {
        t::settings::skills_reinstall()
    };
    let mut buttons = div().flex().items_center().gap(px(12.0)).child(action(
        "skills-reinstall",
        title,
        ButtonKind::Secondary,
        enabled,
        &h.reinstall,
        a,
    ));
    if let Some((text, failed)) = beside_button(page.skills, page.result) {
        let ink = if failed { Ink::Needs } else { Ink::TextMuted };
        buttons = buttons.child(label(&text, TypeRole::Meta, ink, a).flex_1());
    }
    out.child(buttons)
        .child(note(t::settings::skills_footer(), a))
        .part("Skills List", a, cx)
}

fn source_tag(source: Source, a: Appearance) -> Div {
    tag(source.word(), a)
}

fn choice_row(c: &ChoiceLine, a: Appearance) -> Div {
    let model_ink = if c.has_model {
        Ink::Text
    } else {
        Ink::TextMuted
    };
    let mut row = div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .min_h(px(22.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(label(&c.model, TypeRole::Mono, model_ink, a).flex_1())
                .child(source_tag(c.source, a)),
        )
        .child(label(&c.tool_effort, TypeRole::Meta, Ink::TextMuted, a));
    if let Some(note) = &c.note {
        row = row.child(label(note, TypeRole::Meta, Ink::AccentText, a));
    }
    if c.unchecked {
        row = row.child(label(
            t::settings::model_unchecked(),
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ));
    }
    if let Some(advice) = &c.advice {
        row = row.child(label(advice, TypeRole::Meta, Ink::TextFaint, a));
    }
    row
}

fn role_row(role: &RoleLine, first: bool, a: Appearance) -> Div {
    let mut row = div()
        .px(px(14.0))
        .py(px(10.0))
        .flex()
        .flex_col()
        .gap(px(6.0));
    if !first {
        row = row
            .border_t(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)));
    }
    row = row.child(label(&role.title, TypeRole::Heading, Ink::Text, a));
    if role.choices.is_empty() {
        return row.child(label(
            t::settings::model_no_pick(),
            TypeRole::Meta,
            Ink::TextMuted,
            a,
        ));
    }
    row.children(role.choices.iter().map(|c| choice_row(c, a)))
}

fn models_section(page: &SkillsPageState, a: Appearance, cx: &App) -> Div {
    let mut out = section(t::settings::models_title(), a);
    if let Some(status) = page.models.status() {
        out = out.child(status_line(&status, a));
    }
    if let ModelsShown::Found { roles, .. } = page.models {
        out = out.child(
            card(a).flex().flex_col().children(
                roles
                    .iter()
                    .enumerate()
                    .map(|(i, r)| role_row(r, i == 0, a)),
            ),
        );
    }
    out.child(note(t::settings::models_footer(), a))
        .part("Models by Job", a, cx)
}

fn override_section(page: &SkillsPageState, h: &SkillsHandlers, a: Appearance, cx: &App) -> Div {
    let mut out = section(t::settings::override_title(), a);
    if let Some(file) = page.models.file() {
        out = out.child(
            card(a).flex().flex_col().child(
                div()
                    .px(px(14.0))
                    .py(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(label(&file.shown, TypeRole::Mono, Ink::Text, a).elide())
                    .child(label(file.state.line(), TypeRole::Meta, Ink::TextMuted, a)),
            ),
        );
    }
    let usable = page.models.file().is_some_and(|f| f.path.is_some());
    out.child(
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(action(
                "override-open",
                t::settings::override_open(),
                ButtonKind::Secondary,
                usable,
                &h.open_override,
                a,
            ))
            .child(action(
                "override-reload",
                t::settings::override_reload(),
                ButtonKind::Ghost,
                true,
                &h.reload,
                a,
            )),
    )
    .child(note(t::settings::override_footer(), a))
    .part("Models Override", a, cx)
}

pub fn skills_page(
    page: &SkillsPageState,
    handlers: &SkillsHandlers,
    a: Appearance,
    cx: &App,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(how_section(a, cx))
        .child(skills_section(page, handlers, a, cx))
        .child(models_section(page, a, cx))
        .child(override_section(page, handlers, a, cx))
        .part("Skills and Models Page", a, cx)
}
