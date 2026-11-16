use crate::cluster_move::lock;
use crate::cluster_promote::{Answer, Ctx, Promoter};
use crate::cluster_promote_facts::{moving_ids, shared_members};
use notchling_engine::cluster::core::Takeover;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::outward::cut_steps;
use notchling_engine::cluster::promote::{PromoteAction, PromoteEvent, RestoredSession};
use notchling_engine::cluster::state::Command;
use notchling_engine::cluster::{NodeId, Tool};
use notchling_engine::transcript::conversation;
use serde_json::json;
use std::sync::mpsc::channel;
use std::time::Duration;

fn name_of(ctx: &Ctx, id: &NodeId) -> String {
    ctx.facts
        .state
        .members
        .get(id)
        .map_or_else(|| id.to_string(), |m| m.name.clone())
}

impl Promoter {
    pub(crate) fn perform(&self, ctx: &mut Ctx, action: PromoteAction) -> Option<PromoteEvent> {
        match action {
            PromoteAction::Probe {
                primary,
                via,
                within_ms,
            } => Some(self.probe(&primary, &via, within_ms)),
            PromoteAction::Relayed { text } => {
                lock(&self.run).text = Some(text.clone());
                ctx.answer(Answer::Relayed(text));
                None
            }
            PromoteAction::NeedsConfirm { text } => {
                {
                    let mut run = lock(&self.run);
                    run.text = Some(text.clone());
                    run.confirm = true;
                }
                ctx.answer(Answer::NeedsConfirm(text));
                None
            }
            PromoteAction::Fail { reason } => {
                self.failed(ctx, reason);
                None
            }
            PromoteAction::Revoke | PromoteAction::PushTerm { .. } => None,
            PromoteAction::Journal(command) => self.journal(ctx, *command),
            PromoteAction::Restore { sessions } => {
                Some(PromoteEvent::Restored(Ok(self.cut(&sessions))))
            }
            PromoteAction::Resume { session, .. } => {
                self.resume(ctx, &session);
                None
            }
            PromoteAction::RunHeld => Some(PromoteEvent::HeldRun),
            PromoteAction::PausedStep { session, step } => {
                let old = name_of(ctx, &ctx.facts.state.home);
                let term = ctx.facts.state.term.n.saturating_add(1);
                let k = ctx.steps.entry(session.clone()).or_default();
                self.items().paused_step(term, &session, *k, &old, &step);
                *k += 1;
                None
            }
            PromoteAction::Announce(notice) => {
                lock(&self.run).text = Some(notice.text.clone());
                let term = ctx.facts.state.term.n.saturating_add(1);
                self.items().failover(term, &notice.text);
                Some(PromoteEvent::Announced)
            }
            PromoteAction::ClusterChanged => None,
        }
    }

    pub(crate) fn after_batch(&self, ctx: &mut Ctx) -> Option<PromoteEvent> {
        if !std::mem::take(&mut ctx.resumed) {
            return None;
        }
        let failed = std::mem::take(&mut ctx.failed);
        let me = self.p.mover.me()?;
        let term = ctx.facts.state.term.n.saturating_add(1);
        for (session, why) in &failed {
            let title = format!("Couldn't start {session} on {}", name_of(ctx, &me));
            self.items()
                .failed(&format!("promote-resume:{term}:{session}"), &title, why);
        }
        Some(PromoteEvent::Resumed { failed })
    }

    fn failed(&self, ctx: &mut Ctx, reason: String) {
        lock(&self.run).error = Some(reason.clone());
        if ctx.answer(Answer::Failed(reason.clone())) {
            return;
        }
        let term = ctx.facts.state.term.n.saturating_add(1);
        let title = "The move to this machine stopped";
        self.items().failed(
            &format!("promote-failed:{term}:{}", ctx.run),
            title,
            &reason,
        );
    }

    fn probe(&self, primary: &NodeId, via: &[NodeId], within_ms: u64) -> PromoteEvent {
        let (tx, rx) = channel();
        let wait = Duration::from_millis(within_ms);
        let mut asked = 0;
        let cluster = self.p.cluster.clone();
        let (target, sender) = (primary.clone(), tx.clone());
        std::thread::spawn(move || {
            let seen = cluster.call(&target, "node.load", json!({}), wait).is_ok();
            let _ = sender.send(seen);
        });
        asked += 1;
        for peer in via {
            let (cluster, target, sender) = (self.p.cluster.clone(), primary.clone(), tx.clone());
            let peer = peer.clone();
            std::thread::spawn(move || {
                let args = json!({"node": target});
                let seen = cluster
                    .call(&peer, "node.reach", args, wait)
                    .is_ok_and(|v| v["online"].as_bool() == Some(true));
                let _ = sender.send(seen);
            });
            asked += 1;
        }
        drop(tx);
        let deadline = std::time::Instant::now() + wait;
        for _ in 0..asked {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match rx.recv_timeout(left) {
                Ok(true) => return PromoteEvent::Answer,
                Ok(false) => {}
                Err(_) => break,
            }
        }
        PromoteEvent::NoAnswer
    }

    fn journal(&self, ctx: &mut Ctx, command: Command) -> Option<PromoteEvent> {
        match command {
            Command::Term { term } => Some(PromoteEvent::Bumped(self.bump(ctx, term))),
            Command::Claim { claim } => {
                let session = claim.session.clone();
                if let Err(e) = self.p.cluster.record(Command::Claim { claim }) {
                    ctx.unclaimed.insert(session, e.message);
                }
                None
            }
            _ => None,
        }
    }

    fn bump(&self, ctx: &Ctx, term: Term) -> Result<(), String> {
        let state = &ctx.facts.state;
        let takeover = Takeover {
            id: state.id.clone(),
            name: state.name.clone(),
            term,
            vault: state.vault.clone(),
            members: shared_members(state),
            sessions: moving_ids(&ctx.facts),
            set: ctx.facts.set.as_ref().map(|set| set.id.clone()),
            claims: state.claims.values().cloned().collect(),
            restored: None,
            staged: false,
        };
        self.p
            .mover
            .take_over(takeover, &state.home)
            .map(|_| ())
            .map_err(|e| e.message)
    }

    fn cut(&self, sessions: &[String]) -> Vec<RestoredSession> {
        let main = self.p.mover.mains();
        sessions
            .iter()
            .map(|id| {
                let transcript = main
                    .get(id)
                    .filter(|m| m.tool == Tool::Claude)
                    .and_then(|m| m.transcript.as_deref());
                RestoredSession {
                    session: id.clone(),
                    cut: transcript
                        .map(|path| cut_steps(&conversation::read(path)))
                        .unwrap_or_default(),
                }
            })
            .collect()
    }

    fn resume(&self, ctx: &mut Ctx, session: &str) {
        ctx.resumed = true;
        if let Some(why) = ctx.unclaimed.get(session) {
            let why = format!("its claim wasn't recorded: {why}");
            ctx.failed.push((session.to_string(), why));
            return;
        }
        for (id, result) in self.p.mover.start_here(&[session.to_string()]) {
            if let Err(why) = result {
                ctx.failed.push((id, why));
            }
        }
    }
}
