use notchling_engine::pages::clock::civil_at;
use notchling_locale::{self as t, Text};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Refused,
    Failed,
    Acted,
    Setting,
    Inbox,
    AccountSwitch,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Refused,
        Kind::Failed,
        Kind::Acted,
        Kind::Setting,
        Kind::Inbox,
        Kind::AccountSwitch,
    ];

    pub fn wire(self) -> &'static str {
        match self {
            Kind::Refused => "refused",
            Kind::Failed => "failed",
            Kind::Acted => "acted",
            Kind::Setting => "setting",
            Kind::Inbox => "inbox",
            Kind::AccountSwitch => "account_switch",
        }
    }

    pub fn from_wire(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.wire() == name)
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Refused => t::settings::activity_kind_refused(),
            Kind::Failed => t::settings::activity_kind_failed(),
            Kind::Acted => t::settings::activity_kind_acted(),
            Kind::Setting => t::settings::activity_kind_setting(),
            Kind::Inbox => t::settings::activity_kind_inbox(),
            Kind::AccountSwitch => t::settings::activity_kind_account_switch(),
        }
    }

    pub fn stands_out(self) -> bool {
        matches!(self, Kind::Refused | Kind::Failed)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub time_ms: i64,
    pub source: String,
    pub kind: Kind,
    pub op: String,
    pub who: String,
    pub result: String,
    pub detail: Option<String>,
    pub event: Option<String>,
    pub title: Option<String>,
    pub outcome: Option<String>,
    pub seq: Option<u64>,
    pub audit_n: Option<u64>,
    pub spooled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineCheck {
    Matches,
    Differs,
    Unsealed,
}

impl LineCheck {
    pub fn text(self) -> &'static str {
        match self {
            LineCheck::Matches => t::settings::activity_check_matches(),
            LineCheck::Differs => t::settings::activity_check_differs(),
            LineCheck::Unsealed => t::settings::activity_check_unsealed(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    Running,
    Done(LineCheck),
    Failed(String),
}

impl Entry {
    pub fn parse(value: &Value) -> Option<Entry> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        Some(Entry {
            time_ms: i64::try_from(value.get("time_ms")?.as_u64()?).ok()?,
            source: text("source")?,
            kind: Kind::from_wire(value.get("kind")?.as_str()?)?,
            op: text("op")?,
            who: text("who")?,
            result: text("result")?,
            detail: text("detail").filter(|d| !d.is_empty()),
            event: text("event"),
            title: text("title").filter(|t| !t.is_empty()),
            outcome: text("outcome"),
            seq: value.get("seq").and_then(Value::as_u64),
            audit_n: value.get("audit_n").and_then(Value::as_u64),
            spooled: value.get("spooled").and_then(Value::as_bool) == Some(true),
        })
    }

    pub fn heading(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.op)
    }

    pub fn compare(&self, reply: &Value) -> Option<LineCheck> {
        let line = reply.get("line")?;
        if line.get("sealed").and_then(Value::as_bool) != Some(true) {
            return Some(LineCheck::Unsealed);
        }
        if line.get("seal_ok").and_then(Value::as_bool) != Some(true) {
            return Some(LineCheck::Differs);
        }
        let same_text = |mine: &Option<String>, key: &str| {
            mine.as_deref().is_none_or(|mine| {
                line.get(key)
                    .and_then(Value::as_str)
                    .is_none_or(|theirs| theirs == mine)
            })
        };
        let same_time = line
            .get("ts")
            .and_then(Value::as_i64)
            .is_none_or(|ts| ts == self.time_ms);
        let same =
            same_text(&self.event, "kind") && same_text(&self.outcome, "outcome") && same_time;
        Some(if same {
            LineCheck::Matches
        } else {
            LineCheck::Differs
        })
    }

    pub fn line(&self) -> String {
        let place = match self.source.as_str() {
            "phone" => t::settings::activity_from_phone(&self.who),
            _ => self.who.clone(),
        };
        match &self.detail {
            Some(detail) => t::settings::activity_line_detail(&place, &self.result, detail),
            None => t::settings::activity_line(&place, &self.result),
        }
    }

    pub fn matches(&self, needle: &str) -> bool {
        let needle = needle.to_lowercase();
        [&self.op, &self.result, &self.who]
            .into_iter()
            .chain(self.detail.as_ref())
            .chain(self.title.as_ref())
            .any(|field| field.to_lowercase().contains(&needle))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Load {
    #[default]
    Idle,
    Loading,
    Done,
    Failed(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActivityModel {
    pub entries: Vec<Entry>,
    pub load: Load,
    pub truncated: bool,
    pub kinds: Vec<Kind>,
    pub search: String,
    pub open: Option<u64>,
    pub checks: HashMap<u64, Check>,
}

impl ActivityModel {
    pub fn loaded(entries: Vec<Entry>) -> ActivityModel {
        ActivityModel {
            entries,
            load: Load::Done,
            ..ActivityModel::default()
        }
    }

    pub fn start(&mut self) -> bool {
        if self.load == Load::Loading {
            return false;
        }
        self.load = Load::Loading;
        true
    }

    pub fn finish(&mut self, reply: Result<Value, String>) {
        match reply {
            Ok(value) => {
                let mut entries: Vec<Entry> = value
                    .get("rows")
                    .and_then(Value::as_array)
                    .map(|rows| rows.iter().filter_map(Entry::parse).collect())
                    .unwrap_or_default();
                entries.sort_by_key(|e| std::cmp::Reverse(e.time_ms));
                self.entries = entries;
                self.truncated = value.get("truncated").and_then(Value::as_bool) == Some(true);
                self.load = Load::Done;
            }
            Err(message) => self.load = Load::Failed(message),
        }
    }

    pub fn toggle_open(&mut self, seq: u64) {
        self.open = if self.open == Some(seq) {
            None
        } else {
            Some(seq)
        };
    }

    pub fn begin_check(&mut self, seq: u64) -> bool {
        if self.checks.get(&seq) == Some(&Check::Running) {
            return false;
        }
        self.checks.insert(seq, Check::Running);
        true
    }

    pub fn finish_check(&mut self, seq: u64, reply: Result<Value, String>) {
        let entry = self.entries.iter().find(|e| e.seq == Some(seq));
        let done = match (reply, entry) {
            (Ok(value), Some(entry)) => match entry.compare(&value) {
                Some(check) => Check::Done(check),
                None => Check::Failed(t::settings::activity_check_no_entry().to_owned()),
            },
            (Ok(_), None) => Check::Failed(t::settings::activity_check_gone().to_owned()),
            (Err(message), _) => Check::Failed(message),
        };
        self.checks.insert(seq, done);
    }

    pub fn toggle(&mut self, kind: Kind) {
        match self.kinds.iter().position(|k| *k == kind) {
            Some(at) => {
                self.kinds.remove(at);
            }
            None => self.kinds.push(kind),
        }
    }

    pub fn is_on(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.entries.iter().filter(|e| e.kind == kind).count()
    }

    pub fn visible(&self) -> Vec<&Entry> {
        let needle = self.search.trim();
        self.entries
            .iter()
            .filter(|e| self.kinds.is_empty() || self.kinds.contains(&e.kind))
            .filter(|e| needle.is_empty() || e.matches(needle))
            .collect()
    }

    pub fn empty_text(&self) -> Option<String> {
        match &self.load {
            Load::Loading if self.entries.is_empty() => {
                Some(t::settings::activity_loading().to_owned())
            }
            Load::Failed(message) => Some(message.clone()),
            _ if self.visible().is_empty() && self.entries.is_empty() => {
                Some(t::settings::activity_empty().to_owned())
            }
            _ if self.visible().is_empty() => Some(t::settings::activity_no_match().to_owned()),
            _ => None,
        }
    }
}

const MONTHS: [Text; 12] = [
    t::settings::ACTIVITY_MONTH_JAN,
    t::settings::ACTIVITY_MONTH_FEB,
    t::settings::ACTIVITY_MONTH_MAR,
    t::settings::ACTIVITY_MONTH_APR,
    t::settings::ACTIVITY_MONTH_MAY,
    t::settings::ACTIVITY_MONTH_JUN,
    t::settings::ACTIVITY_MONTH_JUL,
    t::settings::ACTIVITY_MONTH_AUG,
    t::settings::ACTIVITY_MONTH_SEP,
    t::settings::ACTIVITY_MONTH_OCT,
    t::settings::ACTIVITY_MONTH_NOV,
    t::settings::ACTIVITY_MONTH_DEC,
];

pub fn time_label(now_ms: i64, at_ms: i64, offset_secs: i64) -> String {
    if at_ms <= 0 {
        return String::new();
    }
    let at = civil_at(at_ms as f64 / 1000.0, offset_secs);
    let now = civil_at(now_ms as f64 / 1000.0, offset_secs);
    let hour = match at.hour % 12 {
        0 => 12,
        h => h,
    };
    let minute = format!("{:02}", at.minute);
    let clock = if at.hour < 12 {
        t::settings::activity_time_am(&hour.to_string(), &minute)
    } else {
        t::settings::activity_time_pm(&hour.to_string(), &minute)
    };
    if (at.year, at.month, at.day) == (now.year, now.month, now.day) {
        return clock;
    }
    let month = MONTHS[(at.month as usize - 1) % 12].get();
    let day = at.day.to_string();
    if at.year == now.year {
        t::settings::activity_time_date(month, &day, &clock)
    } else {
        t::settings::activity_time_date_year(month, &day, &at.year.to_string(), &clock)
    }
}
