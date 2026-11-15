use crate::cluster_move::{failed, lock, Mover, Started, MOVE_OPS};
use crate::cluster_node::{api_error, refused, LocalCalls};
use crate::snap_restore_fs::staged_db;
use notchling_engine::cluster::core::{eligible, Takeover};
use notchling_engine::cluster::move_home::{
    IN_PROGRESS, MOVE_FREEZE, MOVE_RESUME, MOVE_STOP, MOVE_SWITCH, MOVE_UNDO,
};
use notchling_engine::cluster::persist;
use notchling_engine::cluster::wire::Origin;
use notchling_engine::cluster::NodeId;
use notchling_engine::inbox::{Item, State as ItemState};
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub const TOOK_FILE: &str = "move-took.json";

pub struct MoveCalls {
    inner: Arc<dyn LocalCalls>,
    mover: Arc<Mover>,
}

impl MoveCalls {
    pub fn new(inner: Arc<dyn LocalCalls>, mover: Arc<Mover>) -> MoveCalls {
        MoveCalls { inner, mover }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Took {
    pub term: u64,
    pub from: NodeId,
    pub started: Vec<String>,
}

pub fn read_took(cluster: &Path) -> Option<Took> {
    let bytes = std::fs::read(cluster.join(TOOK_FILE)).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    Some(Took {
        term: value["term"].as_u64()?,
        from: value["from"].as_str().and_then(NodeId::parse)?,
        started: value["started"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| id.as_str().map(str::to_string))
            .collect(),
    })
}

pub fn write_durably(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let temp = dir.join(format!("{name}.tmp"));
    let shown = |e: std::io::Error| format!("{}: {e}", temp.display());
    let mut file = std::fs::File::create(&temp).map_err(shown)?;
    file.write_all(bytes).map_err(shown)?;
    file.sync_all().map_err(shown)?;
    drop(file);
    std::fs::rename(&temp, dir.join(name)).map_err(shown)?;
    if let Ok(folder) = std::fs::File::open(dir) {
        let _ = folder.sync_all();
    }
    Ok(())
}

fn write_took(cluster: &Path, took: &Took) -> Result<(), String> {
    let value = json!({"term": took.term, "from": took.from.as_str(), "started": took.started});
    let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(cluster).map_err(|e| e.to_string())?;
    write_durably(cluster, TOOK_FILE, &bytes)
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

fn results(rows: Started) -> Value {
    let list: Vec<Value> = rows
        .into_iter()
        .map(|(id, result)| match result {
            Ok(()) => json!({"id": id, "ok": true}),
            Err(why) => json!({"id": id, "ok": false, "error": why}),
        })
        .collect();
    json!({"results": list})
}

struct Busy<'a, T: Default>(&'a Mutex<T>);

impl<T: Default> Drop for Busy<'_, T> {
    fn drop(&mut self) {
        *lock(self.0) = T::default();
    }
}

impl Mover {
    fn begin_switch(&self, term: u64) -> Result<Busy<'_, Option<u64>>, ApiError> {
        let mut running = lock(&self.switching);
        if running.is_some() {
            return Err(failed(IN_PROGRESS));
        }
        *running = Some(term);
        Ok(Busy(&self.switching))
    }

