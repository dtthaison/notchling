use super::{Ops, Wired};
use crate::account_asks::follow::{AccountFollow, AccountTap};
use crate::account_asks::AccountAsks;
use crate::acting_ops::Room;
use crate::layout::Layout;
use notchling_local_api::{ApiError, Caller};
use serde_json::Value;
use std::sync::Arc;

pub(super) fn tapped(room: &Arc<dyn Room>) -> (Arc<AccountFollow>, Arc<dyn Room>) {
    let follow = AccountFollow::new(Arc::new(crate::acting_ops::now_ms));
    let tap: Arc<dyn Room> = AccountTap::new(room.clone(), follow.clone());
    (follow, tap)
}

impl Ops {
    pub fn with_account_asks(mut self, asks: Arc<AccountAsks>) -> Ops {
        self.account_asks = Some(asks.clone());
        self.with_group(asks)
    }

    pub fn with_cited(mut self, asks: Arc<dyn crate::cited::Cited>) -> Ops {
        self.cited.push(asks);
        self
    }

    pub(super) fn asked(
        &self,
        caller: &Caller,
        op: &str,
        args: &Value,
    ) -> Option<Result<Value, ApiError>> {
        let run = |a: &Value| self.dispatch(caller, op, a);
        if let Some(asks) = self.cluster_asks.as_ref().filter(|a| a.claims(op, args)) {
            return Some(asks.apply(caller, op, args, &run));
        }
        if let Some(asks) = self.account_asks.as_ref().filter(|a| a.claims(op, args)) {
            return Some(asks.apply(caller, op, args, &run));
        }
        let asks = self.cited.iter().find(|a| a.claims(op, args))?;
        Some(asks.apply(caller, op, args, &run))
    }
}

impl Wired {
    pub(super) fn cluster_asks_for(
        &self,
        layout: &Layout,
    ) -> Arc<crate::cluster_asks::ClusterAsks> {
        self.cluster_asks
            .get_or_init(|| crate::daemon_wiring_setup::cluster_asks(self, layout))
            .clone()
    }

    pub(super) fn account_asks_for(&self, layout: &Layout) -> Arc<AccountAsks> {
        self.account_asks
            .get_or_init(|| crate::daemon_wiring_setup::account_asks(self, layout))
            .clone()
    }
}
