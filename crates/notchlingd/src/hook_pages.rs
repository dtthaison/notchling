use crate::layout::Layout;
use notchling_engine::lanes::VARIABLE;
use notchling_engine::pages::account::active_email;
use notchling_engine::pages::{hook, PageLibrary, RecordContext};
use serde_json::Value;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEADLINE: Duration = Duration::from_secs(8);

#[derive(Clone, Debug)]
pub struct PagesEnv {
    pub pages_dir: PathBuf,
    pub home: PathBuf,
    pub data_dir: PathBuf,
    pub config_dir: Option<String>,
    pub deadline: Duration,
}

impl PagesEnv {
    pub fn for_process(layout: &Layout) -> PagesEnv {
        PagesEnv {
            pages_dir: layout.data_dir.join("pages"),
            home: layout.home(),
            data_dir: layout.data_dir.clone(),
            config_dir: std::env::var(VARIABLE).ok(),
            deadline: DEADLINE,
        }
    }
}

pub fn answer(event: Value, env: PagesEnv) -> Option<Value> {
    let deadline = env.deadline;
    with_deadline(deadline, move || answer_now(&event, &env)).flatten()
}

pub fn answer_now(event: &Value, env: &PagesEnv) -> Option<Value> {
    let lib = PageLibrary::writable(&env.pages_dir);
    let account = if hook::may_publish(event) {
        active_email(&env.home, &env.data_dir, env.config_dir.as_deref())
    } else {
        String::new()
    };
    let ctx = RecordContext {
        account: &account,
        now: now_secs(),
        home: &env.home,
    };
    hook::answer(&lib, event, &ctx)
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn with_deadline<T: Send + 'static>(
    limit: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("hook-pages".into())
        .spawn(move || {
            let _ = tx.send(catch_unwind(AssertUnwindSafe(work)));
        })
        .ok()?;
    rx.recv_timeout(limit).ok()?.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_that_panics_or_hangs_is_a_pass() {
        assert_eq!(with_deadline(Duration::from_secs(5), || 7), Some(7));
        assert_eq!(
            with_deadline(Duration::from_secs(5), || -> u8 { panic!("boom") }),
            None
        );
        let started = std::time::Instant::now();
        let slow = with_deadline(Duration::from_millis(100), || {
            std::thread::sleep(Duration::from_secs(3));
            1
        });
        assert_eq!(slow, None);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
