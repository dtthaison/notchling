use notchling_engine::limits::report::{BarKind, UsageReport};
use notchling_engine::pages::clock::civil_at;
use notchling_kit::Ink;
use notchling_locale as t;

use super::markers;
use crate::accounts::limits::{Fill, Zone};
use crate::accounts::wire::Window;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Session,
    Week,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Limit {
    pub name: String,
    pub kind: Kind,
    pub percent: f64,
    pub resets_at: Option<f64>,
}

pub fn claude_limits(windows: &[Window]) -> Vec<Limit> {
    windows
        .iter()
        .map(|w| {
            let lower = w.name.to_ascii_lowercase();
            let kind = if lower.starts_with("5-hour") || lower == "5h" || lower == "session" {
                Kind::Session
            } else if lower.starts_with("week") {
                Kind::Week
            } else {
                Kind::Other
            };
            Limit {
                name: w.name.clone(),
                kind,
                percent: w.percent,
                resets_at: w.resets_at,
            }
        })
        .collect()
}

pub fn codex_limits(report: &UsageReport) -> Vec<Limit> {
    report
        .groups
        .iter()
        .find(|g| g.name.as_deref() == Some(markers::CODEX_GROUP))
        .or_else(|| report.groups.first())
        .map(|g| {
            g.bars
                .iter()
                .map(|b| Limit {
                    name: b.label.clone(),
                    kind: match b.kind {
                        BarKind::Session => Kind::Session,
                        BarKind::Week => Kind::Week,
                        BarKind::Other => Kind::Other,
                    },
                    percent: b.percent,
                    resets_at: b.resets_at,
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn antigravity_limits(report: &UsageReport, model: Option<&str>) -> Vec<Limit> {
    let model = model.unwrap_or("").to_ascii_lowercase();
    let starts = report
        .groups
        .iter()
        .enumerate()
        .filter_map(|(i, g)| {
            let name = g.name.as_deref()?.to_ascii_lowercase();
            (!name.is_empty() && model.starts_with(&name)).then_some((i, name.len()))
        })
        .max_by_key(|(_, len)| *len)
        .map(|(i, _)| i);
    let names = || {
        report.groups.iter().position(|g| {
            g.name.as_deref().is_some_and(|n| {
                n.split('/')
                    .map(|part| part.trim().to_ascii_lowercase())
                    .any(|part| !part.is_empty() && model.contains(&part))
            })
        })
    };
    let first = starts.or_else(names).unwrap_or(0);
    let mut order: Vec<usize> = (0..report.groups.len()).collect();
    if first < order.len() {
        order.remove(first);
        order.insert(0, first);
    }
    order
        .into_iter()
        .flat_map(|i| {
            let group = &report.groups[i];
            let name = group.name.clone().unwrap_or_default();
            let mut bars: Vec<_> = group.bars.iter().collect();
            bars.sort_by_key(|b| b.kind != BarKind::Session);
            bars.into_iter()
                .map(|b| {
                    let window = match b.kind {
                        BarKind::Session => t::chat::limit_five_hour().to_owned(),
                        BarKind::Week => t::chat::limit_weekly_short().to_owned(),
                        BarKind::Other => b.label.clone(),
                    };
                    Limit {
                        name: t::chat::limit_window(&name, &window),
                        kind: Kind::Other,
                        percent: b.percent,
                        resets_at: b.resets_at,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub fn arc_of(limit: &Limit) -> Arc {
    let percent = clamp(limit.percent);
    Arc {
        fraction: (percent / 100.0) as f32,
        fill: Fill::of(percent),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    pub fraction: f32,
    pub fill: Fill,
}

pub fn arc(limits: &[Limit]) -> Option<Arc> {
    let session = limits.iter().find(|l| l.kind == Kind::Session)?;
    let percent = clamp(session.percent);
    Some(Arc {
        fraction: (percent / 100.0) as f32,
        fill: Fill::of(percent),
    })
}

pub fn ink(fill: Fill) -> Ink {
    match fill {
        Fill::Fine => Ink::Working,
        Fill::Warm => Ink::Needs,
        Fill::Hot => Ink::Expired,
    }
}

fn clamp(percent: f64) -> f64 {
    if percent.is_nan() {
        0.0
    } else {
        percent.clamp(0.0, 100.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub label: String,
    pub resets: Option<String>,
    pub percent_text: String,
    pub fraction: f32,
    pub fill: Fill,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub title: String,
    pub rows: Vec<Row>,
}

fn row_label(limit: &Limit) -> String {
    match limit.kind {
        Kind::Session => t::chat::limit_five_hour().to_owned(),
        Kind::Week => t::chat::limit_weekly().to_owned(),
        Kind::Other => {
            let lower = limit.name.to_ascii_lowercase();
            if lower.contains("limit") || lower.contains("week") {
                limit.name.clone()
            } else {
                t::chat::limit_named(&limit.name)
            }
        }
    }
}

pub fn card(limits: &[Limit], plan: Option<&str>, now: f64, zone: Zone) -> Option<Card> {
    if limits.is_empty() {
        return None;
    }
    let mut ordered: Vec<&Limit> = Vec::with_capacity(limits.len());
    for kind in [Kind::Session, Kind::Week, Kind::Other] {
        ordered.extend(limits.iter().filter(|l| l.kind == kind));
    }
    let rows = ordered
        .into_iter()
        .map(|l| {
            let percent = clamp(l.percent);
            Row {
                label: row_label(l),
                resets: l.resets_at.map(|at| reset_label(at, now, zone)),
                percent_text: format!("{}%", percent.round() as i64),
                fraction: (percent / 100.0) as f32,
                fill: Fill::of(percent),
            }
        })
        .collect();
    let title = match plan.map(str::trim).filter(|p| !p.is_empty()) {
        Some(plan) => t::chat::limits_title_plan(plan),
        None => t::chat::limits_title().to_owned(),
    };
    Some(Card { title, rows })
}

const WEEKDAYS: [t::Text; 7] = [
    t::units::WEEKDAY_SUN,
    t::units::WEEKDAY_MON,
    t::units::WEEKDAY_TUE,
    t::units::WEEKDAY_WED,
    t::units::WEEKDAY_THU,
    t::units::WEEKDAY_FRI,
    t::units::WEEKDAY_SAT,
];

pub fn reset_clock(resets_at: f64, zone: Zone) -> String {
    let offset = (zone.0)(resets_at);
    let c = civil_at(resets_at, offset);
    let days = (resets_at.floor() as i64 + offset).div_euclid(86_400);
    let weekday = (days + 4).rem_euclid(7) as usize;
    let (hour, suffix) = match c.hour {
        0 => (12, t::units::AM),
        1..=11 => (c.hour, t::units::AM),
        12 => (12, t::units::PM),
        h => (h - 12, t::units::PM),
    };
    t::units::reset_clock(
        WEEKDAYS[weekday].get(),
        hour,
        &format!("{:02}", c.minute),
        suffix.get(),
    )
}

pub fn reset_label(resets_at: f64, now: f64, zone: Zone) -> String {
    if resets_at <= now {
        t::chat::limit_resetting().to_owned()
    } else {
        t::chat::limit_resets(&reset_clock(resets_at, zone))
    }
}

pub fn arc_end(fraction: f32, cx: f32, cy: f32, r: f32) -> (f32, f32) {
    let angle = fraction.clamp(0.0, 1.0) * std::f32::consts::TAU;
    (cx + r * angle.sin(), cy - r * angle.cos())
}

pub const OPEN_MS: u64 = 1500;

pub const GRACE_MS: u64 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    Open(u64),
    Close(u64),
}

#[derive(Debug, Default)]
pub struct Hover {
    ring: bool,
    card: bool,
    shown: bool,
    epoch: u64,
}

impl Hover {
    pub fn open(&self, forced: bool) -> bool {
        forced || self.shown
    }

    fn away(&self) -> bool {
        !self.ring && !self.card
    }

    pub fn ring(&mut self, inside: bool) -> Option<Wait> {
        self.ring = inside;
        self.moved()
    }

    pub fn card(&mut self, inside: bool) -> Option<Wait> {
        self.card = inside;
        self.moved()
    }

    fn moved(&mut self) -> Option<Wait> {
        self.epoch += 1;
        if !self.shown {
            return self.ring.then_some(Wait::Open(self.epoch));
        }
        self.away().then_some(Wait::Close(self.epoch))
    }

    pub fn click(&mut self) {
        self.epoch += 1;
        self.shown = !self.shown;
    }

    pub fn open_if_still(&mut self, epoch: u64) -> bool {
        let open = self.epoch == epoch && self.ring && !self.shown;
        if open {
            self.shown = true;
        }
        open
    }

    pub fn close_if_still(&mut self, epoch: u64) -> bool {
        let close = self.epoch == epoch && self.away() && self.shown;
        if close {
            self.shown = false;
        }
        close
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::limits::report::{Bar, Group};

    const NOW: f64 = 1_800_000_000.0;

    fn limit(name: &str, kind: Kind, percent: f64, resets_at: Option<f64>) -> Limit {
        Limit {
            name: name.into(),
            kind,
            percent,
            resets_at,
        }
    }

    #[test]
    fn the_arc_is_the_five_hour_limit_and_hides_without_one() {
        let limits = vec![
            limit("weekly", Kind::Week, 78.0, None),
            limit("5-hour", Kind::Session, 41.0, None),
        ];
        let a = arc(&limits).unwrap();
        assert!((a.fraction - 0.41).abs() < 1e-6);
        assert_eq!(a.fill, Fill::Fine);
        assert_eq!(arc(&[limit("weekly", Kind::Week, 78.0, None)]), None);
        assert_eq!(arc(&[]), None);
        let over = arc(&[limit("5-hour", Kind::Session, 140.0, None)]).unwrap();
        assert_eq!(over.fraction, 1.0);
    }

    #[test]
    fn the_ink_is_blue_then_warning_then_expired_at_the_headers_thresholds() {
        assert_eq!(ink(Fill::of(69.9)), Ink::Working);
        assert_eq!(ink(Fill::of(70.0)), Ink::Needs);
        assert_eq!(ink(Fill::of(89.9)), Ink::Needs);
        assert_eq!(ink(Fill::of(90.0)), Ink::Expired);
    }

    #[test]
    fn claude_windows_are_read_by_the_headers_names() {
        let windows = vec![
            Window {
                name: "5-hour".into(),
                percent: 71.0,
                resets_at: None,
            },
            Window {
                name: "weekly".into(),
                percent: 56.0,
                resets_at: None,
            },
            Window {
                name: "Opus".into(),
                percent: 3.0,
                resets_at: None,
            },
        ];
        let kinds: Vec<_> = claude_limits(&windows).iter().map(|l| l.kind).collect();
        assert_eq!(kinds, [Kind::Session, Kind::Week, Kind::Other]);
    }

    #[test]
    fn codex_limits_come_from_its_group() {
        let report = UsageReport {
            groups: vec![Group {
                name: Some("Codex".into()),
                bars: vec![
                    Bar {
                        label: "5h".into(),
                        kind: BarKind::Session,
                        percent: 10.0,
                        resets_at: Some(NOW),
                    },
                    Bar {
                        label: "Week".into(),
                        kind: BarKind::Week,
                        percent: 20.0,
                        resets_at: None,
                    },
                ],
            }],
            fetched: NOW,
        };
        let l = codex_limits(&report);
        assert_eq!((l[0].kind, l[1].kind), (Kind::Session, Kind::Week));
        assert_eq!(l[0].resets_at, Some(NOW));
    }

    #[test]
    fn the_card_orders_five_hour_weekly_then_the_rest_and_words_the_plan() {
        let limits = vec![
            limit("Sonnet", Kind::Other, 3.4, None),
            limit("weekly", Kind::Week, 56.0, Some(NOW + 86_400.0)),
            limit("5-hour", Kind::Session, 71.2, Some(NOW + 3_600.0)),
            limit("Design", Kind::Other, 95.0, None),
        ];
        let c = card(&limits, Some("Team"), NOW, Zone::UTC).unwrap();
        assert_eq!(c.title, "Your usage limits · Team");
        let labels: Vec<_> = c.rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "5-hour limit",
                "Weekly · all models",
                "Sonnet limit",
                "Design limit"
            ]
        );
        assert_eq!(c.rows[0].percent_text, "71%");
        assert_eq!(c.rows[0].resets.as_deref(), Some("Resets Fri 9:00 AM"));
        assert_eq!(c.rows[1].resets.as_deref(), Some("Resets Sat 8:00 AM"));
        assert_eq!(c.rows[2].resets, None);
        assert_eq!(c.rows[3].fill, Fill::Hot);
        assert_eq!(
            card(&limits, None, NOW, Zone::UTC).unwrap().title,
            "Your usage limits"
        );
        assert_eq!(card(&[], Some("Team"), NOW, Zone::UTC), None);
    }

    #[test]
    fn a_limit_named_for_itself_keeps_its_name() {
        let named = limit("Opus weekly", Kind::Other, 1.0, None);
        assert_eq!(row_label(&named), "Opus weekly");
    }

    #[test]
    fn the_reset_reads_weekday_then_a_twelve_hour_clock() {
        let midnight = 1_800_000_000.0 - 8.0 * 3600.0;
        assert_eq!(reset_clock(midnight, Zone::UTC), "Fri 12:00 AM");
        assert_eq!(
            reset_clock(midnight + 90.0 * 60.0, Zone::UTC),
            "Fri 1:30 AM"
        );
        assert_eq!(
            reset_clock(midnight + 12.0 * 3600.0, Zone::UTC),
            "Fri 12:00 PM"
        );
        assert_eq!(
            reset_clock(midnight + 23.0 * 3600.0 + 5.0 * 60.0, Zone::UTC),
            "Fri 11:05 PM"
        );
        let ahead = Zone(|_| 2 * 3600);
        assert_eq!(reset_clock(midnight + 23.0 * 3600.0, ahead), "Sat 1:00 AM");
        assert_eq!(reset_label(NOW - 1.0, NOW, Zone::UTC), "Resetting");
        assert_eq!(
            reset_label(midnight + 86_400.0, midnight, Zone::UTC),
            "Resets Sat 12:00 AM"
        );
    }

    #[test]
    fn the_arc_ends_where_the_clock_hand_would() {
        let near =
            |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4;
        assert!(near(arc_end(0.0, 10.0, 10.0, 5.0), (10.0, 5.0)));
        assert!(near(arc_end(0.25, 10.0, 10.0, 5.0), (15.0, 10.0)));
        assert!(near(arc_end(0.5, 10.0, 10.0, 5.0), (10.0, 15.0)));
        assert!(near(arc_end(0.75, 10.0, 10.0, 5.0), (5.0, 10.0)));
    }

    fn opened() -> Hover {
        let mut h = Hover::default();
        let Some(Wait::Open(epoch)) = h.ring(true) else {
            panic!("entering the ring waits to open");
        };
        assert!(h.open_if_still(epoch));
        h
    }

    #[test]
    fn the_card_opens_only_after_the_pointer_rests_on_the_ring() {
        let mut h = Hover::default();
        let Some(Wait::Open(epoch)) = h.ring(true) else {
            panic!("entering the ring waits to open");
        };
        assert!(!h.open(false), "nothing shows during the wait");
        assert!(h.open_if_still(epoch));
        assert!(h.open(false));

        let mut h = Hover::default();
        let Some(Wait::Open(epoch)) = h.ring(true) else {
            panic!();
        };
        assert_eq!(
            h.ring(false),
            None,
            "a leave before it opens waits for nothing"
        );
        assert!(!h.open_if_still(epoch));
        assert!(!h.open(false));
        assert_eq!(OPEN_MS, 1500);
    }

    #[test]
    fn a_click_opens_the_card_at_once_and_a_second_closes_it() {
        let mut h = Hover::default();
        let Some(Wait::Open(epoch)) = h.ring(true) else {
            panic!("entering the ring waits to open");
        };
        h.click();
        assert!(h.open(false), "open without waiting");
        assert!(!h.open_if_still(epoch), "the wait no longer matters");
        assert!(h.open(false));
        h.click();
        assert!(!h.open(false), "a second click closes it");
        h.click();
        let Some(Wait::Close(epoch)) = h.ring(false) else {
            panic!("away from both waits to close");
        };
        assert!(h.close_if_still(epoch));
        assert!(!h.open(false));
    }

    #[test]
    fn the_card_waits_a_grace_before_closing_and_a_return_cancels_it() {
        let mut h = opened();
        let Some(Wait::Close(epoch)) = h.ring(false) else {
            panic!("away from both waits to close");
        };
        assert!(h.open(false), "still open during the grace");
        assert_eq!(h.card(true), None);
        assert!(!h.close_if_still(epoch), "a later move cancels the close");
        assert!(h.open(false));
        let Some(Wait::Close(epoch)) = h.card(false) else {
            panic!();
        };
        assert!(h.close_if_still(epoch));
        assert!(!h.open(false));
        assert!(h.open(true), "a render can force it open");
    }

    fn agy_report() -> UsageReport {
        let bar = |kind, percent| Bar {
            label: if kind == BarKind::Week { "Week" } else { "5h" }.into(),
            kind,
            percent,
            resets_at: None,
        };
        UsageReport {
            groups: vec![
                Group {
                    name: Some("Claude/GPT".into()),
                    bars: vec![bar(BarKind::Week, 47.0), bar(BarKind::Session, 94.0)],
                },
                Group {
                    name: Some("Gemini".into()),
                    bars: vec![bar(BarKind::Session, 0.0), bar(BarKind::Week, 0.0)],
                },
                Group {
                    name: Some("Gemini 4 Argon".into()),
                    bars: vec![bar(BarKind::Week, 100.0)],
                },
            ],
            fetched: NOW,
        }
    }

    #[test]
    fn antigravity_rings_the_group_of_the_model_in_use() {
        let names = |model: Option<&str>| -> Vec<String> {
            antigravity_limits(&agy_report(), model)
                .into_iter()
                .map(|l| l.name)
                .collect()
        };
        let argon = names(Some("Gemini 4 Argon (Medium)"));
        assert_eq!(argon[0], "Gemini 4 Argon · weekly limit");
        assert_eq!(argon.len(), 5, "every group is in the card");
        assert_eq!(names(Some("Gemini 3 Flash"))[0], "Gemini · 5-hour limit");
        assert_eq!(
            names(Some("Claude Sonnet 4.6 (Thinking)"))[0],
            "Claude/GPT · 5-hour limit"
        );
        assert_eq!(names(None)[0], "Claude/GPT · 5-hour limit");
        let limits = antigravity_limits(&agy_report(), Some("Gemini 4 Argon"));
        let arc = arc_of(&limits[0]);
        assert_eq!(arc.fraction, 1.0);
        assert_eq!(arc.fill, Fill::Hot);
        assert!(antigravity_limits(
            &UsageReport {
                groups: vec![],
                fetched: NOW
            },
            None
        )
        .is_empty());
    }
}
