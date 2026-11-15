use super::invariants::Violation;
use super::secrets::OUTLIVE_MS;
use super::world::World;

pub(super) fn check_secrets(world: &World) -> Result<(), Violation> {
    let s = world.secrets();
    for (node, store) in &s.stores {
        for (job, _) in store.keys() {
            if s.job_nodes.get(job) != Some(node) {
                return Err(Violation::GrantOnWrongNode {
                    job: job.clone(),
                    node: node.clone(),
                });
            }
        }
    }
    if let Some((node, path)) = &s.leak {
        return Err(Violation::SecretLeaked {
            node: node.clone(),
            path: path.clone(),
        });
    }
    if let Some(node) = s.refreshes.first() {
        return Err(Violation::RefreshOnWorker { node: node.clone() });
    }
    for (item, times) in &s.answered {
        if *times > 1 {
            return Err(Violation::AnswerTwice { item: item.clone() });
        }
    }
    for r in &s.releases {
        let at_vault = s.vault_node.as_ref() == Some(&r.node);
        if !(at_vault && r.vault == r.node && r.home == r.node && r.term.node == r.node) {
            return Err(Violation::ReleasedOffPrimary {
                node: r.node.clone(),
                term: r.term.clone(),
            });
        }
    }
    let now = world.now();
    for (node, store) in &s.stores {
        if !world.is_up(node) {
            continue;
        }
        for (job, _) in store.keys() {
            let since = match (
                s.local_ended.get(&(node.clone(), job.clone())),
                s.ended.get(job),
                s.reach.get(node).copied().flatten(),
            ) {
                (Some(t), ..) => s.alive_since.get(node).map(|a| (*t).max(*a)),
                (None, Some(end), Some(reach)) => Some((*end).max(reach)),
                _ => None,
            };
            if since.is_some_and(|t| now.saturating_sub(t) > OUTLIVE_MS) {
                return Err(Violation::GrantOutlivedJob {
                    job: job.clone(),
                    node: node.clone(),
                });
            }
        }
    }
    Ok(())
}
