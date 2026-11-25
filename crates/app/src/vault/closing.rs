use std::sync::Arc;
use std::time::Duration;

use super::consent_save::{Failure, Refusal};
use super::store::Vault;
use super::twins::{Cover, Stage};
use super::wire::PendingRequest;
use crate::switches::feed::SwitchFeed;

pub const QUIT_WAIT: Duration = Duration::from_millis(800);

pub fn cancel_form(
    cover: &mut Cover,
    busy: bool,
    feed: &SwitchFeed,
    vault: &Arc<Vault>,
    wait: Duration,
) -> bool {
    if busy {
        return false;
    }
    let ids = leave(cover, feed);
    deny_within(vault, ids, wait);
    true
}

pub fn lapse(
    cover: &mut Cover,
    busy: bool,
    feed: &SwitchFeed,
    vault: &Arc<Vault>,
    wait: Duration,
) -> bool {
    if busy {
        for id in cover.pressed() {
            feed.dismiss_request(&id);
        }
        return false;
    }
    let pending = feed
        .requests()
        .iter()
        .filter_map(PendingRequest::from_event)
        .any(|r| cover.holds(&r));
    pending && cancel_form(cover, busy, feed, vault, wait)
}

fn leave(cover: &mut Cover, feed: &SwitchFeed) -> Vec<String> {
    let open = cover.stage() == Stage::Open;
    let twins = cover.settle(Stage::Done, feed);
    feed.dismiss_request(cover.id());
    if !open {
        return Vec::new();
    }
    let mut ids = vec![cover.id().to_owned()];
    ids.extend(twins);
    ids
}

fn deny(vault: &Vault, ids: &[String]) {
    for id in ids {
        let _ = vault.cancel_request(id);
    }
}

struct Counted(Arc<Vault>);

impl Counted {
    fn begin(vault: &Arc<Vault>) -> Counted {
        vault.ending().begin();
        Counted(vault.clone())
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.ending().finish();
    }
}

fn deny_within(vault: &Arc<Vault>, ids: Vec<String>, wait: Duration) {
    if ids.is_empty() || !vault.is_live() {
        return;
    }
    let counted = Counted::begin(vault);
    let started = std::thread::Builder::new()
        .name("vault-request-cancel".into())
        .spawn(move || deny(&counted.0, &ids));
    if started.is_ok() {
        vault.flush_cancels(wait);
    }
}

pub fn end_unsaved(
    vault: &Arc<Vault>,
    feed: &SwitchFeed,
    ids: Vec<String>,
    outcome: &Result<(), Failure>,
    wait: Duration,
) {
    if !matches!(outcome, Err(Failure::Refused(Refusal::NameTaken))) {
        return;
    }
    let ids: Vec<String> = ids
        .into_iter()
        .filter(|id| feed.unwanted_request(id))
        .collect();
    deny_within(vault, ids, wait);
}

#[cfg(test)]
#[path = "closing_tests.rs"]
mod tests;
