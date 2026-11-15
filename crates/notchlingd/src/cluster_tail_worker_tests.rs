use super::*;
use crate::grant_janitor::GrantJanitor;
use crate::grant_roots::PENDING_MS;
use crate::grant_use::tests::{deliver_args, from_vault, real_now, rig_at, Rig as Grants};
use crate::grant_use::UseCalls;
use notchling_local_api::ops;

const CLAUDE_ROW: &str = "0b9e6c0e-1111-2222-3333-444455556666";
const VALUE: &str = "NL-MARKER-LONG-VALUE";
const HALF: &str = "NL-MARKER-LONG";
const ENDING: &str = "-VALUE more\n";

struct Starts(Value);

impl LocalCalls for Starts {
    fn call(&self, _op: &str, _args: &Value, _origin: &Origin) -> Result<Value, ApiError> {
        Ok(self.0.clone())
    }
}

fn from_home() -> Origin {
    Origin {
        node: id("vault"),
        tier: OriginTier::App,
    }
}

struct Worker {
    grants: Grants,
    _dir: Dir,
    file: PathBuf,
    row: String,
    now: Arc<AtomicI64>,
    tails: Arc<MaskedTails>,
    start: UseCalls,
    janitor: Arc<GrantJanitor>,
}

fn worker(row: &str, short: Option<&str>, started: Value) -> Worker {
    let now = Arc::new(AtomicI64::new(real_now()));
    let clock = clock_of(&now);
    let grants = rig_at(clock.clone(), "worker");
    let state = grants.fake.state.lock().unwrap().clone().expect("a state");
    assert!(state.jobs.is_empty(), "a worker's state comes from a sync");
    let dir = Dir::new("tail-worker");
    let file = dir.path().join("transcript");
    std::fs::write(&file, b"").unwrap();
    let tails = Arc::new(MaskedTails::new(
        Arc::new(FakeCalls::default()),
        Arc::new(Rows {
            path: file.clone(),
            id: row.to_string(),
            short: short.map(str::to_string),
        }),
        (grants.masks.clone(), grants.fake.clone()),
        Arc::new(|_: &str| true),
        (grants.roots.clone(), clock.clone()),
    ));
    let janitor = GrantJanitor::new(
        grants.store.clone(),
        (
            grants.masks.clone(),
            grants.marks.clone(),
            grants.roots.clone(),
        ),
        grants.fake.clone(),
        clock,
    );
    let start = UseCalls::new(Arc::new(Starts(started)), grants.uses.clone());
    Worker {
        grants,
        _dir: dir,
        file,
        row: row.to_string(),
        now,
        tails,
        start,
        janitor,
    }
}

fn claude() -> Worker {
    worker(CLAUDE_ROW, Some("c1"), json!({"session": "c1"}))
}

fn agy() -> Worker {
    worker("agy:one", None, json!({"id": "agy:one"}))
}

fn untied() -> Worker {
    worker("some-other-session", None, json!({"session": "c1"}))
}

impl Worker {
    fn deliver(&self) {
        let args = deliver_args(JOB, "api-key", "V", VALUE);
        self.grants
            .uses
            .deliver(&from_vault(), &args)
            .expect("accepted before the job starts here");
    }

    fn begin(&self, op: &str) {
        let args = json!({"folder": "/work", "job": JOB});
        self.start
            .call(op, &args, &from_home())
            .expect("the start op runs");
    }

    fn key(&self) {
        self.janitor.tick().expect("swept");
    }

    fn ends_here(&self) {
        let failed = Err(ApiError::new(ErrorCode::Failed, "gone"));
        self.grants.uses.started(JOB, &failed);
    }

    fn append(&self, text: &str) {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&self.file)
            .unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    fn tail(&self, from: u64) -> Result<Value, ApiError> {
        let args = json!({"id": self.row, "from": from});
        self.tails.call(NODE_TAIL, &args, &origin())
    }

    fn held_first(&self) -> Value {
        self.append(&format!("said {HALF}"));
        let first = self.tail(0).expect("served");
        assert!(!served(&first).contains("NL-MARKER"));
        first
    }
}

fn is_held(out: &Value) -> bool {
    out["next"].as_u64().unwrap() < out["size"].as_u64().unwrap()
}

#[test]
fn a_held_prefix_in_a_worker_claude_row_is_not_flushed_while_the_job_is_live_here() {
    let w = claude();
    w.deliver();
    w.begin(ops::SESSION_NEW);
    w.key();
    let first = w.held_first();
    assert!(is_held(&first), "the job may still write the rest");
    assert_eq!(served(&first), "said ");
    w.append(ENDING);
    let second = w.tail(first["next"].as_u64().unwrap()).expect("served");
    let both = served(&first) + &served(&second);
    assert_eq!(both, "said [secret api-key] more\n");
    assert!(!both.contains("-VALUE"), "no part of the value is served");
}

#[test]
fn the_held_prefix_is_masked_and_flushed_once_the_job_ends_here() {
    let w = claude();
    w.deliver();
    w.begin(ops::SESSION_NEW);
    w.key();
    assert!(is_held(&w.held_first()));
    w.ends_here();
    let end = w.tail(0).expect("served");
    assert_eq!(served(&end), "said [secret api-key]");
    assert_eq!(end["next"], end["size"]);
}

#[test]
fn an_antigravity_row_of_a_worker_is_held_the_same_way() {
    let w = agy();
    w.deliver();
    w.begin(ops::AGY_SEND);
    w.key();
    assert!(is_held(&w.held_first()), "live here, so not over");
    w.ends_here();
    let end = w.tail(0).expect("served");
    assert_eq!(served(&end), "said [secret api-key]");
    assert_eq!(end["next"], end["size"]);
}

