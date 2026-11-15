use super::invariants::Violation;
use super::secrets::{RECOVERY_MS, STEADY_MS};
use super::world::World;
use crate::cluster::logins::argv_read_only;
use crate::inbox::{ActionId, State};

pub(super) fn check_secrets(world: &World) -> Result<(), Violation> {
    let s = world.secrets();
    if let Some((node, path)) = &s.leak {
        return Err(Violation::SecretLeaked {
            node: node.clone(),
            path: path.clone(),
        });
    }
    for (node, store) in &s.stores {
        for (job, _) in store.keys() {
            if !s.truth.get(job).is_some_and(|nodes| nodes.contains(node)) {
                return Err(Violation::GrantOnWrongNode {
                    job: job.clone(),
                    node: node.clone(),
                });
            }
        }
    }
    if let Some((node, _)) = s.login_runs.iter().find(|(_, argv)| !argv_read_only(argv)) {
        return Err(Violation::RefreshOnWorker { node: node.clone() });
    }
    for (item, times) in &s.answered {
        if *times > 1 {
            return Err(Violation::AnswerTwice { item: item.clone() });
        }
    }
    if let Some(a) = s.stale_accepts.first() {
        return Err(Violation::ReleasedOffPrimary {
            node: a.node.clone(),
            term: a.release.clone(),
        });
    }
    for r in &s.releases {
        let at_vault = s.vault_node.as_ref() == Some(&r.node);
        let own = at_vault && r.vault == r.node && r.home == r.node && r.term.node == r.node;
        if !own {
            return Err(Violation::ReleasedOffPrimary {
                node: r.node.clone(),
                term: r.term.clone(),
            });
        }
        let book = s.books.get(&r.node);
        for name in &r.asked {
            let allowed = book.is_some_and(|b| {
                b.items().iter().any(|i| {
                    i.source.job.as_deref() == Some(r.job.as_str())
                        && i.detail.split(',').any(|n| n == name)
                        && i.state == State::Answered
                        && matches!(i.answer, Some(ActionId::AllowOnce | ActionId::AllowAlways))
                        && i.closed_ms.is_some_and(|c| c as u64 <= r.at_ms)
                })
            });
            if !allowed {
                return Err(Violation::ReleasedWithoutAllow { job: r.job.clone() });
            }
        }
    }
    let now = world.now();
    for (node, store) in &s.stores {
        if !world.is_up(node) {
            continue;
        }
        let alive = s.alive_since.get(node).copied();
        let reach = s.reach.get(node).copied().flatten();
        for (job, _) in store.keys() {
            let home = world.state_of(node).map(|st| st.home);
            let told_at = home.and_then(|h| s.ended.get(&(h, job.clone())));
            let (base, told) = match (s.local_ended.get(&(node.clone(), job.clone())), told_at) {
                (Some(t), _) => (*t, false),
                (None, Some(t)) => (*t, true),
                _ => continue,
            };
            let ready = match (alive, told, reach) {
                (None, ..) | (_, true, None) => continue,
                (Some(a), false, _) => a,
                (Some(a), true, Some(r)) => a.max(r),
            };
            let (since, limit) = if ready <= base {
                (base, STEADY_MS)
            } else {
                (ready, RECOVERY_MS)
            };
            if now.saturating_sub(since) > limit {
                return Err(Violation::GrantOutlivedJob {
                    job: job.clone(),
                    node: node.clone(),
                });
            }
        }
    }
    Ok(())
}
