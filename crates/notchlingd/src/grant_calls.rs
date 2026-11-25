use crate::acting_ops::now_ms;
use crate::cluster_node::{ClusterLog, ClusterNode, LocalCalls, LOCAL_WAIT};
use crate::grant_broker::{
    Audit, GrantBroker, Link, Notices, Release, Vault, View, GRANT_ATTACH, GRANT_REQUEST,
    GRANT_REVOKE_JOB, LOG_DOWN,
};
use crate::grant_use::{Asker, UseAudit};
use crate::inbox_ops::InboxOps;
use crate::vault_ops::{Looked, ReleaseFor, Released, Still, VaultOps};
use notchling_engine::cluster::core::ATTACH_CALL_MS;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::{ClusterState, Job, LocalCmd, NodeId};
use notchling_engine::inbox::{Action, ActionId, Item, Kind, Source, State};
use notchling_events::kinds::security::ClusterStep;
use notchling_events::spec::Outcome;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::Value;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub type Holder = Arc<OnceLock<Arc<ClusterNode>>>;

fn node_of(holder: &Holder) -> Result<&Arc<ClusterNode>, ApiError> {
    holder
        .get()
        .ok_or_else(|| ApiError::new(ErrorCode::Failed, "the cluster node isn't up"))
}

impl Vault for VaultOps {
    fn lookup(&self, names: &[String]) -> Result<Vec<Looked>, ApiError> {
        VaultOps::lookup(self, names)
    }

    fn release(
        &self,
        who: &ReleaseFor,
        names: &[String],
        still: &Still<'_>,
    ) -> Result<Released, ApiError> {
        VaultOps::release(self, who, names, still)
    }
}

pub struct NodeLink {
    holder: Holder,
}

impl NodeLink {
    pub fn new(holder: Holder) -> NodeLink {
        NodeLink { holder }
    }
}

impl Link for NodeLink {
    fn call(&self, node: &NodeId, op: &str, args: Value) -> Result<Value, ApiError> {
        let this = node_of(&self.holder)?;
        let origin = Origin {
            node: this.me().clone(),
            tier: OriginTier::Node,
        };
        this.local_within(
            LocalCmd::Forward {
                node: node.clone(),
                op: op.to_string(),
                args: args.into(),
                origin,
            },
            LOCAL_WAIT,
        )
    }
}

pub struct NodeAsker {
    holder: Holder,
}

impl NodeAsker {
    pub fn new(holder: Holder) -> NodeAsker {
        NodeAsker { holder }
    }
}

impl Asker for NodeAsker {
    fn request(&self, vault: &NodeId, args: Value) -> Result<Value, ApiError> {
        let this = node_of(&self.holder)?;
        let origin = Origin {
            node: this.me().clone(),
            tier: OriginTier::Agent,
        };
        this.local_within(
            LocalCmd::Forward {
                node: vault.clone(),
                op: GRANT_REQUEST.to_string(),
                args: args.into(),
                origin,
            },
            LOCAL_WAIT + Duration::from_millis(ATTACH_CALL_MS),
        )
    }
}

pub struct ClusterUseAudit {
    log: ClusterLog,
}

impl ClusterUseAudit {
    pub fn new(log: ClusterLog) -> ClusterUseAudit {
        ClusterUseAudit { log }
    }
}

impl UseAudit for ClusterUseAudit {
    fn used(&self, vault: &NodeId, _job: &str, ok: bool) {
        let (step, outcome) = if ok {
            (ClusterStep::Call, Outcome::Ok)
        } else {
            (ClusterStep::Refused, Outcome::Refused)
        };
        self.log
            .step(step, Some(vault.as_str()), Some("vault.use"), outcome);
    }
}

pub struct NodeView {
    holder: Holder,
}

impl NodeView {
    pub fn new(holder: Holder) -> NodeView {
        NodeView { holder }
    }
}

impl View for NodeView {
    fn state(&self) -> Option<ClusterState> {
        self.holder.get().and_then(|node| node.state())
    }

    fn me(&self) -> Option<NodeId> {
        self.holder.get().map(|node| node.me().clone())
    }
}

pub struct EventAudit {
    vault: Arc<VaultOps>,
    log: ClusterLog,
}

impl EventAudit {
    pub fn new(vault: Arc<VaultOps>, log: ClusterLog) -> EventAudit {
        EventAudit { vault, log }
    }
}

impl Audit for EventAudit {
    fn released(&self, release: &Release<'_>) -> Result<(), String> {
        if !self
            .vault
            .note_released(release.who, release.name, release.released)
        {
            return Err(LOG_DOWN.to_string());
        }
        self.log.step(
            ClusterStep::Call,
            Some(release.node.as_str()),
            Some(crate::grant_broker::GRANT_DELIVER),
            Outcome::Ok,
        );
        Ok(())
    }

    fn refused(&self, node: &NodeId, who: &ReleaseFor, name: &str) {
        self.vault.note_refused(who, name);
        self.log.step(
            ClusterStep::Refused,
            Some(node.as_str()),
            Some(crate::grant_broker::GRANT_DELIVER),
            Outcome::Refused,
        );
    }
}

pub struct InboxNotices {
    inbox: Arc<InboxOps>,
}

impl InboxNotices {
    pub fn new(inbox: Arc<InboxOps>) -> InboxNotices {
        InboxNotices { inbox }
    }
}

fn wait_item_id(job: &str) -> String {
    format!("grant-wait:{job}")
}

impl Notices for InboxNotices {
    fn waiting(&self, job: &Job, node_name: &str, vault_name: &str) {
        let item = Item {
            id: wait_item_id(&job.id),
            kind: Kind::Notice,
            title: format!("A job on {node_name} waits for a secret: the vault is on {vault_name}"),
            detail: format!("Job {} wants {}", job.id, job.spec.secrets.join(", ")),
            source: Source {
                node: Some(job.spec.node.to_string()),
                session: None,
                job: Some(job.id.clone()),
            },
            created_ms: now_ms(),
            expires_ms: None,
            actions: vec![Action {
                id: ActionId::Done,
                label: "Done".to_string(),
                primary: false,
            }],
            state: State::Open,
            answer: None,
            answered_by: None,
            seen: false,
            open: None,
            closed_ms: None,
            switch: None,
        };
        self.inbox.raise(item, None);
    }

    fn clear(&self, job: &str) {
        self.inbox
            .resolve(&wait_item_id(job), State::Withdrawn, None, None);
    }
}

pub struct GrantCalls {
    inner: Arc<dyn LocalCalls>,
    broker: Arc<GrantBroker>,
}

impl GrantCalls {
    pub fn new(inner: Arc<dyn LocalCalls>, broker: Arc<GrantBroker>) -> GrantCalls {
        GrantCalls { inner, broker }
    }

    fn own(&self, origin: &Origin) -> Result<(), ApiError> {
        if self.broker.me().as_ref() == Some(&origin.node) && origin.tier == OriginTier::Node {
            Ok(())
        } else {
            Err(ApiError::new(
                ErrorCode::Forbidden,
                "only this node starts that",
            ))
        }
    }
}

impl LocalCalls for GrantCalls {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        match op {
            GRANT_ATTACH => {
                self.own(origin)?;
                self.broker.attach(args)
            }
            GRANT_REVOKE_JOB => {
                self.own(origin)?;
                self.broker.revoke(args)
            }
            GRANT_REQUEST => self.broker.request(origin, args),
            _ => self.inner.call(op, args, origin),
        }
    }
}
