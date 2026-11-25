use notchling_locale as t;
use serde_json::Value;

use crate::accounts::cost::{dollars, tokens};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    Week,
    Month,
}

impl Span {
    pub const ALL: [Span; 2] = [Span::Week, Span::Month];

    pub fn days(self) -> u32 {
        match self {
            Span::Week => 7,
            Span::Month => 30,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Span::Week => "week",
            Span::Month => "month",
        }
    }

    pub fn from_key(key: &str) -> Option<Span> {
        Span::ALL.into_iter().find(|s| s.key() == key)
    }

    pub fn title(self) -> &'static str {
        match self {
            Span::Week => t::costs::spend_week(),
            Span::Month => t::costs::spend_month(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Side {
    pub from: String,
    pub to: String,
    pub days: u32,
    pub calls: u64,
    pub tokens: u64,
    pub cost: Option<f64>,
    pub unpriced_tokens: u64,
}

impl Side {
    fn parse(v: &Value) -> Side {
        let n = |key: &str| v.get(key).and_then(Value::as_u64).unwrap_or(0);
        let s = |key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
        Side {
            from: s("from"),
            to: s("to"),
            days: n("days") as u32,
            calls: n("calls"),
            tokens: n("tokens"),
            cost: v.get("cost").and_then(Value::as_f64),
            unpriced_tokens: n("unpriced_tokens"),
        }
    }

    fn per_day(&self) -> Option<f64> {
        self.cost.map(|c| c / f64::from(self.days.max(1)))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Compare {
    pub window: u32,
    pub applied: String,
    pub before: Side,
    pub after: Side,
}

impl Compare {
    pub fn parse(v: &Value) -> Option<Compare> {
        let side = |key: &str| v.get(key).filter(|s| s.is_object()).map(Side::parse);
        Some(Compare {
            window: v.get("window")?.as_u64()? as u32,
            applied: v.get("applied")?.as_str()?.to_owned(),
            before: side("before")?,
            after: side("after")?,
        })
    }

    pub fn change(&self) -> Option<f64> {
        let (b, a) = (self.before.per_day()?, self.after.per_day()?);
        (b > 0.0).then(|| (a - b) / b)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub title: &'static str,
    pub range: String,
    pub headline: String,
    pub detail: Option<String>,
    pub activity: Option<String>,
    pub unpriced: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpendView {
    pub intro: Option<String>,
    pub empty: Option<&'static str>,
    pub columns: Vec<Column>,
    pub change: Option<String>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpendModel {
    pub span: Span,
    pub loaded: bool,
    pub compare: Option<Compare>,
    pub message: Option<String>,
}

impl Default for SpendModel {
    fn default() -> SpendModel {
        SpendModel {
            span: Span::Week,
            loaded: false,
            compare: None,
            message: None,
        }
    }
}

impl SpendModel {
    pub fn show_for(&mut self, span: Span, reply: Result<Value, String>) -> bool {
        if span != self.span {
            return false;
        }
        self.show(reply);
        true
    }

    pub fn show(&mut self, reply: Result<Value, String>) {
        self.loaded = true;
        match reply {
            Ok(v) => {
                self.compare = v.get("compare").and_then(Compare::parse);
                self.message = None;
            }
            Err(why) => {
                self.compare = None;
                self.message = Some(why);
            }
        }
    }

    pub fn view(&self) -> SpendView {
        let mut out = SpendView {
            intro: None,
            empty: None,
            columns: Vec::new(),
            change: None,
            note: None,
        };
        let Some(c) = &self.compare else {
            out.empty = Some(if self.loaded {
                t::costs::spend_not_applied()
            } else {
                t::costs::spend_loading()
            });
            return out;
        };
        out.intro = Some(t::costs::spend_intro(&c.window.to_string(), &c.applied));
        out.columns = vec![
            column(t::costs::spend_before(), &c.before),
            column(t::costs::spend_after(), &c.after),
        ];
        out.change = c.change().map(|x| t::costs::spend_change(&percent(x)));
        if c.after.days < c.window {
            out.note = Some(t::costs::spend_days_since(u64::from(c.after.days)));
        }
        out
    }
}

pub fn percent(change: f64) -> String {
    let rounded = (change * 100.0).round();
    if rounded == 0.0 {
        return "0%".to_string();
    }
    let sign = if rounded > 0.0 { "+" } else { "" };
    format!("{sign}{rounded:.0}%")
}

fn column(title: &'static str, side: &Side) -> Column {
    let range = t::costs::spend_range(&side.from, &side.to);
    if side.calls == 0 {
        return Column {
            title,
            range,
            headline: t::costs::spend_no_usage().to_owned(),
            detail: None,
            activity: None,
            unpriced: None,
        };
    }
    let activity = Some(t::costs::spend_activity(
        &tokens(side.calls as f64),
        &tokens(side.tokens as f64),
    ));
    match side.cost {
        Some(cost) => Column {
            title,
            range,
            headline: t::costs::spend_per_day(&dollars(cost / f64::from(side.days.max(1)))),
            detail: Some(t::costs::spend_total(&dollars(cost))),
            activity,
            unpriced: (side.unpriced_tokens > 0)
                .then(|| t::costs::spend_unpriced(&tokens(side.unpriced_tokens as f64))),
        },
        None => Column {
            title,
            range,
            headline: t::costs::spend_no_price(&tokens(side.tokens as f64)),
            detail: None,
            activity,
            unpriced: None,
        },
    }
}
