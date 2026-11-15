use super::Driver;
use crate::cluster_node::{Events, In};
use notchling_cluster_net::Conn;
use notchling_engine::cluster::persist;
use notchling_engine::cluster::{Input, NodeId};
use notchling_events::kinds::security::ClusterStep;
use notchling_events::spec::Outcome;
use std::sync::Arc;

impl Driver {
    pub(super) fn opened(&mut self, conn: Arc<dyn Conn>) {
        let Some(peer) = Driver::peer_of(&conn.peer_fingerprint()) else {
            conn.close();
            return;
        };
        if self.impostor(&conn.peer_fingerprint()) {
            conn.close();
            return;
        }
        if !self.link {
            conn.close();
            return;
        }
        if let Some(old) = self.conns.insert(peer.clone(), conn.clone()) {
            if old.id() != conn.id() {
                old.close();
            }
        }
        self.step(Input::Linked { peer });
    }

    pub(super) fn closed(&mut self, fp: &str, id: u64) {
        let Some(peer) = Driver::peer_of(fp) else {
            return;
        };
        let current = self.conns.get(&peer).is_some_and(|c| c.id() == id);
        if current {
            self.conns.remove(&peer);
            self.step(Input::Unlinked { peer });
        }
    }

    pub(super) fn set_link(&mut self, on: bool) {
        if on == self.link {
            return;
        }
        self.link = on;
        if on {
            self.listen();
            return;
        }
        self.listener = None;
        self.bound.clear();
        let peers: Vec<NodeId> = self.conns.keys().cloned().collect();
        for (_, conn) in std::mem::take(&mut self.conns) {
            conn.close();
        }
        for peer in peers {
            self.step(Input::Unlinked { peer });
        }
    }

    pub(super) fn listen(&mut self) {
        self.listener = None;
        if !self.net_allowed() {
            return;
        }
        let events = Arc::new(Events {
            tx: self.tx.clone(),
        });
        match self.net.transport.listen(self.setup.port, events) {
            Ok(listener) => {
                self.port = listener.ports().first().copied().or(self.port);
                self.bound = self.net.transport.addrs();
                self.listener = Some(listener);
                if self
                    .error
                    .as_deref()
                    .is_some_and(|e| e.starts_with("listening"))
                {
                    self.error = None;
                }
            }
            Err(e) => {
                self.error = Some(format!("listening on port {} failed: {e}", self.setup.port));
                self.audit
                    .step(ClusterStep::Listen, None, None, Outcome::Failed);
            }
        }
    }

    pub(super) fn dial(&mut self, to: NodeId, addrs: Vec<String>, port: u16, fingerprint: String) {
        if !self.link || !self.net_allowed() || self.conns.contains_key(&to) {
            return;
        }
        if !self.dialing.insert(to.clone()) {
            return;
        }
        let transport = self.net.transport.clone();
        let tx = self.tx.clone();
        let events = Arc::new(Events { tx: tx.clone() });
        let spawned = std::thread::Builder::new()
            .name("cluster-dial".into())
            .spawn(move || {
                let _ = transport.dial(&addrs, port, &fingerprint, events);
                let _ = tx.send(In::DialDone(to));
            });
        if spawned.is_err() {
            self.dialing.clear();
        }
    }

    pub(super) fn wipe(&mut self) {
        for (_, conn) in std::mem::take(&mut self.conns) {
            conn.close();
        }
        let inbox_safe = self.hooks.wiped();
        let kept = inbox_safe || {
            let aside = persist::keep_aside(&self.setup.cluster_dir, crate::acting_ops::now_ms());
            let outcome = if aside.is_ok() {
                Outcome::Ok
            } else {
                Outcome::Failed
            };
            self.audit.step(ClusterStep::Save, None, None, outcome);
            aside.is_ok()
        };
        let first = if kept {
            persist::wipe(&self.setup.cluster_dir)
        } else {
            Err("the inbox could not be kept".to_string())
        };
        let outcome = match first
            .and_then(|()| crate::cluster_mirror::wipe(&self.setup.cluster_dir))
            .and_then(|()| crate::cluster_blobs::wipe(&self.setup.cluster_dir))
        {
            Ok(()) => Outcome::Ok,
            Err(_) => Outcome::Failed,
        };
        self.audit.step(ClusterStep::Wiped, None, None, outcome);
    }
}
