use crate::cluster_move::{failed, Mover, MOVE_OPS};
use crate::cluster_node::{api_error, LocalCalls};
use crate::snap_restore::DbHooks;
use notchling_engine::cluster::core::Takeover;
use notchling_engine::cluster::move_home::{
    MOVE_FREEZE, MOVE_RESUME, MOVE_STOP, MOVE_SWITCH, MOVE_UNDO,
};
use notchling_engine::cluster::persist;
use notchling_engine::cluster::wire::Origin;
use notchling_engine::cluster::NodeId;
use notchling_engine::inbox::{Item, State as ItemState};
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;

pub struct MoveCalls {
    inner: Arc<dyn LocalCalls>,
    mover: Arc<Mover>,
}

impl MoveCalls {
    pub fn new(inner: Arc<dyn LocalCalls>, mover: Arc<Mover>) -> MoveCalls {
        MoveCalls { inner, mover }
    }
}

fn bad(why: &str) -> ApiError {
    ApiError::new(ErrorCode::BadRequest, why)
}

fn ids(args: &Value) -> Result<Vec<String>, ApiError> {
    let list = args
        .get("sessions")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("sessions must be a list of ids"))?;
    Ok(list
        .iter()
        .filter_map(|id| id.as_str().map(str::to_string))
        .collect())
}

fn results(rows: Vec<(String, Result<(), String>)>) -> Value {
    let list: Vec<Value> = rows
        .into_iter()
        .map(|(id, result)| match result {
            Ok(()) => json!({"id": id, "ok": true}),
            Err(why) => json!({"id": id, "ok": false, "error": why}),
        })
        .collect();
    json!({"results": list})
}

impl Mover {
    pub fn switch_here(&self, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        let mut takeover: Takeover = serde_json::from_value(args.clone())
            .map_err(|_| bad("move.switch takes a takeover"))?;
        let me = self
            .me()
            .ok_or_else(|| failed("this machine is in no cluster"))?;
        let status = self.p.link.status();
        let held = status["cluster"]["home"].as_str() == Some(me.as_str());
        let current = status["cluster"]["term"]["n"].as_u64().unwrap_or(0);
        if held && current == takeover.term.n {
            return self.p.link.take_primary(takeover).map_err(api_error);
        }
        if takeover.term.node != me || takeover.term.n <= current {
            return Err(failed("that term isn't this machine's to take"));
        }
        let name = self.name_of(&me);
        let sessions: BTreeSet<String> = takeover.sessions.iter().cloned().collect();
        let earlier: Vec<Item> = self
            .p
            .inbox
            .items()
            .into_iter()
            .filter(|item| item.state == ItemState::Open)
            .collect();
        let inbox = self.p.inbox.clone();
        let again = self.p.inbox.clone();
        let hooks = DbHooks {
            close: &move || {
                inbox.flush();
                Ok(())
            },
            reopen: &move || {
                again.load();
                Ok(())
            },
        };
        self.p
            .restore
            .restore(&origin.node, &name, &sessions, &hooks)
            .map_err(|why| failed(format!("restoring from the snapshot failed: {why}")))?;
        let restored = persist::load(&self.p.cluster)
            .map_err(|why| failed(format!("the restored database can't be read: {why}")))?
            .ok_or_else(|| failed("the restored database holds no cluster"))?;
        takeover.restored = Some(restored);
        for item in earlier {
            if !self.p.inbox.items().iter().any(|kept| kept.id == item.id) {
                self.p.inbox.raise(item, None);
            }
        }
        self.p.link.take_primary(takeover).map_err(api_error)
    }

    pub fn resume_op(&self, args: &Value) -> Result<Value, ApiError> {
        Ok(results(self.start_here(&ids(args)?)))
    }
}

impl LocalCalls for MoveCalls {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        if !MOVE_OPS.contains(&op) {
            return self.inner.call(op, args, origin);
        }
        match op {
            MOVE_SWITCH => self.mover.switch_here(args, origin),
            MOVE_RESUME => self.mover.resume_op(args),
            MOVE_FREEZE => {
                let to = args
                    .get("to")
                    .and_then(Value::as_str)
                    .unwrap_or("another machine");
                self.mover.freeze(&ids(args)?, to);
                Ok(json!({"frozen": true}))
            }
            MOVE_STOP => {
                let result = self.mover.stop(&ids(args)?);
                Ok(json!({"done": result.done, "failed": result.failed}))
            }
            MOVE_UNDO => {
                self.mover.thaw();
                Ok(results(self.mover.restart(&ids(args)?)))
            }
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

pub fn node_of(text: &str) -> Option<NodeId> {
    NodeId::parse(text)
}
