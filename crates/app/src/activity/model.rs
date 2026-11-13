use notchling_engine::pages::clock::civil_at;
use serde_json::Value;

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
            Kind::Refused => "Refused",
            Kind::Failed => "Failed",
            Kind::Acted => "Acted",
            Kind::Setting => "Setting",
            Kind::Inbox => "Inbox",
            Kind::AccountSwitch => "Account Switch",
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
        })
    }

    pub fn line(&self) -> String {
        let place = match self.source.as_str() {
            "phone" => format!("{} (phone)", self.who),
            _ => self.who.clone(),
        };
        let mut line = format!("{place} \u{b7} {}", self.result);
        if let Some(detail) = &self.detail {
            line.push_str(" \u{b7} ");
            line.push_str(detail);
        }
        line
    }

    pub fn matches(&self, needle: &str) -> bool {
        let needle = needle.to_lowercase();
        [&self.op, &self.result, &self.who]
            .into_iter()
            .chain(self.detail.as_ref())
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
            Load::Loading if self.entries.is_empty() => Some("Loading\u{2026}".to_owned()),
            Load::Failed(message) => Some(message.clone()),
            _ if self.visible().is_empty() && self.entries.is_empty() => {
                Some("No activity yet".to_owned())
            }
            _ if self.visible().is_empty() => Some("Nothing matches".to_owned()),
            _ => None,
        }
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
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
    let clock = format!(
        "{hour}:{:02} {}",
        at.minute,
        if at.hour < 12 { "AM" } else { "PM" }
    );
    if (at.year, at.month, at.day) == (now.year, now.month, now.day) {
        return clock;
    }
    let month = MONTHS[(at.month as usize - 1) % 12];
    if at.year == now.year {
        format!("{month} {}, {clock}", at.day)
    } else {
        format!("{month} {}, {}, {clock}", at.day, at.year)
    }
}
