use crate::layout::Layout;
use crate::op_group::OpGroup;
use notchling_engine::activity::{self, EventsDb, ImportFiles, Query};
use notchling_local_api::{ops, ApiError, Caller, OpSpec};
use serde_json::Value;
use std::sync::{Arc, Mutex, PoisonError};

pub type Names = Arc<dyn Fn(&str, &str) -> Option<String> + Send + Sync>;

pub struct ActivityOps {
    db: Arc<Mutex<EventsDb>>,
    names: Names,
}

impl ActivityOps {
    pub fn new(db: Arc<Mutex<EventsDb>>, names: Names) -> ActivityOps {
        ActivityOps { db, names }
    }

    pub fn for_daemon(layout: &Layout) -> ActivityOps {
        let mut db = EventsDb::open(&layout.events_db_path())
            .or_else(|_| EventsDb::memory())
            .unwrap_or_else(|e| panic!("no in-memory database: {e}"));
        let files = ImportFiles {
            audit: vec![
                layout.audit_path().with_extension("jsonl.1"),
                layout.audit_path(),
            ],
            remote_log: layout.remote_log_path(),
            account_log: layout.account_log_path(),
        };
        let _ = activity::import_once(&mut db, &files, "0000000000000000");
        ActivityOps::new(Arc::new(Mutex::new(db)), Arc::new(|_, _| None))
    }
}

impl OpGroup for ActivityOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::ACTIVITY_LIST).then_some(OpSpec::APP_QUIET)
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::ACTIVITY_LIST => {
                let query = Query::from_args(args).map_err(ApiError::bad_request)?;
                let db = self.db.lock().unwrap_or_else(PoisonError::into_inner);
                Ok(activity::list(&db, &query, &*self.names))
            }
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }
}
