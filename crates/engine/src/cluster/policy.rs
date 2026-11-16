use super::id::{NodeId, Role};
use super::members::MemberState;
use super::wire::{Origin, OriginTier};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sent {
    Home,
    Client,
    Member,
    Standby,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    pub op: &'static str,
    pub from: &'static [Sent],
    pub tier: OriginTier,
    pub id_field: Option<&'static str>,
    pub home_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SenderFacts {
    pub id: NodeId,
    pub role: Role,
    pub client: bool,
    pub state: MemberState,
    pub is_home: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denied {
    NotAMember,
    Removed,
    NotAllowed,
    TierTooHigh,
}

const HOME: &[Sent] = &[Sent::Home];
const CLIENT: &[Sent] = &[Sent::Client, Sent::Home];
const CLIENT_ONLY: &[Sent] = &[Sent::Client];
const MEMBER: &[Sent] = &[Sent::Member];
const PHONE_RELAY: &[Sent] = &[Sent::Client, Sent::Standby];

const fn rule(
    op: &'static str,
    from: &'static [Sent],
    tier: OriginTier,
    id_field: Option<&'static str>,
    home_only: bool,
) -> Rule {
    Rule {
        op,
        from,
        tier,
        id_field,
        home_only,
    }
}

const fn home_node(op: &'static str) -> Rule {
    rule(op, HOME, OriginTier::Node, None, false)
}

const fn home_app(op: &'static str) -> Rule {
    rule(op, HOME, OriginTier::App, None, false)
}

const fn home_node_id(op: &'static str) -> Rule {
    rule(op, HOME, OriginTier::Node, Some("id"), false)
}

const fn home_app_id(op: &'static str) -> Rule {
    rule(op, HOME, OriginTier::App, Some("id"), false)
}

const fn client_app(op: &'static str) -> Rule {
    client_app_by(op, "id")
}

const fn client_app_by(op: &'static str, field: &'static str) -> Rule {
    rule(op, CLIENT, OriginTier::App, Some(field), false)
}

const fn client_plain(op: &'static str) -> Rule {
    rule(op, CLIENT, OriginTier::App, None, false)
}

const fn client_home(op: &'static str) -> Rule {
    rule(op, CLIENT_ONLY, OriginTier::App, None, true)
}

const fn phone_relay(op: &'static str) -> Rule {
    rule(op, PHONE_RELAY, OriginTier::App, None, true)
}

pub const RULES: &[Rule] = &[
    home_node("session.new"),
    home_app("session.new"),
    home_node_id("codex.send"),
    home_app_id("codex.send"),
    home_node("agy.open"),
    home_app("agy.open"),
    home_node_id("agy.send"),
    home_app_id("agy.send"),
    home_node("shell.run"),
    home_app("shell.run"),
    home_node_id("shell.stop"),
    home_app_id("shell.stop"),
    home_node_id("session.stop"),
    home_app_id("session.stop"),
    home_node_id("codex.stop"),
    home_app_id("codex.stop"),
    home_node_id("agy.cancel"),
    home_app_id("agy.cancel"),
    home_node("node.switch.set"),
    home_app("node.switch.set"),
    home_node("node.put"),
    client_plain("node.put"),
    home_node("node.put.end"),
    client_plain("node.put.end"),
    home_node("snap.offer"),
    home_node("snap.part"),
    home_node("snap.commit"),
    rule("snap.ack", MEMBER, OriginTier::Node, None, true),
    home_node("move.freeze"),
    home_node("move.stop"),
    home_node("move.switch"),
    rule("move.resume", MEMBER, OriginTier::Node, None, true),
    home_node("move.undo"),
    home_node("workspace.prepare"),
    client_plain("workspace.prepare"),
    home_node("workspace.return"),
    client_plain("workspace.return"),
    rule("delegate.run", HOME, OriginTier::Agent, None, false),
    client_app("session.reply"),
    client_app("session.send"),
    client_app_by("outbox.list", "session"),
    client_app_by("outbox.edit", "session"),
    client_app_by("outbox.remove", "session"),
    client_app_by("outbox.resend", "session"),
    client_app_by("outbox.discard", "session"),
    client_app("session.answer"),
    client_app("session.permission"),
    client_app("session.interrupt"),
    client_app("session.stop"),
    client_app("codex.send"),
    client_app("codex.stop"),
    client_app("agy.send"),
    client_app("agy.cancel"),
    client_app("agy.permission"),
    client_app("agy.conversation"),
    client_app("prompts.list"),
    client_app("node.tail"),
    client_plain("node.follow"),
    client_app("shell.stop"),
    client_home("cluster.dispatch"),
    phone_relay("session.new"),
    phone_relay("cluster.dispatch"),
    client_home("cluster.member.set"),
    client_home("cluster.remove"),
    client_home("cluster.main.set"),
    client_home("cluster.move_home"),
    client_home("cluster.move.now"),
    client_home("cluster.failover.set"),
    rule("node.sessions", CLIENT, OriginTier::Node, None, false),
    rule("node.versions", MEMBER, OriginTier::Node, None, false),
    rule("node.load", MEMBER, OriginTier::Node, None, false),
    rule("cluster.join.token", MEMBER, OriginTier::Agent, None, true),
    rule("cluster.join.token", MEMBER, OriginTier::App, None, true),
    rule("cluster.dispatch", MEMBER, OriginTier::Agent, None, true),
    home_app("install.plan"),
    home_app("install.apply"),
];

fn stands(sender: &SenderFacts, sent: Sent) -> bool {
    match sent {
        Sent::Home => sender.is_home,
        Sent::Client => sender.client,
        Sent::Member => true,
        Sent::Standby => sender.role == Role::Standby,
    }
}

pub fn check(op: &str, sender: &SenderFacts, origin: &Origin) -> Result<&'static Rule, Denied> {
    match sender.state {
        MemberState::Pending => return Err(Denied::NotAMember),
        MemberState::Removed => return Err(Denied::Removed),
        MemberState::Active => {}
    }
    if origin.node != sender.id {
        return Err(Denied::NotAllowed);
    }
    if origin.tier == OriginTier::App
        && !sender.client
        && !sender.is_home
        && sender.role != Role::Standby
    {
        return Err(Denied::TierTooHigh);
    }
    let mut highest: Option<OriginTier> = None;
    for rule in RULES.iter().filter(|r| r.op == op) {
        if !rule.from.iter().any(|s| stands(sender, *s)) {
            continue;
        }
        if rule.tier == origin.tier {
            return Ok(rule);
        }
        highest = Some(highest.map_or(rule.tier, |h| h.max(rule.tier)));
    }
    match highest {
        Some(h) if origin.tier > h => Err(Denied::TierTooHigh),
        _ => Err(Denied::NotAllowed),
    }
}

pub fn cluster_may_set(switch: &str) -> bool {
    matches!(switch, "acting" | "bridge" | "install")
}
