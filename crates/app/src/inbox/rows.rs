use gpui::{div, prelude::*, px, Div};
use notchling_engine::inbox::Kind;
use notchling_kit::metric::SPACE;
use notchling_kit::ui::{icon, label, KitStyled};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};

use super::model::{Pill, Row, Tone};
use super::triage::Section;

const PILL_BAR: f32 = 44.0;
const DOT: f32 = 7.0;
const UNSEEN: f32 = 4.0;

pub fn kind_icon(kind: Kind) -> Icon {
    match kind {
        Kind::SecretApproval | Kind::SecretRequest => Icon::Key,
        Kind::LimitNotice => Icon::Gauge,
        Kind::ActionFailed | Kind::VersionCheckFailed => Icon::Warning,
        Kind::SettingChange => Icon::Settings,
        Kind::VersionDrift => Icon::Info,
        Kind::NodeJoin
        | Kind::PrimaryUnreachable
        | Kind::Failover
        | Kind::NoPrimary
        | Kind::PrimaryBack => Icon::Server,
        Kind::OutboxUnsent => Icon::Send,
        Kind::LoginNeeded => Icon::Account,
        Kind::PausedStep => Icon::Clock,
        Kind::SessionWaits => Icon::Sessions,
        Kind::Question => Icon::Question,
        Kind::Consent => Icon::Permission,
        Kind::ShellStuck => Icon::Terminal,
        Kind::Notice => Icon::Bell,
    }
}

pub fn status_ink(section: Section) -> Ink {
    match section {
        Section::Blocking | Section::Waiting | Section::DidntWork => Ink::Needs,
        Section::Running => Ink::Working,
        Section::Done => Ink::Done,
        Section::Info => Ink::TextFaint,
    }
}

fn tone_ink(tone: Tone) -> Ink {
    match tone {
        Tone::Plain => Ink::TextMuted,
        Tone::Good => Ink::Done,
        Tone::Bad | Tone::Warn => Ink::Needs,
    }
}

pub fn pill(pill: &Pill, a: Appearance) -> Div {
    let ink = tone_ink(pill.tone);
    let mut column = div()
        .flex()
        .flex_col()
        .items_end()
        .gap(px(2.0))
        .child(label(&pill.text, TypeRole::Meta, ink, a));
    if let Some(left) = pill.drain {
        column = column.child(
            div()
                .w(px(PILL_BAR))
                .h(px(2.0))
                .rounded_full()
                .fill(Ink::Hairline, a)
                .child(
                    div()
                        .h_full()
                        .w(px(PILL_BAR * left))
                        .rounded_full()
                        .fill(ink, a),
                ),
        );
    }
    column
}

fn chip(glyph: Icon, text: &str, a: Appearance) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[0]))
        .min_w(px(0.0))
        .child(icon(glyph, IconSize::Row.points(), Ink::TextFaint, a))
        .child(label(text, TypeRole::Meta, Ink::TextMuted, a).elide())
}

pub fn chips(row: &Row, a: Appearance) -> Option<Div> {
    let mut out = div().flex().flex_wrap().items_center().gap_x(px(SPACE[1]));
    let mut any = false;
    if let Some(machine) = &row.machine {
        out = out.child(chip(Icon::Server, machine, a));
        any = true;
    }
    if let Some(session) = &row.session {
        out = out.child(chip(Icon::Branch, session, a));
        any = true;
    }
    if let Some(waited) = &row.waited {
        out = out.child(chip(Icon::Clock, waited, a));
        any = true;
    }
    any.then_some(out)
}

pub fn head(row: &Row, enabled: bool, a: Appearance) -> Div {
    let ink = if enabled { Ink::Text } else { Ink::TextMuted };
    let status = status_ink(row.section);
    div()
        .relative()
        .flex()
        .items_start()
        .gap(px(SPACE[1]))
        .children(row.unseen.then(|| {
            div()
                .absolute()
                .left(px(-SPACE[1] - 2.0))
                .top(px(7.0))
                .size(px(UNSEEN))
                .rounded_full()
                .fill(Ink::Accent, a)
        }))
        .child(
            div()
                .size(px(DOT))
                .mt(px(5.0))
                .flex_shrink_0()
                .rounded_full()
                .fill(status, a),
        )
        .child(icon(
            kind_icon(row.kind),
            IconSize::Row.points(),
            Ink::TextMuted,
            a,
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .child(label(&row.title, TypeRole::Heading, ink, a)),
        )
        .child(div().flex_shrink_0().child(pill(&row.pill, a)))
}
