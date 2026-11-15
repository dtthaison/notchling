use crate::acting_events::{session_ref, PlayError};
use crate::acting_ops::{ActingOps, NoLog};
use notchling_engine::reply::held::{after_tried, Held, Route, Tried};
use notchling_engine::reply::plan;
use notchling_engine::sessions::{Agent, AgentGroup};
use notchling_events::kinds::agents::ReplyVia;
use notchling_events::kinds::choices::FailWhy;
use notchling_local_api::topics;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

pub const HELD_TICK: Duration = Duration::from_secs(1);

pub const BOX_NOTE: &str = "Waiting — the terminal box has other text";
pub const UNSURE_NOTE: &str =
    "It may already be in the terminal. Check it, then send again or discard.";
const SENT_SLACK_MS: i64 = 2_000;

#[derive(Default)]
pub struct HeldState {
    held: Mutex<Held>,
    stop: Mutex<Option<Sender<()>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn note_of(failure: &PlayError) -> Option<String> {
    match failure.why {
        FailWhy::NotIdle => None,
        FailWhy::NotCleared => Some(BOX_NOTE.to_owned()),
        _ => Some(failure.err.message.clone()),
    }
}

impl ActingOps {
    pub(crate) fn reply_or_hold(
        &self,
        agent: &Agent,
        text: &str,
        now_ms: i64,
        clear_box: bool,
    ) -> Result<Value, PlayError> {
        if !agent.can_reply() {
            self.play(agent, &plan::reply(text), &mut NoLog)?;
            return Ok(json!({"sent": true}));
        }
        let working = agent.turn_running() || self.new_turn_since(agent, now_ms);
        let route = lock(&self.held.held).route(&agent.id, text, working, now_ms);
        match route {
            Route::Hold(all) => {
                let note = self.held_note(&agent.id);
                self.publish_held(&agent.id, Some(&all), note.as_deref());
                Ok(json!({"sent": true, "state": "queued", "waiting": 1, "held": all}))
            }
            Route::Send => match self.play_reply(agent, text, clear_box) {
                Ok(_) => Ok(json!({"sent": true})),
                Err(e) => {
                    lock(&self.held.held).not_sent(&agent.id);
                    Err(e)
                }
            },
        }
    }

    pub fn held_text(&self, id: &str) -> Option<String> {
        lock(&self.held.held).get(id).map(String::from)
    }

    fn new_turn_since(&self, agent: &Agent, now_ms: i64) -> bool {
        if agent.group != AgentGroup::Working || !agent.at_prompt {
            return false;
        }
        let fresh = self.rest.fresh(agent);
        let row = fresh.as_ref().unwrap_or(agent);
        row.turn_running() || self.rest.restarted(row, now_ms)
    }

    pub(crate) fn calm_now(&self, cached: &Agent, now_ms: i64) -> bool {
        let Some(fresh) = self.rest.fresh(cached) else {
            return false;
        };
        if !fresh.takes_held_text() {
            return false;
        }
        let path = fresh
            .transcript_path
            .as_ref()
            .or(cached.transcript_path.as_ref());
        let row = Agent {
            transcript_path: path.cloned(),
            ..fresh
        };
        row.group != AgentGroup::Working || !self.rest.restarted(&row, now_ms)
    }

    pub fn flush_held(&self, now_ms: i64) {
        if !self.acting.is_on() {
            return;
        }
        let rows = self.agents_now();
        let waiting = lock(&self.held.held).waiting();
        let calm: BTreeSet<String> = waiting
            .into_iter()
            .filter(|id| {
                rows.iter()
                    .find(|row| &row.id == id)
                    .is_some_and(|row| row.takes_held_text() && self.calm_now(row, now_ms))
            })
            .collect();
        let ready = lock(&self.held.held).ready(&rows, now_ms, &calm);
        for (id, text) in ready {
            let Some(agent) = rows.iter().find(|a| a.id == id) else {
                continue;
            };
            let Some(text) = self.after_earlier_try(agent, &id, text, now_ms) else {
                continue;
            };
            let chars = text.chars().count();
            let guard = || self.calm_now(agent, now_ms);
            match self.play_guarded(agent, &plan::reply(&text), &mut NoLog, &guard) {
                Ok(_) => {
                    self.note_reply(session_ref(agent), ReplyVia::Held, chars, None, None, false);
                    lock(&self.held.held).settled_ok(&id);
                    let rest = self.held_text(&id);
                    self.publish_held(&id, rest.as_deref(), None);
                }
                Err(failure) => {
                    self.note_reply(
                        session_ref(agent),
                        ReplyVia::Held,
                        chars,
                        Some(failure.why),
                        None,
                        false,
                    );
                    let note = note_of(&failure);
                    let mut held = lock(&self.held.held);
                    held.failed(&id, &text, now_ms, failure.typed, note.as_deref());
                    let all = held.get(&id).map(String::from);
                    let note = held.note(&id).map(String::from);
                    drop(held);
                    self.publish_held(&id, all.as_deref(), note.as_deref());
                }
            }
        }
    }