    fn begin_resume(&self) -> Result<Busy<'_, bool>, ApiError> {
        let mut running = lock(&self.resuming);
        if *running {
            return Err(failed(IN_PROGRESS));
        }
        *running = true;
        Ok(Busy(&self.resuming))
    }

    pub fn switch_here(&self, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        let takeover: Takeover = serde_json::from_value(args.clone())
            .map_err(|_| bad("move.switch takes a takeover"))?;
        let me = self
            .me()
            .ok_or_else(|| failed("this machine is in no cluster"))?;
        let status = self.p.link.status();
        if status["cluster"].is_null() {
            return Err(failed("this machine's cluster node isn't up yet"));
        }
        let held = status["cluster"]["home"].as_str() == Some(me.as_str());
        let current = status["cluster"]["term"]["n"].as_u64().unwrap_or(0);
        if held && current == takeover.term.n {
            return self.p.link.take_primary(takeover).map_err(api_error);
        }
        let _busy = self.begin_switch(takeover.term.n)?;
        let name = self.name_of(&me);
        if status["cluster"]["id"].as_str() != Some(takeover.id.as_str()) {
            return Err(refused("that is another cluster"));
        }
        if takeover.term.node != me || takeover.term.n <= current {
            return Err(refused("that term isn't this machine's to take"));
        }
        if !eligible(&takeover, &me) {
            return Err(refused(format!(
                "{name} can't be primary: it isn't an active Can Be Master member"
            )));
        }
        let sessions: BTreeSet<String> = takeover.sessions.iter().cloned().collect();
        let set = takeover.set.clone();
        self.p
            .restore
            .check(&origin.node, &name, set.as_deref(), &sessions)
            .map_err(refused)?;
        let took = Took {
            term: takeover.term.n,
            from: origin.node.clone(),
            started: Vec::new(),
        };
        write_took(&self.p.cluster, &took).map_err(|why| failed(format!("{TOOK_FILE}: {why}")))?;
        let stage = staged_db(&self.p.cluster);
        let _ = std::fs::remove_file(&stage);
        let answer = self.stage_and_take(takeover, &origin.node, &name, &stage);
        if answer.is_err() {
            let _ = std::fs::remove_file(&stage);
        }
        answer
    }

    fn stage_and_take(
        &self,
        mut takeover: Takeover,
        primary: &NodeId,
        name: &str,
        stage: &Path,
    ) -> Result<Value, ApiError> {
        let sessions: BTreeSet<String> = takeover.sessions.iter().cloned().collect();
        self.p
            .restore
            .restore(primary, name, &sessions, takeover.set.as_deref(), stage)
            .map_err(|why| refused(format!("restoring from the snapshot failed: {why}")))?;
        let staged_dir = stage.parent().unwrap_or(stage);
        let restored = persist::load(staged_dir)
            .map_err(|why| refused(format!("the restored database can't be read: {why}")))?
            .ok_or_else(|| refused("the restored database holds no cluster"))?;
        if restored.id != takeover.id {
            return Err(refused("the restored database is another cluster's"));
        }
        takeover.restored = Some(restored);
        takeover.staged = true;
        let earlier: Vec<Item> = self
            .p
            .inbox
            .items()
            .into_iter()
            .filter(|item| item.state == ItemState::Open)
            .collect();
        self.p.inbox.flush();
        let answer = self.p.link.take_primary(takeover);
        self.p.inbox.load();
        for item in earlier {
            if !self.p.inbox.items().iter().any(|kept| kept.id == item.id) {
                self.p.inbox.raise(item, None);
            }
        }
        answer.map_err(api_error)
    }

    pub fn resume_op(&self, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        let sessions = ids(args)?;
        let mut took =
            read_took(&self.p.cluster).ok_or_else(|| refused("this machine took no handover"))?;
        let status = self.p.link.status();
        let me = text(&status, "me");
        let home = status["cluster"]["home"].as_str();
        let term = status["cluster"]["term"]["n"].as_u64().unwrap_or(0);
        if me.is_none() || home != me || term != took.term {
            return Err(refused("that handover is over"));
        }
        if origin.node != took.from {
            let name = self.name_of(&took.from);
            return Err(refused(format!("only {name} can resume its sessions here")));
        }
        let _busy = self.begin_resume()?;
        let mut rows = Vec::new();
        for id in sessions {
            if took.started.contains(&id) {
                rows.push((id, Ok(())));
                continue;
            }
            let result = self
                .start_here(std::slice::from_ref(&id))
                .pop()
                .map_or(Ok(()), |(_, result)| result);
            if result.is_ok() {
                took.started.push(id.clone());
                let _ = write_took(&self.p.cluster, &took);
            }
            rows.push((id, result));
        }
        Ok(results(rows))
    }
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

impl LocalCalls for MoveCalls {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        if !MOVE_OPS.contains(&op) {
            return self.inner.call(op, args, origin);
        }
        match op {
            MOVE_SWITCH => self.mover.switch_here(args, origin),
            MOVE_RESUME => self.mover.resume_op(args, origin),
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
                Ok(results(self.mover.restart(&ids(args)?, &[])))
            }
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

pub fn node_of(text: &str) -> Option<NodeId> {
    NodeId::parse(text)
}
