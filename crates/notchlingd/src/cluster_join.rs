use crate::cluster_node::{JoinAsk, Submit};
use crate::inbox_ops::{InboxOps, Responder};
use notchling_engine::cluster::LocalCmd;
use notchling_engine::inbox::{Action, ActionId, Item, Kind, Source, State};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const JOIN_WAIT: Duration = Duration::from_secs(600);
const NAME_MAX: usize = 64;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(NAME_MAX)
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn join_item(id: &str, ask: &JoinAsk, created_ms: i64, wait: Duration) -> Item {
    let name = match plain(&ask.name) {
        name if name.is_empty() => ask.node.to_string(),
        name => name,
    };
    let mut detail = vec![format!("{} {}", plain(&ask.os), plain(&ask.arch))];
    if let Some(addr) = ask.addr.as_deref().map(plain).filter(|a| !a.is_empty()) {
        detail.push(addr);
    }
    detail.push(ask.node.to_string());
    if ask.can_be_master {
        detail.push("Can Be Master".to_string());
    }
    let lasts = i64::try_from(wait.as_millis()).unwrap_or(i64::MAX);
    Item {
        id: id.to_string(),
        kind: Kind::NodeJoin,
        title: format!("Add {name} to the cluster"),
        detail: detail.join(" · "),
        source: Source {
            node: Some(ask.node.to_string()),
            session: None,
            job: None,
        },
        created_ms,
        expires_ms: Some(created_ms.saturating_add(lasts)),
        actions: vec![
            Action {
                id: ActionId::Allow,
                label: "Add".to_string(),
                primary: true,
            },
            Action {
                id: ActionId::Deny,
                label: "Deny".to_string(),
                primary: false,
            },
        ],
        state: State::Open,
        answer: None,
        answered_by: None,
        seen: false,
        open: None,
        closed_ms: None,
    }
}

struct JoinResponder {
    ask: JoinAsk,
    node: Submit,
    done: Arc<AtomicBool>,
}

impl Responder for JoinResponder {
    fn answered(&self, _item: &Item, action: ActionId) -> bool {
        let cmd = match action {
            ActionId::Allow => LocalCmd::Approve {
                node: self.ask.node.clone(),
                client: false,
                can_be_master: self.ask.can_be_master,
            },
            ActionId::Deny => LocalCmd::Deny {
                node: self.ask.node.clone(),
            },
            _ => return false,
        };
        if self.done.swap(true, Ordering::SeqCst) {
            return false;
        }
        self.node.send(cmd);
        true
    }
}

pub struct JoinAsker {
    inbox: Arc<InboxOps>,
    wait: Duration,
}

impl JoinAsker {
    pub fn new(inbox: Arc<InboxOps>, wait: Duration) -> JoinAsker {
        JoinAsker { inbox, wait }
    }

    pub fn ask(&self, ask: JoinAsk, node: Submit) {
        let created = now_ms();
        let id = format!("{}:{created}", ask.item);
        let done = Arc::new(AtomicBool::new(false));
        let item = join_item(&id, &ask, created, self.wait);
        let responder = Arc::new(JoinResponder {
            ask: ask.clone(),
            node: node.clone(),
            done: done.clone(),
        });
        self.inbox.raise(item, Some(responder));
        let (inbox, wait) = (self.inbox.clone(), self.wait);
        let _ = std::thread::Builder::new()
            .name("cluster-join-wait".into())
            .spawn(move || {
                std::thread::sleep(wait);
                if done.swap(true, Ordering::SeqCst) {
                    return;
                }
                inbox.resolve(&id, State::Expired, None, None);
                node.send(LocalCmd::Deny { node: ask.node });
            });
    }
}

#[cfg(test)]
#[path = "cluster_join_tests.rs"]
mod tests;
