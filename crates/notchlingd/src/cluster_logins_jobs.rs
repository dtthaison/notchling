use crate::grant_roots::valid_key;
use notchling_engine::cluster::blob::sha256_of;
use notchling_engine::cluster::grant_files::GrantValue;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

pub const JOBS_FILE: &str = "jobs.json";
pub const MAX_JOBS: usize = 10_000;
pub const MAX_OVERFLOW: usize = 32;
const EVERYTHING: &str = "*";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn hash_of(token: &GrantValue) -> String {
    sha256_of(token.expose().as_bytes())
}

pub fn lost_name(session: &str) -> String {
    format!("login-{session}")
}

#[derive(Default)]
struct Book {
    jobs: BTreeMap<String, String>,
    overflow: BTreeSet<String>,
}

pub struct LoginJobs {
    path: PathBuf,
    book: Mutex<Book>,
    readable: bool,
    cap: usize,
}

fn parse(text: &str) -> Option<Book> {
    let value: Value = serde_json::from_str(text).ok()?;
    let jobs = value
        .get("jobs")?
        .as_object()?
        .iter()
        .map(|(session, hash)| Some((session.clone(), hash.as_str()?.to_string())))
        .collect::<Option<BTreeMap<_, _>>>()?;
    let overflow = match value.get("overflow") {
        None => BTreeSet::new(),
        Some(list) => list
            .as_array()?
            .iter()
            .map(|h| Some(h.as_str()?.to_string()))
            .collect::<Option<BTreeSet<_>>>()?,
    };
    Some(Book { jobs, overflow })
}

fn fold(book: &mut Book, keep: &str, cap: usize) {
    let mut order: Vec<String> = book.jobs.keys().cloned().collect();
    order.sort_by_key(|session| book.jobs.get(session).is_none_or(|h| h != keep));
    for session in order.into_iter().take(book.jobs.len() - cap / 2) {
        if let Some(hash) = book.jobs.remove(&session) {
            book.overflow.insert(hash);
        }
    }
    if book.overflow.len() > MAX_OVERFLOW {
        book.overflow = BTreeSet::from([EVERYTHING.to_string()]);
    }
}

impl LoginJobs {
    pub fn load(logins_dir: &Path) -> LoginJobs {
        LoginJobs::load_capped(logins_dir, MAX_JOBS)
    }

    pub fn load_capped(logins_dir: &Path, cap: usize) -> LoginJobs {
        let path = logins_dir.join(JOBS_FILE);
        let (book, readable) = match std::fs::read_to_string(&path) {
            Ok(text) => match parse(&text) {
                Some(book) => (book, true),
                None => (Book::default(), false),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Book::default(), true),
            Err(_) => (Book::default(), false),
        };
        LoginJobs {
            path,
            book: Mutex::new(book),
            readable,
            cap,
        }
    }

    pub fn usable(&self) -> bool {
        self.readable
    }

    pub fn all(&self) -> Vec<(String, String)> {
        lock(&self.book)
            .jobs
            .iter()
            .map(|(session, hash)| (session.clone(), hash.clone()))
            .collect()
    }

    pub fn overflow(&self) -> Vec<String> {
        lock(&self.book).overflow.iter().cloned().collect()
    }

    pub fn record(&self, session: &str, hash: &str) -> Result<(), String> {
        if !valid_key(session) {
            return Err("a session key the record can't hold".to_string());
        }
        let mut book = lock(&self.book);
        book.jobs.insert(session.to_string(), hash.to_string());
        if book.jobs.len() > self.cap {
            fold(&mut book, hash, self.cap);
        }
        let jobs: serde_json::Map<String, Value> = book
            .jobs
            .iter()
            .map(|(session, hash)| (session.clone(), json!(hash)))
            .collect();
        let bytes = serde_json::to_vec(&json!({"v": 1, "jobs": jobs, "overflow": book.overflow}))
            .map_err(|_| "the record can't be encoded".to_string())?;
        if let Some(dir) = self.path.parent() {
            notchling_platform::files::ensure_private_dir(dir).map_err(|e| e.to_string())?;
        }
        notchling_platform::files::write_private_no_follow(&self.path, &bytes)
            .map_err(|e| e.to_string())
    }
}
