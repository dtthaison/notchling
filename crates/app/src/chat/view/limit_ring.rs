//! The usage-limit ring at the right end of the reply box's footer, and the card that opens over it
//! while the pointer is on the ring or the card (part "Limit Ring" and "Limit Popover"). The ring
//! is a 16 pt arc of the 5-hour limit in a 28 pt ghost button; the card lists every limit the
//! session's account has, with "See all →" opening the Accounts sheet on that account. The rules
//! (which limits, the arc and its ink, the rows, the reset words, the grace delay) are
//! `chat::limit_ring`; this draws them. Claude Code's limits are the account in use's, the same
//! windows as the header's meters (a session in a lane has none yet); Codex's are its active
//! login's first group; Antigravity's are every group of its active login, the session model's
//! group first, and its ring is that group's first bar. Unknown limits draw nothing.

use std::time::Duration;

use gpui::{
    canvas, div, fill, point, prelude::*, px, size, Bounds, Context, Div, PathBuilder, Pixels,
    Stateful, Window,
};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{CONTROL, METER_BAR, RADIUS_SM};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button_with, card as kit_card, hsla, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::ChatView;
use crate::accounts::limits::Zone;
use crate::accounts::model::{ANTIGRAVITY, CLAUDE_CODE, CODEX};
use crate::chat::data::Tool;
use crate::chat::limit_ring::{self as rules, Arc, Hover, Limit, Wait, GRACE_MS, OPEN_MS};

/// Set to `1`, a render draws the card open, since nothing hovers in a picture.
pub const LIMITS_ENV: &str = "NOTCHLING_RENDER_LIMITS";

/// The ring's drawing size and stroke, in points.
const RING: f32 = 16.0;
const STROKE: f32 = 2.5;
/// The card's width, and the gap between it and the ring (part of the card's hover area, so the
/// pointer crosses it without leaving).
const CARD_WIDTH: f32 = 340.0;
const CARD_GAP: f32 = 6.0;

/// What Chat keeps for the ring: where the pointer is, and a render's switch.
#[derive(Default)]
pub struct State {
    hover: Hover,
    preview: bool,
}

