use notchling_engine::pages::clock::civil_at;
use notchling_locale as t;
use serde_json::Value;

use super::model::{ClusterModel, NodeRow};
use super::move_sheet::{shown, Names, Note};
use super::replication::ago;
use super::wire::{self, Request};
use crate::chat::message_meta::clock_text;

pub use super::promote_sheet::{PromotePhase, PromoteSheet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKind {
    Handover,
    Promote,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromoteStep {
    Check,
    Reach,
    Confirm,
    Bump,
    Restore,
    Resume,
    Held,
    Items,
    Announce,
    Done,
    Relayed,
    Failed,
    Unknown,
}

pub const RUN: [PromoteStep; 9] = [
    PromoteStep::Check,
    PromoteStep::Reach,
    PromoteStep::Bump,
    PromoteStep::Restore,
    PromoteStep::Resume,
    PromoteStep::Held,
    PromoteStep::Items,
    PromoteStep::Announce,
    PromoteStep::Done,
];

impl PromoteStep {
    pub fn from_name(name: &str) -> PromoteStep {
        match name {
            "check" => PromoteStep::Check,
            "reach" => PromoteStep::Reach,
            "confirm" => PromoteStep::Confirm,
            "bump" => PromoteStep::Bump,
            "restore" => PromoteStep::Restore,
            "resume" => PromoteStep::Resume,
            "held" => PromoteStep::Held,
            "items" => PromoteStep::Items,
            "announce" => PromoteStep::Announce,
            "done" => PromoteStep::Done,
            "relayed" => PromoteStep::Relayed,
            "failed" => PromoteStep::Failed,
            _ => PromoteStep::Unknown,
        }
    }

    pub fn ended(self) -> bool {
        matches!(
            self,
            PromoteStep::Done
                | PromoteStep::Relayed
                | PromoteStep::Failed
                | PromoteStep::Confirm
                | PromoteStep::Unknown
        )
    }

    pub fn position(self) -> Option<usize> {
        RUN.iter().position(|s| *s == self)
    }

    pub fn line(self, to: &str, primary: &str) -> String {
        match self {
            PromoteStep::Check => t::cluster::promote_step_check(to),
            PromoteStep::Reach => t::cluster::promote_step_reach(primary),
            PromoteStep::Bump => t::cluster::promote_step_bump(to),
            PromoteStep::Restore => t::cluster::promote_step_restore().to_owned(),
            PromoteStep::Resume => t::cluster::promote_step_resume(to),
            PromoteStep::Held => t::cluster::promote_step_held().to_owned(),
            PromoteStep::Items => t::cluster::promote_step_items().to_owned(),
            PromoteStep::Announce => t::cluster::move_step_announce().to_owned(),
            PromoteStep::Done => t::cluster::move_step_done(to),
            PromoteStep::Confirm
            | PromoteStep::Relayed
            | PromoteStep::Failed
            | PromoteStep::Unknown => t::cluster::promote_step_unknown().to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromoteStatus {
    pub node: String,
    pub step: PromoteStep,
    pub started: i64,
    pub error: Option<String>,
    pub text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CopyEntry {
    session: String,
    node: String,
    path: Option<String>,
    at_ms: i64,
    hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WaitEntry {
    session: String,
    node: String,
    name: String,
    title: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PromoteInfo {
    copies: Vec<CopyEntry>,
    claimed_by: Vec<String>,
    waiting: Vec<WaitEntry>,
    status: Option<PromoteStatus>,
}

fn text_of(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn list_of<T>(status: &Value, key: &str, read: impl Fn(&Value) -> Option<T>) -> Vec<T> {
    status
        .get(key)
        .and_then(Value::as_array)
        .map(|all| all.iter().filter_map(read).collect())
        .unwrap_or_default()
}

impl PromoteInfo {
    pub fn of(status: &Value) -> PromoteInfo {
        PromoteInfo {
            copies: list_of(status, "sideCopies", |c| {
                Some(CopyEntry {
                    session: text_of(c, "session")?,
                    node: text_of(c, "node")?,
                    path: text_of(c, "path"),
                    at_ms: c.get("at_ms").and_then(Value::as_i64).unwrap_or(0),
                    hidden: c.get("hidden").and_then(Value::as_bool).unwrap_or(false),
                })
            }),
            claimed_by: list_of(status, "claims", |c| text_of(c, "node")),
            waiting: list_of(status, "waiting", |w| {
                Some(WaitEntry {
                    session: text_of(w, "session")?,
                    node: text_of(w, "node").unwrap_or_default(),
                    name: text_of(w, "name").unwrap_or_default(),
                    title: text_of(w, "title"),
                })
            }),
            status: status
                .get("promote")
                .filter(|p| p.is_object())
                .and_then(|p| {
                    Some(PromoteStatus {
                        node: text_of(p, "node")?,
                        step: PromoteStep::from_name(&text_of(p, "step")?),
                        started: p.get("started").and_then(Value::as_i64).unwrap_or(0),
                        error: text_of(p, "error"),
                        text: text_of(p, "text"),
                    })
                }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub title: String,
    pub message: String,
    pub confirm: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyRow {
    pub session: String,
    pub node: String,
    pub title: String,
    pub line: String,
    pub open: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitRow {
    pub session: String,
    pub title: String,
    pub text: String,
    pub wait: &'static str,
    pub continue_here: &'static str,
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

impl ClusterModel {
    fn silent_primary(&self) -> Option<&NodeRow> {
        self.primary_row().filter(|p| !p.me && !p.online)
    }

    fn name_of(&self, id: &str) -> Option<&str> {
        self.row(id).map(|r| r.name.as_str())
    }

    fn failover_target(&self) -> Option<&NodeRow> {
        let target = self.failover()?.target.as_deref()?;
        self.row(target).filter(|r| r.active)
    }

    pub fn failover_not_ready(&self) -> bool {
        self.failover()
            .is_some_and(|f| f.value != "off" && !f.armed && self.failover_target().is_some())
    }

    fn takeover_line(&self, offset_secs: i64) -> Option<String> {
        let failover = self.failover().filter(|f| f.value != "off")?;
        let at = failover.takes_over_at?;
        let standby = self.failover_target()?;
        let primary = self.silent_primary()?;
        let time = civil_at(at as f64 / 1000.0, offset_secs);
        Some(t::cluster::takes_over(
            &standby.name,
            &clock_text(time.hour, time.minute),
            &primary.name,
        ))
    }

    pub fn failover_notes(&self, offset_secs: i64) -> Vec<Note> {
        let mut notes = Vec::new();
        if self.failover_not_ready() {
            notes.push(Note {
                text: t::settings::cluster_failover_not_ready().to_owned(),
                problem: true,
            });
        }
        if let Some(text) = self.takeover_line(offset_secs) {
            notes.push(Note {
                text,
                problem: false,
            });
        }
        notes
    }

    pub fn promote_status(&self) -> Option<&PromoteStatus> {
        self.promote.status.as_ref()
    }

    pub fn promote_running(&self) -> bool {
        self.promote_status().is_some_and(|p| !p.step.ended())
    }

    pub fn offers_promote(&self, id: &str) -> bool {
        self.cluster_name().is_some()
            && self.link_on()
            && !self.move_running()
            && !self.promote_running()
            && self.silent_primary().is_some()
            && self.row(id).is_some_and(|r| {
                r.active
                    && (r.online || r.me)
                    && r.can_be_master
                    && !r.primary
                    && r.replication.as_ref().is_some_and(|m| m.ready.is_ok())
            })
    }

    pub fn move_kind(&self, id: &str) -> Option<MoveKind> {
        if self.offers_move_now(id) {
            Some(MoveKind::Handover)
        } else if self.offers_promote(id) {
            Some(MoveKind::Promote)
        } else {
            None
        }
    }

    pub fn promote_request(&self, id: &str, confirmed: bool) -> Option<Request> {
        self.offers_promote(id)
            .then(|| wire::promote(id, confirmed))
    }

    pub fn offers_take_back(&self) -> bool {
        let Some(me) = self
            .row(self.me())
            .filter(|r| r.active && r.can_be_master && !r.primary)
        else {
            return false;
        };
        let was_primary = self.promote.copies.iter().any(|c| c.node == me.id)
            || self.promote.claimed_by.iter().any(|node| *node != me.id);
        self.cluster_name().is_some()
            && self.link_on()
            && !self.move_running()
            && was_primary
            && self.primary_row().is_some_and(|p| p.online && !p.me)
            && me.replication.as_ref().is_some_and(|m| m.ready.is_ok())
    }

    pub fn take_back_request(&self) -> Option<Request> {
        self.offers_take_back().then(wire::take_back)
    }

    pub fn take_back_ask(&self) -> Ask {
        Ask {
            title: t::cluster::take_back_title().to_owned(),
            message: t::cluster::take_back_note().to_owned(),
            confirm: t::cluster::take_back().to_owned(),
        }
    }

    fn title_of(&self, names: &Names, session: &str, known: Option<&str>) -> String {
        match names.get(session).filter(|n| !n.is_empty()) {
            Some(name) => name.clone(),
            None => known
                .map(str::to_owned)
                .unwrap_or_else(|| shown(names, session)),
        }
    }

    fn machine_of(&self, node: &str, said: &str) -> String {
        match self.name_of(node) {
            Some(name) => name.to_owned(),
            None if !said.is_empty() => said.to_owned(),
            None => short(node),
        }
    }

    pub fn side_copy_rows(&self, names: &Names, now_ms: i64) -> Vec<CopyRow> {
        let mut copies: Vec<&CopyEntry> =
            self.promote.copies.iter().filter(|c| !c.hidden).collect();
        copies.sort_by_key(|c| std::cmp::Reverse(c.at_ms));
        copies
            .into_iter()
            .map(|c| {
                let age = u64::try_from((now_ms - c.at_ms) / 1000).unwrap_or(0);
                CopyRow {
                    session: c.session.clone(),
                    node: c.node.clone(),
                    title: self.title_of(names, &c.session, None),
                    line: t::cluster::copy_line(&self.machine_of(&c.node, ""), &ago(age)),
                    open: c.path.clone().filter(|_| c.node == self.me()),
                }
            })
            .collect()
    }

    pub fn hide_side_copy(&self, session: &str, node: &str) -> Option<Request> {
        self.promote
            .copies
            .iter()
            .any(|c| !c.hidden && c.session == session && c.node == node)
            .then(|| wire::hide_side_copy(session, node))
    }

    fn waiting_for(&self, session: &str) -> Option<&WaitEntry> {
        self.promote.waiting.iter().find(|w| w.session == session)
    }

    pub fn waiting_rows(&self, names: &Names) -> Vec<WaitRow> {
        self.promote
            .waiting
            .iter()
            .map(|w| {
                let title = self.title_of(names, &w.session, w.title.as_deref());
                let machine = self.machine_of(&w.node, &w.name);
                WaitRow {
                    session: w.session.clone(),
                    text: t::cluster::waits(&title, &machine),
                    title,
                    wait: t::cluster::wait(),
                    continue_here: t::cluster::continue_copy(),
                }
            })
            .collect()
    }

    pub fn continue_ask(&self, session: &str, names: &Names) -> Option<Ask> {
        let w = self.waiting_for(session)?;
        let title = self.title_of(names, session, w.title.as_deref());
        Some(Ask {
            title: t::cluster::continue_title().to_owned(),
            message: t::cluster::continue_note(&title, &self.machine_of(&w.node, &w.name)),
            confirm: t::cluster::continue_copy().to_owned(),
        })
    }

    pub fn continue_request(&self, session: &str) -> Option<Request> {
        self.waiting_for(session)
            .map(|_| wire::continue_here(session))
    }
}
