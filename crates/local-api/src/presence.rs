use notchling_events::kinds::security::{AppAttached, AppDetached, DetachCause};
use notchling_events::{Count, Exe, Millis, Version};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const DROP_AFTER: Duration = Duration::from_secs(10);

struct Entry {
    connections: u32,
    attached_at: Instant,
    last_close: Option<Instant>,
}

#[derive(Default)]
pub struct Presence {
    apps: Mutex<HashMap<u32, Entry>>,
}

impl Presence {
    pub fn new() -> Presence {
        Presence::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u32, Entry>> {
        self.apps.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn on_hello(
        &self,
        pid: u32,
        version: &str,
        exe: Option<&Path>,
        now: Instant,
    ) -> Option<AppAttached> {
        let mut apps = self.lock();
        if let Some(entry) = apps.get_mut(&pid) {
            entry.connections += 1;
            entry.last_close = None;
            return None;
        }
        apps.insert(
            pid,
            Entry {
                connections: 1,
                attached_at: now,
                last_close: None,
            },
        );
        Some(AppAttached {
            version: Version::new(version),
            exe: exe.map_or_else(|| Exe::named("unknown"), Exe::from_path),
            pid: Count(u64::from(pid)),
        })
    }

    pub fn on_close(&self, pid: u32, now: Instant) {
        let mut apps = self.lock();
        if let Some(entry) = apps.get_mut(&pid) {
            entry.connections = entry.connections.saturating_sub(1);
            if entry.connections == 0 {
                entry.last_close = Some(now);
            }
        }
    }

    pub fn due(&self, now: Instant) -> Vec<AppDetached> {
        let mut apps = self.lock();
        let gone: Vec<u32> = apps
            .iter()
            .filter(|(_, e)| {
                e.connections == 0
                    && e.last_close
                        .is_some_and(|at| now.saturating_duration_since(at) >= DROP_AFTER)
            })
            .map(|(pid, _)| *pid)
            .collect();
        gone.into_iter()
            .filter_map(|pid| apps.remove(&pid))
            .map(|entry| {
                let at = entry.last_close.unwrap_or(now);
                detached(&entry, at, DetachCause::Dropped)
            })
            .collect()
    }
}

fn detached(entry: &Entry, at: Instant, cause: DetachCause) -> AppDetached {
    let ms = at.saturating_duration_since(entry.attached_at).as_millis();
    AppDetached {
        cause,
        attached_ms: Millis(u64::try_from(ms).unwrap_or(u64::MAX)),
    }
}
