use crate::acting::Acting;
use crate::cluster_move::{raise_notice, Mover};
use crate::cluster_returns::{Returns, ReturnsParts};
use crate::cluster_returns_aside::GitRepo;
use crate::cluster_returns_waiting::Waiting;
use crate::layout::Layout;
use notchling_engine::control::Launcher;
use notchling_engine::inbox::Kind;
use std::sync::Arc;

pub fn waiting_for(layout: &Layout, mover: &Arc<Mover>) -> Arc<Waiting> {
    let waiting = Waiting::new(&layout.cluster_dir());
    let (inbox, clock) = (mover.p.inbox.clone(), mover.p.clock.clone());
    waiting.on_damage(Arc::new(move |why| {
        let title = "The list of waiting sessions is damaged";
        let detail = format!("{why}. It is read as empty.");
        let id = "cluster-waiting-damaged";
        raise_notice(&inbox, clock(), id, (Kind::ActionFailed, title, &detail));
    }));
    waiting
}

pub fn returns_for(
    layout: &Layout,
    (mover, acting): (&Arc<Mover>, &Arc<Acting>),
    (git, waiting): (Arc<dyn Launcher>, &Arc<Waiting>),
) -> Arc<Returns> {
    Returns::new(ReturnsParts {
        mover: mover.clone(),
        repo: Arc::new(GitRepo(git)),
        acting: acting.clone(),
        data: layout.data_dir.clone(),
        waiting: waiting.clone(),
    })
}
