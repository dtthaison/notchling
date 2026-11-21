use crate::acting_ops::Room;
use crate::consent_ops::ConsentOps;
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use notchling_engine::consent::{consent_of, ConsentStore};
use notchling_events::kinds::choices::AgentTool;
use notchling_events::{MemorySink, Salt};
use notchling_local_api::{AgentIdentity, Caller, ClientKind, Tier};
use serde_json::{json, Value};
use std::sync::Arc;

pub(crate) fn verified(session: &str) -> AgentIdentity {
    AgentIdentity {
        tool: AgentTool::Claude,
        session: Some(session.to_string()),
        verified: true,
        root: Some("Terminal".to_string()),
        folder: None,
        organization: None,
        cmd_hash: None,
        out_hash: None,
    }
}

pub(crate) fn shell(root: &str) -> AgentIdentity {
    AgentIdentity {
        root: Some(root.to_string()),
        ..AgentIdentity::shell()
    }
}

pub(crate) fn consent(
    room: Arc<dyn Room>,
    inbox: Arc<InboxOps>,
    sink: Arc<MemorySink>,
) -> Arc<ConsentOps> {
    consent_at(room, inbox, sink, Arc::new(crate::acting_ops::now_ms))
}

pub(crate) fn consent_at(
    room: Arc<dyn Room>,
    inbox: Arc<InboxOps>,
    sink: Arc<MemorySink>,
    clock: crate::consent_ops::Clock,
) -> Arc<ConsentOps> {
    let consent = ConsentOps::with_clock(
        Arc::new(ConsentStore::in_memory_for_tests()),
        room,
        Some(inbox),
        Some(Events::fixed(sink, Salt([3; 32]))),
        clock,
    );
    consent.bind_identity(Arc::new(|_| verified("s-test")));
    consent.start();
    consent
}

pub(crate) fn agent_caller() -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: Some(4242),
    }
}

pub(crate) fn app_caller() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: Some(1),
    }
}

pub(crate) fn item_for(inbox: &Arc<InboxOps>, id: &str) -> Value {
    inbox.flush();
    inbox
        .items()
        .into_iter()
        .rev()
        .find(|i| consent_of(&i.id) == Some(id))
        .map(|i| serde_json::to_value(i).unwrap())
        .unwrap_or(Value::Null)
}

pub(crate) fn item_id_for(inbox: &Arc<InboxOps>, id: &str) -> String {
    item_for(inbox, id)["id"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn call_op(consent: &ConsentOps, caller: &Caller, op: &str, args: Value) -> Value {
    use crate::op_group::OpGroup;
    consent
        .call(caller, op, &args)
        .unwrap_or_else(|e| json!({"error": e.message, "code": e.code.as_str()}))
}