impl State {
    /// The render constructor's: the card drawn open (`NOTCHLING_RENDER_LIMITS=1`).
    pub fn preview(on: bool) -> State {
        State {
            preview: on,
            ..State::default()
        }
    }
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// The selected session's limits and where "See all" goes.
struct Shown {
    limits: Vec<Limit>,
    arc: Option<Arc>,
    plan: Option<String>,
    provider: &'static str,
}

fn shown(view: &ChatView) -> Option<Shown> {
    let session = view.selected_session()?;
    let s = view.accounts.snapshot();
    let shown = match session.tool {
        Tool::Claude if session.agent.config_dir.is_none() => {
            let windows = &s
                .limits
                .providers
                .iter()
                .find(|p| p.id == "claude")?
                .windows;
            let limits = rules::claude_limits(windows);
            Shown {
                arc: rules::arc(&limits),
                limits,
                plan: s
                    .claude
                    .active
                    .as_ref()
                    .and_then(|id| s.claude.accounts.iter().find(|a| &a.id == id))
                    .and_then(|a| a.plan.clone()),
                provider: CLAUDE_CODE,
            }
        }
        Tool::Codex => {
            let codex = s.providers.providers.iter().find(|p| p.id == CODEX)?;
            let active = codex.active_id.as_ref()?;
            let report = s.usage.get(CODEX)?.get(active)?;
            let limits = rules::codex_limits(report);
            Shown {
                arc: rules::arc(&limits),
                limits,
                plan: codex
                    .accounts
                    .iter()
                    .find(|a| &a.id == active)
                    .and_then(|a| a.plan.clone()),
                provider: CODEX,
            }
        }
        Tool::Antigravity => {
            let agy = s.providers.providers.iter().find(|p| p.id == ANTIGRAVITY)?;
            let active = agy.active_id.as_ref()?;
            let report = s.usage.get(ANTIGRAVITY)?.get(active)?;
            let model = view
                .tools
                .agy_config
                .iter()
                .find(|o| o.id == "model")
                .map(|o| o.current_name().to_owned());
            let limits = rules::antigravity_limits(report, model.as_deref());
            Shown {
                arc: limits.first().map(rules::arc_of),
                limits,
                plan: agy
                    .accounts
                    .iter()
                    .find(|a| &a.id == active)
                    .and_then(|a| a.plan.clone()),
                provider: ANTIGRAVITY,
            }
        }
        _ => return None,
    };
    Some(shown)
}

impl ChatView {
    fn limit_hover(&mut self, ring: bool, inside: bool, cx: &mut Context<Self>) {
        let hover = &mut self.limit_ring.hover;
        let wait = if ring {
            hover.ring(inside)
        } else {
            hover.card(inside)
        };
        if let Some(wait) = wait {
            let ms = match wait {
                Wait::Open(_) => OPEN_MS,
                Wait::Close(_) => GRACE_MS,
            };
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(ms))
                    .await;
                this.update(cx, |view, cx| {
                    let hover = &mut view.limit_ring.hover;
                    let changed = match wait {
                        Wait::Open(epoch) => hover.open_if_still(epoch),
                        Wait::Close(epoch) => hover.close_if_still(epoch),
                    };
                    if changed {
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    fn limit_see_all(&mut self, provider: &str, cx: &mut Context<Self>) {
        self.limit_ring.hover = Hover::default();
        self.open_accounts_sheet(Some(provider), cx);
    }
}

/// Paints the arc: a faint full circle, then the used part from 12 o'clock clockwise with round
/// ends (GPUI's path strokes have butt caps, so the ends are small discs).
fn paint_ring(
    bounds: Bounds<Pixels>,
    arc: Arc,
    ink: gpui::Hsla,
    track: gpui::Hsla,
    window: &mut Window,
) {
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let r = w.min(h) / 2.0 - STROKE / 2.0;
    let (cx, cy) = (
        f32::from(bounds.origin.x) + w / 2.0,
        f32::from(bounds.origin.y) + h / 2.0,
    );
    let at = |(x, y): (f32, f32)| point(px(x), px(y));
    let radii = point(px(r), px(r));
    let top = rules::arc_end(0.0, cx, cy, r);
    let bottom = rules::arc_end(0.5, cx, cy, r);

    let mut full = PathBuilder::stroke(px(STROKE));
    full.move_to(at(top));
    full.arc_to(radii, px(0.0), false, true, at(bottom));
    full.arc_to(radii, px(0.0), false, true, at(top));
    if let Ok(path) = full.build() {
        window.paint_path(path, track);
    }

    let f = arc.fraction;
    if f <= 0.0 {
        return;
    }
    let mut used = PathBuilder::stroke(px(STROKE));
    used.move_to(at(top));
    if f >= 0.999 {
        used.arc_to(radii, px(0.0), false, true, at(bottom));
        used.arc_to(radii, px(0.0), false, true, at(top));
    } else {
        used.arc_to(
            radii,
            px(0.0),
            f > 0.5,
            true,
            at(rules::arc_end(f, cx, cy, r)),
        );
    }
    if let Ok(path) = used.build() {
        window.paint_path(path, ink);
    }
    if f < 0.999 {
        for end in [top, rules::arc_end(f, cx, cy, r)] {
            let cap = Bounds::centered_at(at(end), size(px(STROKE), px(STROKE)));
            window.paint_quad(fill(cap, ink).corner_radii(px(STROKE / 2.0)));
        }
    }
}

fn ring_mark(arc: Arc, a: Appearance) -> impl IntoElement {
    let ink = hsla(rules::ink(arc.fill).color(a));
    let track = hsla(Ink::TextFaint.color(a).with_alpha(0.3));
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| paint_ring(bounds, arc, ink, track, window),
    )
    .size(px(RING))
}

/// One limit: its name with the reset and percent at the right, and a thin bar under it.
fn row(r: &rules::Row, a: Appearance) -> Div {
    let ink = rules::ink(r.fill);
    let mut right = div().flex().items_center().gap(px(8.0)).flex_shrink_0();
    if let Some(resets) = &r.resets {
        right = right.child(label(resets, TypeRole::Meta, Ink::TextMuted, a));
    }
    right = right.child(label(&r.percent_text, TypeRole::Meta, Ink::TextMuted, a));
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(label(&r.label, TypeRole::Label, Ink::Text, a))
                .child(right),
        )
        .child(
            div()
                .h(px(METER_BAR))
                .w_full()
                .rounded_full()
                .fill(Ink::Raised, a)
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(r.fraction.clamp(0.0, 1.0)))
                        .rounded_full()
                        .fill(ink, a),
                ),
        )
}

fn popover(
    card: &rules::Card,
    provider: &'static str,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    let see_all = button_with(
        "limit-see-all",
        label("See all →", TypeRole::Label, Ink::Text, a),
        ButtonKind::Ghost,
        PartState::Rest,
        a,
    )
    .h(px(20.0))
    .px(px(6.0))
    .on_click(cx.listener(move |this, _, _, cx| this.limit_see_all(provider, cx)));
    let body = kit_card(a)
        .w(px(CARD_WIDTH))
        .px(px(16.0))
        .py(px(14.0))
        .flex()
        .flex_col()
        .gap(px(14.0))
        .shadow_lg()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(label(&card.title, TypeRole::Label, Ink::TextMuted, a))
                .child(see_all),
        )
        .children(card.rows.iter().map(|r| row(r, a)));
    // The transparent gap under the card belongs to it, so the pointer can cross from the ring.
    div()
        .id("limit-popover")
        .absolute()
        .bottom(gpui::relative(1.0))
        .right(px(0.0))
        .pb(px(CARD_GAP))
        .occlude()
        .on_hover(cx.listener(|this, inside: &bool, _, cx| this.limit_hover(false, *inside, cx)))
        .child(body)
        .part_at("Limit Popover", Corner::TopTrailing, a, cx)
}

/// The ring for the footer, with its card while the pointer is over either; nothing when the
/// session has no 5-hour limit to show.
pub(super) fn ring(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let shown = shown(view)?;
    let arc = shown.arc?;
    let open = view.limit_ring.hover.open(view.limit_ring.preview);
    let card = open
        .then(|| {
            rules::card(
                &shown.limits,
                shown.plan.as_deref(),
                now_secs(),
                Zone::LOCAL,
            )
        })
        .flatten();
    let button = button_with(
        "limit-ring",
        ring_mark(arc, a),
        ButtonKind::Ghost,
        PartState::Rest,
        a,
    )
    .w(px(CONTROL))
    .h(px(CONTROL))
    .px(px(0.0))
    .rounded(px(RADIUS_SM))
    .on_hover(cx.listener(|this, inside: &bool, _, cx| this.limit_hover(true, *inside, cx)))
    // A click opens the card at once (or closes it), without the hover's wait.
    .on_click(cx.listener(|this, _, _, cx| {
        this.limit_ring.hover.click();
        cx.notify();
    }))
    .part_at("Limit Ring", Corner::Top, a, cx);
    Some(
        div()
            .relative()
            .flex_shrink_0()
            .child(button)
            .children(card.map(|c| popover(&c, shown.provider, a, cx))),
    )
}