    fn held_note(&self, id: &str) -> Option<String> {
        lock(&self.held.held).note(id).map(String::from)
    }

    fn after_earlier_try(
        &self,
        agent: &Agent,
        id: &str,
        text: String,
        now_ms: i64,
    ) -> Option<String> {
        let Some(tried) = lock(&self.held.held).tried(id) else {
            return Some(text);
        };
        let went_in = text.get(..tried.len).unwrap_or(&text);
        match self.sent_since_try(agent, went_in, tried) {
            Some(false) => Some(text),
            Some(true) => {
                let rest = after_tried(&text, tried).to_owned();
                self.note_reply(
                    session_ref(agent),
                    ReplyVia::Held,
                    went_in.chars().count(),
                    None,
                    None,
                    false,
                );
                let mut held = lock(&self.held.held);
                held.forget_tried(id);
                if rest.is_empty() {
                    held.settled_ok(id);
                }
                let shown = match rest.is_empty() {
                    true => held.get(id).map(String::from),
                    false => Some(rest.clone()),
                };
                drop(held);
                self.publish_held(id, shown.as_deref(), None);
                (!rest.is_empty()).then_some(rest)
            }
            None => {
                let mut held = lock(&self.held.held);
                let said = held.note(id) == Some(UNSURE_NOTE);
                held.failed(id, &text, now_ms, false, Some(UNSURE_NOTE));
                let all = held.get(id).map(String::from);
                drop(held);
                if !said {
                    self.publish_held(id, all.as_deref(), Some(UNSURE_NOTE));
                }
                None
            }
        }
    }

    fn sent_since_try(&self, agent: &Agent, text: &str, tried: Tried) -> Option<bool> {
        let fresh = self.rest.fresh(agent);
        let path = fresh
            .as_ref()
            .and_then(|row| row.transcript_path.as_ref())
            .or(agent.transcript_path.as_ref())?;
        let row = Agent {
            transcript_path: Some(path.clone()),
            ..agent.clone()
        };
        self.rest
            .delivered(&row, text, tried.at.saturating_sub(SENT_SLACK_MS))
    }

    pub fn spawn_held_loop(self: &Arc<Self>) {
        let mut stop = lock(&self.held.stop);
        if stop.is_some() {
            return;
        }
        let (tx, rx) = channel::<()>();
        let ops = Arc::downgrade(self);
        let spawned = std::thread::Builder::new()
            .name("notchling-held".into())
            .spawn(move || {
                while let Err(RecvTimeoutError::Timeout) = rx.recv_timeout(HELD_TICK) {
                    let Some(ops) = ops.upgrade() else { return };
                    ops.flush_held(now_ms());
                }
            });
        if spawned.is_ok() {
            *stop = Some(tx);
        }
    }

    pub fn stop_held_loop(&self) {
        lock(&self.held.stop).take();
    }

    fn publish_held(&self, id: &str, text: Option<&str>, note: Option<&str>) {
        let mut data = json!({"id": id, "text": text});
        if let (Some(note), Some(map)) = (note.filter(|_| text.is_some()), data.as_object_mut()) {
            map.insert("note".into(), json!(note));
        }
        self.room
            .publish(topics::TOOLS, topics::SESSION_HELD, &data);
    }

    pub(crate) fn held_discard(&self, args: &Value) -> Result<Value, ApiError> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                ApiError::new(ErrorCode::BadRequest, "session.held.discard takes {\"id\"}")
            })?;
        let found = lock(&self.held.held).discard(id);
        if found {
            self.publish_held(id, None, None);
        }
        Ok(json!({"discarded": found}))
    }
}
