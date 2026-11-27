use super::entry::VaultEntry;
use super::expiry::{self, Status, RENEW_SOON_DAYS};
use std::collections::{BTreeMap, BTreeSet};

pub type Sent = BTreeMap<String, Vec<String>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Threshold {
    Week,
    Day,
    Today,
    Expired,
}

impl Threshold {
    pub fn key(self) -> &'static str {
        match self {
            Threshold::Week => "7d",
            Threshold::Day => "1d",
            Threshold::Today => "0d",
            Threshold::Expired => "expired",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Due {
    pub name: String,
    pub id: String,
    pub day: String,
    pub threshold: Threshold,
    pub message: String,
}

impl Due {
    pub fn title(&self) -> &'static str {
        if self.threshold == Threshold::Expired {
            "Secret expired"
        } else {
            "Secret expiring"
        }
    }
}

pub fn key(id: &str, day: &str) -> String {
    format!("{id}|{day}")
}

pub fn due(entries: &[VaultEntry], sent: &Sent, today: i64) -> Vec<Due> {
    entries
        .iter()
        .filter_map(|entry| {
            let day = entry.expires.as_deref()?;
            let (threshold, when) = match expiry::status(day, today)? {
                Status::Expired => (Threshold::Expired, format!("expired on {day}")),
                Status::Today => (Threshold::Today, format!("expires today ({day})")),
                Status::InDays(n) if n <= RENEW_SOON_DAYS => {
                    let threshold = if n <= 1 {
                        Threshold::Day
                    } else {
                        Threshold::Week
                    };
                    let unit = if n == 1 { "day" } else { "days" };
                    (threshold, format!("expires in {n} {unit} ({day})"))
                }
                Status::InDays(_) => return None,
            };
            let id = entry.id();
            let already = sent
                .get(&key(&id, day))
                .is_some_and(|s| s.iter().any(|t| t == threshold.key()));
            (!already).then(|| Due {
                name: entry.name.clone(),
                id,
                day: day.to_string(),
                threshold,
                message: format!("{} {when}", entry.name),
            })
        })
        .collect()
}

pub fn mark_sent(due: &Due, sent: &Sent) -> Sent {
    let mut sent = sent.clone();
    sent.entry(key(&due.id, &due.day))
        .or_default()
        .push(due.threshold.key().to_string());
    sent
}

pub fn clearing(id: &str, sent: &Sent) -> Sent {
    let prefix = format!("{id}|");
    sent.iter()
        .filter(|(k, _)| !k.starts_with(&prefix))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

pub fn pruned(sent: &Sent, entries: &[VaultEntry]) -> Sent {
    let live: BTreeSet<String> = entries
        .iter()
        .filter_map(|e| e.expires.as_deref().map(|d| key(&e.id(), d)))
        .collect();
    sent.iter()
        .filter(|(k, _)| live.contains(*k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
