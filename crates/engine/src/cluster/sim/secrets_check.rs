use std::collections::BTreeMap;

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
        for ((job, _), grant) in store {
            let meant = grant.release.map(|r| &s.releases[r]);
            let right_job = meant.is_none_or(|r| r.job == *job);
            if grant.intended.as_ref() != Some(node) || !right_job {
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
    let mut used: BTreeMap<&str, usize> = BTreeMap::new();
    for (rid, r) in s.releases.iter().enumerate() {
        let at_vault = s.vault_node.as_ref() == Some(&r.node);
        let own = at_vault && r.vault == r.node && r.home == r.node && r.term.node == r.node;
        if !own {
            return Err(Violation::ReleasedOffPrimary {
                node: r.node.clone(),
                term: r.term.clone(),
            });
        }
        let book = s.books.get(&r.node);
        for (name, approval) in &r.asked {
            let item = approval
                .as_deref()
                .and_then(|id| book.and_then(|b| b.items().into_iter().find(|i| i.id == id)));
            let allowed = item.as_ref().is_some_and(|i| {
                i.source.job.as_deref() == Some(r.job.as_str())
                    && i.detail.split(',').any(|n| n == name)
                    && i.state == State::Answered
                    && i.closed_ms.is_some_and(|c| c as u64 <= r.at_ms)
                    && match i.answer {
                        Some(ActionId::AllowOnce) => {
                            let id = approval.as_deref().unwrap_or_default();
                            *used.entry(id).or_insert(rid) == rid
                        }
                        Some(ActionId::AllowAlways) => true,
                        _ => false,
                    }
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
        let up = world.up_since(node).unwrap_or(0);
        for ((job, _), grant) in store {
            let key = (node.clone(), job.clone());
            if s.started_on.contains(&key) && !s.local_ended.contains_key(&key) {
                continue;
            }
            let base = s.local_ended.get(&key).copied().unwrap_or(grant.stored_at);
            let (since, limit) = if up > base {
                (up, RECOVERY_MS)
            } else {
                (base, STEADY_MS)
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
