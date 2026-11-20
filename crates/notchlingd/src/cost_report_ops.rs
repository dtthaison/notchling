use crate::op_group::OpGroup;
use notchling_engine::cost::check::{Options, Plan, DEFAULT_DAYS};
use notchling_engine::cost::CostService;
use notchling_local_api::{ApiError, Caller, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const COST_REPORT: &str = "cost.report";
pub const MAX_DAYS: u32 = 30;
pub const BUDGET: Duration = Duration::from_secs(20);

pub struct CostReportOps {
    service: Arc<CostService>,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    budget: Duration,
}

fn system_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

fn days_of(args: &Value) -> Result<u32, ApiError> {
    let bad = || ApiError::bad_request(format!("days is a whole number from 1 to {MAX_DAYS}"));
    match args.get("days") {
        None | Some(Value::Null) => Ok(DEFAULT_DAYS),
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=u64::from(MAX_DAYS)).contains(n))
            .map(|n| n as u32)
            .ok_or_else(bad),
    }
}

fn plan_of(args: &Value) -> Result<Plan, ApiError> {
    match args.get("plan") {
        None | Some(Value::Null) => Ok(Plan::Unknown),
        Some(Value::String(s)) => Plan::parse(s)
            .ok_or_else(|| ApiError::bad_request("plan is \"api\", \"subscription\" or \"auto\"")),
        Some(_) => Err(ApiError::bad_request(
            "plan is \"api\", \"subscription\" or \"auto\"",
        )),
    }
}

pub fn shared(service: &Arc<CostService>) -> Arc<CostReportOps> {
    Arc::new(CostReportOps::new(service.clone()))
}

impl CostReportOps {
    pub fn new(service: Arc<CostService>) -> CostReportOps {
        CostReportOps {
            service,
            now: Arc::new(system_now),
            budget: BUDGET,
        }
    }

    pub fn with_clock(mut self, now: Arc<dyn Fn() -> f64 + Send + Sync>) -> CostReportOps {
        self.now = now;
        self
    }

    pub fn with_budget(mut self, budget: Duration) -> CostReportOps {
        self.budget = budget;
        self
    }

    pub fn report(&self, args: &Value) -> Result<Value, ApiError> {
        let mut options = Options::new((self.now)());
        options.days = days_of(args)?;
        options.plan = plan_of(args)?;
        let checked = self
            .service
            .check(options, self.budget)
            .ok_or_else(|| ApiError::new(ErrorCode::Failed, "the usage ledger is not available"))?;
        let report = serde_json::to_value(&checked.report)
            .map_err(|e| ApiError::internal(format!("report: {e}")))?;
        Ok(json!({
            "report": report,
            "complete": checked.complete,
            "scanned": checked.scanned,
            "ledger_calls": checked.calls,
        }))
    }
}

impl OpGroup for CostReportOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == COST_REPORT).then_some(OpSpec::AGENT)
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            COST_REPORT => self.report(args),
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }
}
