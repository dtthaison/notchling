use crate::acting_events::{session_ref, PlayError};
use crate::acting_ops::{ActingOps, NoLog};
use notchling_engine::reply::held::{Held, Route};
use notchling_engine::reply::plan;
use notchling_engine::sessions::Agent;
use notchling_events::kinds::agents::ReplyVia;
use notchling_local_api::topics;
use serde_json::{json, Value};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

pub const HELD_TICK: Duration = Duration::from_secs(1);

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

impl ActingOps {
    pub(crate) fn reply_or_hold(
        &self,
        agent: &Agent,
        text: &str,
        now_ms: i64,
    ) -> Result<Value, PlayError> {
        if !agent.can_reply() {
            self.play(agent, &plan::reply(text), &mut NoLog)?;
            return Ok(json!({"sent": true}));
        }
        let route = lock(&self.held.held).route(&agent.id, text, agent.turn_running(), now_ms);
        match route {
            Route::Hold(all) => {
                self.publish_held(&agent.id, Some(&all));
                Ok(json!({"sent": true, "state": "queued", "waiting": 1, "held": all}))
            }
            Route::Send => match self.play(agent, &plan::reply(text), &mut NoLog) {
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

    pub fn flush_held(&self, now_ms: i64) {
        if !self.acting.is_on() {
            return;
        }
        let rows = self.agents_now();
        let ready = lock(&self.held.held).ready(&rows, now_ms);
        for (id, text) in ready {
            let Some(agent) = rows.iter().find(|a| a.id == id) else {
                continue;
            };
            let chars = text.chars().count();
            match self.play(agent, &plan::reply(&text), &mut NoLog) {
                Ok(_) => {
                    self.note_reply(session_ref(agent), ReplyVia::Held, chars, None, None, false);
                    let rest = self.held_text(&id);
                    self.publish_held(&id, rest.as_deref());
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
                    let mut held = lock(&self.held.held);
                    held.put_back(&id, &text, now_ms);
                    let all = held.get(&id).map(String::from);
                    drop(held);
                    self.publish_held(&id, all.as_deref());
                }
            }
        }
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

    fn publish_held(&self, id: &str, text: Option<&str>) {
        self.room.publish(
            topics::TOOLS,
            topics::SESSION_HELD,
            &json!({"id": id, "text": text}),
        );
    }
}
