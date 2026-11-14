use notchling_engine::tools::agy::elsewhere;
use notchling_engine::transcript::model::Conversation;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

const KEEP: usize = 64;

#[derive(Clone, Debug)]
pub struct PhoneRead {
    pub conversation: Conversation,
    pub working: bool,
    pub omitted: usize,
}

impl PhoneRead {
    pub fn bounded(conversation: Conversation, working: bool, max_bytes: usize) -> PhoneRead {
        let (conversation, omitted) = conversation.bounded(max_bytes);
        PhoneRead {
            conversation,
            working,
            omitted,
        }
    }
}

type Stamp = (u64, SystemTime);

type Read = Arc<(Conversation, usize)>;

pub(crate) struct LogReads {
    root: PathBuf,
    kept: Mutex<HashMap<String, (Stamp, Read)>>,
    reads: AtomicUsize,
}

impl LogReads {
    pub fn new(root: PathBuf) -> LogReads {
        LogReads {
            root,
            kept: Mutex::new(HashMap::new()),
            reads: AtomicUsize::new(0),
        }
    }

    pub fn get(&self, session: &str, max_bytes: usize) -> Read {
        let Some(stamp) = elsewhere::log_stamp(&self.root, session) else {
            return Arc::default();
        };
        if let Some((at, kept)) = lock(&self.kept).get(session) {
            if *at == stamp {
                return kept.clone();
            }
        }
        self.reads.fetch_add(1, Ordering::SeqCst);
        let path = elsewhere::log_path(&self.root, session);
        let (conversation, _) = elsewhere::from_log_file(&path, None);
        let made = Arc::new(conversation.bounded(max_bytes));
        let mut kept = lock(&self.kept);
        if kept.len() >= KEEP {
            kept.clear();
        }
        kept.insert(session.to_owned(), (stamp, made.clone()));
        made
    }

    #[cfg(test)]
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
