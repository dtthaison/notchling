use super::engine::{lock, SetupEngine};
use super::registry::Machine;
use super::step::{FixOutcome, StepId};
use super::usage::SecretUsage;
use crate::store;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const OFFERS_DOC: &str = "machines/pending-updates";

pub fn old_offers_file(data: &Path) -> PathBuf {
    data.join("daemon")
        .join("machines")
        .join("pending-updates.json")
}

#[derive(Serialize, Deserialize)]
struct OfferFile {
    version: u32,
    offers: BTreeMap<String, Vec<String>>,
}

pub fn offer_bytes(offers: &BTreeMap<String, Vec<SecretUsage>>) -> Option<Vec<u8>> {
    if offers.is_empty() {
        return None;
    }
    let file = OfferFile {
        version: 1,
        offers: offers
            .iter()
            .map(|(k, v)| (k.clone(), v.iter().map(|u| u.machine.clone()).collect()))
            .collect(),
    };
    let value = serde_json::to_value(file).ok()?;
    Some(super::registry::pretty_sorted(value))
}

pub fn parse_offers(data: &[u8], usages: &[SecretUsage]) -> BTreeMap<String, Vec<SecretUsage>> {
    let Ok(file) = serde_json::from_slice::<OfferFile>(data) else {
        return BTreeMap::new();
    };
    if file.version != 1 {
        return BTreeMap::new();
    }
    file.offers
        .into_iter()
        .filter_map(|(name, machines)| {
            let found: Vec<SecretUsage> = usages
                .iter()
                .filter(|u| u.name == name && machines.contains(&u.machine))
                .cloned()
                .collect();
            (!found.is_empty()).then_some((name, found))
        })
        .collect()
}

impl SetupEngine {
    pub(super) fn load_offers(&self) {
        let Some(data) = &self.deps.offers_data else {
            return;
        };
        let _ = store::import_file(data, OFFERS_DOC, &old_offers_file(data), |text| {
            serde_json::from_str::<OfferFile>(text).is_ok_and(|f| f.version == 1)
        });
        let Ok(Some(text)) = store::doc(data, OFFERS_DOC) else {
            return;
        };
        let usages = (self.catalog.usages)(&self.current_machines());
        lock(&self.state).offers = parse_offers(text.as_bytes(), &usages);
    }

    pub(super) fn save_offers(&self) {
        let Some(data) = &self.deps.offers_data else {
            return;
        };
        let old = old_offers_file(data);
        let offers = lock(&self.state).offers.clone();
        let written = match offer_bytes(&offers).and_then(|b| String::from_utf8(b).ok()) {
            None => {
                !store::exists(data) || store::update_doc(data, OFFERS_DOC, |_| Ok(None)).is_ok()
            }
            Some(text) => store::put_doc(data, OFFERS_DOC, &text).is_ok(),
        };
        if written {
            let _ = std::fs::remove_file(old);
        }
    }

    fn set_offer(&self, name: &str, usages: Option<Vec<SecretUsage>>) {
        {
            let mut s = lock(&self.state);
            match usages {
                Some(u) if !u.is_empty() => {
                    s.offers.insert(name.to_string(), u);
                }
                _ => {
                    s.offers.remove(name);
                }
            }
        }
        self.save_offers();
        self.changed();
    }

    pub fn vault_value_changed(&self, name: &str) {
        *lock(&self.state)
            .offer_revision
            .entry(name.to_string())
            .or_default() += 1;
        let found: Vec<SecretUsage> = (self.catalog.usages)(&self.current_machines())
            .into_iter()
            .filter(|u| u.name == name)
            .collect();
        self.set_offer(name, Some(found));
    }

    pub(super) fn send_again(&self, usage: &SecretUsage) -> FixOutcome {
        let Some(m) = self.machine(&usage.machine) else {
            return FixOutcome::fail(format!("no machine named {}", usage.machine));
        };
        if !(self.catalog.usages)(&self.current_machines()).contains(usage) {
            return FixOutcome::fail(format!(
                "{} at {} is not a secret file of {}",
                usage.name, usage.remote_path, m.name
            ));
        }
        if self.running(&m.name).is_some() {
            return FixOutcome::fail(format!("another Fix is running on {}", m.name));
        }
        let outcome = self.deliver_and_check(usage, &m, &format!("Send {} again", usage.name));
        if outcome.ok {
            let rest = self.update_offers().get(&usage.name).map(|u| {
                u.iter()
                    .filter(|x| *x != usage)
                    .cloned()
                    .collect::<Vec<_>>()
            });
            if let Some(rest) = rest {
                self.set_offer(&usage.name, Some(rest));
            }
        }
        outcome
    }

    fn deliver_and_check(&self, usage: &SecretUsage, m: &Machine, title: &str) -> FixOutcome {
        let issues = m.sd_worker().is_some_and(|j| j.issues_secret == usage.name)
            || (usage.name != m.claude_secret && usage.remote_path == super::shell::GITHUB_TOKEN);
        let id = if issues {
            StepId::ISSUES_TOKEN
        } else {
            StepId::CLAUDE_LOGIN
        };
        let deliver = self.catalog.deliver.clone();
        let outcome = self.perform(id, title, m, None, |ctx| deliver(usage, ctx));
        self.refresh(Some(std::slice::from_ref(&m.name)), issues);
        outcome
    }

    pub(super) fn update_machines(&self, secret: &str) -> Vec<FixOutcome> {
        let revision = lock(&self.state).offer_revision.get(secret).copied();
        let mut outcomes = Vec::new();
        let mut failed = Vec::new();
        for usage in self
            .update_offers()
            .get(secret)
            .cloned()
            .unwrap_or_default()
        {
            let Some(m) = self.machine(&usage.machine) else {
                outcomes.push(FixOutcome::fail(format!(
                    "no machine named {}",
                    usage.machine
                )));
                failed.push(usage);
                continue;
            };
            if self.running(&m.name).is_some() {
                outcomes.push(FixOutcome::fail(format!(
                    "another Fix is running on {}",
                    m.name
                )));
                failed.push(usage);
                continue;
            }
            let outcome = self.deliver_and_check(&usage, &m, &format!("Update {}", usage.name));
            if !outcome.ok {
                failed.push(usage);
            }
            outcomes.push(outcome);
        }
        if lock(&self.state).offer_revision.get(secret).copied() == revision {
            self.set_offer(secret, Some(failed));
        }
        outcomes
    }
}