#[test]
fn a_job_that_has_not_started_yet_keeps_an_untied_rows_prefix_held_until_its_window_ends() {
    let w = untied();
    w.deliver();
    assert!(is_held(&w.held_first()), "pending is not over");
    w.now.fetch_add(PENDING_MS - 1, Ordering::SeqCst);
    assert!(is_held(&w.tail(0).unwrap()), "still inside the window");
    w.now.fetch_add(1, Ordering::SeqCst);
    let end = w.tail(0).expect("served");
    assert_eq!(served(&end), "said [secret api-key]");
    assert_eq!(end["next"], end["size"]);
}

#[test]
fn a_pending_job_is_not_over_for_its_own_row_either() {
    let shell = format!("shell:{JOB}");
    let w = worker(&shell, None, json!({}));
    w.deliver();
    assert!(is_held(&w.held_first()), "delivered, not started: not over");
    w.now.fetch_add(PENDING_MS, Ordering::SeqCst);
    let end = w.tail(0).expect("served");
    assert_eq!(served(&end), "said [secret api-key]");
    assert_eq!(end["next"], end["size"]);
}

#[test]
fn a_job_live_here_keeps_an_untied_rows_prefix_held_and_its_end_releases_it() {
    let w = untied();
    w.deliver();
    w.begin(ops::SESSION_NEW);
    assert!(is_held(&w.held_first()));
    w.ends_here();
    let end = w.tail(0).expect("served");
    assert_eq!(served(&end), "said [secret api-key]");
    assert_eq!(end["next"], end["size"]);
}

#[test]
fn a_shell_jobs_root_alive_here_keeps_its_log_prefix_held_though_the_log_is_closed() {
    let shell = format!("shell:{JOB}");
    let w = worker(&shell, None, json!({}));
    w.deliver();
    w.grants.roots.set(JOB, 500);
    assert!(is_held(&w.held_first()));
    w.grants.roots.end(JOB);
    let end = w.tail(0).expect("served");
    assert_eq!(served(&end), "said [secret api-key]");
    assert_eq!(end["next"], end["size"]);
}

#[test]
fn the_state_decides_for_a_job_it_lists_whatever_the_local_record_says() {
    let w = claude();
    w.deliver();
    w.begin(ops::SESSION_NEW);
    w.key();
    assert!(is_held(&w.held_first()));
    let done = cluster("vault", &[job(JOB, "worker", JobState::Done)]);
    *w.grants.fake.state.lock().unwrap() = Some(done);
    let end = w.tail(0).expect("served");
    assert_eq!(end["next"], end["size"], "the state says it is over");
    assert_eq!(served(&end), "said [secret api-key]");
    let running = cluster("vault", &[job(JOB, "worker", JobState::Running)]);
    *w.grants.fake.state.lock().unwrap() = Some(running);
    w.ends_here();
    assert!(is_held(&w.tail(0).unwrap()), "the state says it runs");
}

#[test]
fn rows_tied_to_no_job_follow_the_state_for_a_job_it_lists() {
    let w = untied();
    w.deliver();
    w.begin(ops::SESSION_NEW);
    assert!(is_held(&w.held_first()), "live here, so not over");
    let done = cluster("vault", &[job(JOB, "worker", JobState::Done)]);
    *w.grants.fake.state.lock().unwrap() = Some(done);
    let end = w.tail(0).expect("served");
    assert_eq!(end["next"], end["size"], "the state lists it as finished");
    let running = cluster("vault", &[job(JOB, "worker", JobState::Running)]);
    *w.grants.fake.state.lock().unwrap() = Some(running);
    assert!(is_held(&w.tail(0).unwrap()), "the state says it runs");
}

#[test]
fn a_lost_login_refuses_a_worker_claude_row_but_not_an_antigravity_or_shell_row() {
    let w = claude();
    w.deliver();
    w.begin(ops::SESSION_NEW);
    w.key();
    w.append("anything\n");
    assert!(w.tail(0).is_ok(), "nothing is lost yet");
    w.grants.masks.mark_granted_as("login-record", &[]);
    let err = w.tail(0).expect_err("the login's masks are gone");
    assert_eq!(err.message, UNMAPPED_LOST);

    let a = agy();
    a.deliver();
    a.begin(ops::AGY_SEND);
    a.key();
    a.append("anything\n");
    a.grants.masks.mark_granted_as("login-record", &[]);
    assert!(
        a.tail(0).is_ok(),
        "no Claude login reached an Antigravity job"
    );

    let shell = format!("shell:{JOB}");
    let s = worker(&shell, None, json!({}));
    s.deliver();
    s.grants.roots.set(JOB, 500);
    s.append("anything\n");
    s.grants.masks.mark_granted_as("login-record", &[]);
    assert!(s.tail(0).is_ok(), "nor to a shell job");
}

#[test]
fn a_lost_login_refuses_the_claude_row_of_a_job_that_began_a_claude_start_only() {
    let w = claude();
    w.deliver();
    w.begin(ops::AGY_SEND);
    w.key();
    w.append("anything\n");
    w.grants.masks.mark_granted_as("login-record", &[]);
    assert!(
        w.tail(0).is_ok(),
        "an agy start doesn't make the job a Claude job"
    );
    w.begin(ops::SESSION_NEW);
    assert_eq!(
        w.tail(0).expect_err("now it ran Claude").message,
        UNMAPPED_LOST
    );
}
