use crate::transport::Stream;
use crate::wire::{ClientCounts, ClientKind, Tier};
use std::collections::HashMap;
use std::net::Shutdown;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Mutex;

struct Entry {
    kind: Option<ClientKind>,
    tier: Tier,
    topics: Vec<String>,
    tx: SyncSender<Vec<u8>>,
    stream: Stream,
}

#[derive(Default)]
pub(crate) struct Hub {
    entries: Mutex<HashMap<u64, Entry>>,
    next: AtomicU64,
}

impl Hub {
    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn add(&self, tier: Tier, tx: SyncSender<Vec<u8>>, stream: Stream) -> u64 {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        self.entries().insert(
            id,
            Entry {
                kind: None,
                tier,
                topics: Vec::new(),
                tx,
                stream,
            },
        );
        id
    }

    pub(crate) fn hello(&self, id: u64, kind: ClientKind, tier: Tier) {
        if let Some(entry) = self.entries().get_mut(&id) {
            entry.kind = Some(kind);
            entry.tier = tier;
        }
    }

    pub(crate) fn subscribe(&self, id: u64, topics: &[String]) {
        if let Some(entry) = self.entries().get_mut(&id) {
            for topic in topics {
                if !entry.topics.contains(topic) {
                    entry.topics.push(topic.clone());
                }
            }
        }
    }

    pub(crate) fn forget(&self, id: u64) {
        self.entries().remove(&id);
    }

    pub(crate) fn len(&self) -> usize {
        self.entries().len()
    }

    pub(crate) fn app_attached(&self) -> bool {
        self.entries()
            .values()
            .any(|e| e.kind.is_some() && e.tier == Tier::App)
    }

    pub(crate) fn app_client_attached(&self) -> bool {
        self.entries()
            .values()
            .any(|e| e.kind == Some(ClientKind::App) && e.tier == Tier::App)
    }

    pub(crate) fn app_client_following(&self, topic: &str) -> bool {
        self.entries().values().any(|e| {
            e.kind == Some(ClientKind::App)
                && e.tier == Tier::App
                && e.topics.iter().any(|t| t == topic)
        })
    }

    pub(crate) fn counts(&self) -> ClientCounts {
        let mut counts = ClientCounts::default();
        for entry in self.entries().values() {
            match entry.kind {
                Some(ClientKind::App) => counts.app += 1,
                Some(ClientKind::Cli) => counts.cli += 1,
                Some(ClientKind::Hook) => counts.hook += 1,
                Some(ClientKind::Node) | None => {}
            }
        }
        counts
    }

    pub(crate) fn publish(&self, topic: &str, line: &[u8]) -> usize {
        let mut entries = self.entries();
        let mut sent = 0;
        let mut dropped = Vec::new();
        for (id, entry) in entries.iter() {
            if !entry.topics.iter().any(|t| t == topic) {
                continue;
            }
            match entry.tx.try_send(line.to_vec()) {
                Ok(()) => sent += 1,
                Err(_) => dropped.push(*id),
            }
        }
        for id in dropped {
            if let Some(entry) = entries.remove(&id) {
                let _ = entry.stream.shutdown(Shutdown::Both);
            }
        }
        sent
    }

    pub(crate) fn close_all(&self) {
        for (_, entry) in self.entries().drain() {
            let _ = entry.stream.shutdown(Shutdown::Both);
        }
    }
}
