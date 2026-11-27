use super::*;
use crate::sessions::testutil::TempDir;
use std::fs::OpenOptions;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DAY: f64 = 86_400.0;
const LIMIT: Duration = Duration::from_secs(5);
const GRACE: Duration = Duration::from_millis(250);

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn write(dir: &TempDir, name: &str, body: &str) {
    let line = format!(
        r#"{{"type":"user","cwd":"/work/app","message":{{"role":"user","content":"{body}"}}}}"#
    );
    let path = dir.write(
        &format!("claude/projects/-work-app/{name}.jsonl"),
        &format!("{line}\n"),
    );
    let file = OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs_f64(now() - DAY))
        .unwrap();
}

fn roots(dir: &TempDir) -> IndexRoots {
    IndexRoots {
        claude: vec![ClaudeRoot {
            projects: dir.path("claude/projects"),
            config_dir: None,
        }],
        codex_sessions: None,
        titled: Vec::new(),
    }
}

fn warmed(dir: &TempDir) -> PathBuf {
    let db = dir.path("search.db");
    write(dir, "one", "zebrafish talk");
    let index = SessionIndex::with_range(Some(db.clone()), None);
    index.refresh(&roots(dir), now());
    assert_eq!(index.len(), 1);
    db
}

fn wait_for(what: &str, ready: impl Fn() -> bool) {
    let deadline = Instant::now() + LIMIT;
    while !ready() {
        assert!(Instant::now() < deadline, "gave up waiting: {what}");
        thread::yield_now();
    }
}

fn sessions_on(index: &SessionIndex) -> Option<bool> {
    index.lock().sessions_on.is()
}

fn pages_on(index: &SessionIndex) -> Option<bool> {
    index.lock().pages_on.is()
}

fn sessions_off(index: &SessionIndex) -> bool {
    sessions_on(index) == Some(false)
}

fn pages_off(index: &SessionIndex) -> bool {
    pages_on(index) == Some(false)
}

fn cached(db: &Path) -> usize {
    let reopened = SessionIndex::with_range(Some(db.to_path_buf()), None);
    reopened.load_cache();
    reopened.len()
}

#[test]
fn a_refresh_after_sessions_are_turned_off_leaves_the_index_empty() {
    let dir = TempDir::new("fence-off");
    let db = warmed(&dir);
    let index = SessionIndex::with_range(Some(db.clone()), None);
    index.set_sessions_on(false).unwrap();
    let pass = index.refresh(&roots(&dir), now());
    assert!(pass.skipped);
    assert!(index.is_empty());
    assert_eq!(cached(&db), 0);
    index.set_sessions_on(true).unwrap();
    index.refresh(&roots(&dir), now());
    assert_eq!(index.len(), 1);
}

#[test]
fn turning_sessions_off_on_a_cold_index_empties_the_cache_on_disk() {
    let dir = TempDir::new("fence-cold");
    let db = warmed(&dir);
    let cold = SessionIndex::with_range(Some(db.clone()), None);
    assert!(cold.is_empty());
    cold.set_sessions_on(false).unwrap();
    assert_eq!(cached(&db), 0);
    cold.load_cache();
    assert!(cold.is_empty());
}

#[test]
fn a_refresh_that_began_before_sessions_were_turned_off_does_not_land() {
    let dir = TempDir::new("fence-inflight");
    let db = dir.path("search.db");
    write(&dir, "one", "zebrafish talk");
    write(&dir, "two", "quokka talk");
    let index = SessionIndex::with_range(Some(db.clone()), None);
    let pass = index.refresh_with(&roots(&dir), now(), &|| {
        index.set_sessions_on(false).unwrap();
    });
    assert_eq!(pass.read, 2);
    assert!(index.is_empty());
    assert_eq!(cached(&db), 0);
}

#[test]
fn load_cache_holds_the_fence_while_it_reads() {
    let dir = TempDir::new("fence-load");
    let db = warmed(&dir);
    let index = Arc::new(SessionIndex::with_range(Some(db), None));
    let stall = index.db.as_ref().unwrap().hold();
    let loader = {
        let index = index.clone();
        thread::spawn(move || index.load_cache())
    };
    wait_for("load_cache never took the fence", || {
        index.writes.try_lock().is_err()
    });
    drop(stall);
    loader.join().unwrap();
    assert_eq!(index.len(), 1);
}

fn page_library(dir: &TempDir) -> PageLibrary {
    dir.write(
        "pages/aaa/index.html",
        "<h1>Intro</h1><p>the quokka smiles</p>",
    );
    dir.write(
        "pages/pages.json",
        &serde_json::json!({"pages": [{
            "folder": "aaa", "file": "index.html", "title": "Cost Report",
            "url": "https://claude.ai/artifact/aaa", "source": "", "cwd": "",
            "project": "app", "account": "", "updated": 10.0, "version": "v1"
        }]})
        .to_string(),
    );
    PageLibrary::read_only(dir.path("pages"))
}

#[test]
fn turning_pages_off_waits_for_a_page_refresh_in_flight() {
    let dir = TempDir::new("fence-pages");
    let library = page_library(&dir);
    let index = Arc::new(SessionIndex::with_range(None, None));
    let (ready, reading) = channel();
    let (release, released) = channel();
    let refresher = {
        let index = index.clone();
        thread::spawn(move || {
            index.refresh_pages_with(&library, &|| {
                ready.send(()).unwrap();
                released.recv().unwrap();
            })
        })
    };
    reading.recv_timeout(LIMIT).unwrap();
    assert!(index.pages_writes.try_lock().is_err());
    let off = {
        let index = index.clone();
        thread::spawn(move || index.set_pages_on(false))
    };
    wait_for("set_pages_on never took effect", || pages_off(&index));
    release.send(()).unwrap();
    assert_eq!(refresher.join().unwrap().read, 1);
    off.join().unwrap().unwrap();
    assert!(index.search_pages(&["quokka"], 100.0, 30).is_empty());
}

fn still_waiting(answered: &Receiver<()>, what: &str) {
    assert_eq!(
        answered.recv_timeout(GRACE),
        Err(RecvTimeoutError::Timeout),
        "{what}"
    );
}

fn spawn_toggle(
    index: &Arc<SessionIndex>,
    on: bool,
    pages: bool,
) -> (thread::JoinHandle<Result<(), String>>, Receiver<()>) {
    let (back, answered) = channel();
    let index = index.clone();
    let handle = thread::spawn(move || {
        let reply = if pages {
            index.set_pages_on(on)
        } else {
            index.set_sessions_on(on)
        };
        let _ = back.send(());
        reply
    });
    (handle, answered)
}

fn draining(index: &SessionIndex) -> bool {
    index.is_empty() && index.writes.try_lock().is_err()
}

#[test]
fn turning_sessions_on_waits_for_a_drain_in_flight() {
    let dir = TempDir::new("fence-on-waits");
    let db = warmed(&dir);
    let index = Arc::new(SessionIndex::with_range(Some(db.clone()), None));
    index.load_cache();
    assert_eq!(index.len(), 1);
    let stall = index.db.as_ref().unwrap().hold();
    let (off, _) = spawn_toggle(&index, false, false);
    wait_for("the drain never began", || draining(&index));
    let (on, answered) = spawn_toggle(&index, true, false);
    wait_for("turning sessions on never began", || {
        sessions_on(&index) == Some(true)
    });
    still_waiting(&answered, "turning sessions on returned during a drain");
    drop(stall);
    off.join().unwrap().unwrap();
    on.join().unwrap().unwrap();
    assert_eq!(sessions_on(&index), Some(true));
    assert_eq!(cached(&db), 0);
    index.refresh(&roots(&dir), now());
    assert_eq!(index.len(), 1);
    assert_eq!(cached(&db), 1);
}

#[test]
fn turning_pages_on_waits_for_a_page_clear_in_flight() {
    let dir = TempDir::new("fence-on-waits-pages");
    let library = page_library(&dir);
    let index = Arc::new(SessionIndex::with_range(Some(dir.path("search.db")), None));
    assert_eq!(index.refresh_pages(&library).read, 1);
    let stall = index.db.as_ref().unwrap().hold();
    let (off, _) = spawn_toggle(&index, false, true);
    wait_for("the page clear never began", || {
        pages_off(&index) && index.pages_writes.try_lock().is_err()
    });
    let (on, answered) = spawn_toggle(&index, true, true);
    wait_for("turning pages on never began", || {
        pages_on(&index) == Some(true)
    });
    still_waiting(&answered, "turning pages on returned during a page clear");
    drop(stall);
    off.join().unwrap().unwrap();
    on.join().unwrap().unwrap();
    assert_eq!(pages_on(&index), Some(true));
    index.refresh_pages(&library);
    assert_eq!(index.search_pages(&["quokka"], 100.0, 30).len(), 1);
}

#[test]
fn the_last_toggle_wins_while_a_drain_is_in_flight() {
    let dir = TempDir::new("fence-last");
    let db = warmed(&dir);
    let index = Arc::new(SessionIndex::with_range(Some(db.clone()), None));
    index.load_cache();
    assert_eq!(index.len(), 1);
    let stall = index.db.as_ref().unwrap().hold();
    let (first, _) = spawn_toggle(&index, false, false);
    wait_for("the drain never began", || draining(&index));
    let (second, answered) = spawn_toggle(&index, true, false);
    wait_for("turning sessions on never began", || {
        sessions_on(&index) == Some(true)
    });
    still_waiting(&answered, "turning sessions on returned during a drain");
    let (third, _) = spawn_toggle(&index, false, false);
    wait_for("the last off never began", || sessions_off(&index));
    drop(stall);
    first.join().unwrap().unwrap();
    second.join().unwrap().unwrap();
    third.join().unwrap().unwrap();
    assert!(sessions_off(&index));
    assert!(index.is_empty());
    assert_eq!(cached(&db), 0);
}

fn refreshing_pages(
    index: &Arc<SessionIndex>,
    library: PageLibrary,
) -> (thread::JoinHandle<Refresh>, std::sync::mpsc::Sender<()>) {
    let (ready, reading) = channel();
    let (release, released) = channel();
    let index = index.clone();
    let refresher = thread::spawn(move || {
        index.refresh_pages_with(&library, &|| {
            ready.send(()).unwrap();
            released.recv().unwrap();
        })
    });
    reading.recv_timeout(LIMIT).unwrap();
    (refresher, release)
}

#[test]
fn the_last_page_toggle_wins_while_a_refresh_is_in_flight() {
    let dir = TempDir::new("fence-last-pages");
    let library = page_library(&dir);
    let index = Arc::new(SessionIndex::with_range(Some(dir.path("search.db")), None));
    index.set_pages_on(true).unwrap();
    let (refresher, release) = refreshing_pages(&index, library);
    assert!(index.pages_writes.try_lock().is_err());
    let (first, _) = spawn_toggle(&index, false, true);
    wait_for("the first off never began", || pages_off(&index));
    let (second, answered) = spawn_toggle(&index, true, true);
    wait_for("turning pages on never began", || {
        pages_on(&index) == Some(true)
    });
    still_waiting(&answered, "turning pages on returned during a refresh");
    let (third, _) = spawn_toggle(&index, false, true);
    wait_for("the last off never began", || pages_off(&index));
    release.send(()).unwrap();
    assert_eq!(refresher.join().unwrap().read, 1);
    first.join().unwrap().unwrap();
    second.join().unwrap().unwrap();
    third.join().unwrap().unwrap();
    assert_eq!(pages_on(&index), Some(false));
    assert!(index.search_pages(&["quokka"], 100.0, 30).is_empty());
}

#[test]
fn pages_turned_back_on_during_a_refresh_keep_what_it_read() {
    let dir = TempDir::new("fence-back-on-pages");
    let library = page_library(&dir);
    let index = Arc::new(SessionIndex::with_range(Some(dir.path("search.db")), None));
    index.set_pages_on(true).unwrap();
    let (refresher, release) = refreshing_pages(&index, library);
    let (first, _) = spawn_toggle(&index, false, true);
    wait_for("the off never began", || pages_off(&index));
    let (second, answered) = spawn_toggle(&index, true, true);
    wait_for("turning pages on never began", || {
        pages_on(&index) == Some(true)
    });
    still_waiting(&answered, "turning pages on returned during a refresh");
    release.send(()).unwrap();
    assert_eq!(refresher.join().unwrap().read, 1);
    first.join().unwrap().unwrap();
    second.join().unwrap().unwrap();
    assert_eq!(pages_on(&index), Some(true));
    assert_eq!(index.search_pages(&["quokka"], 100.0, 30).len(), 1);
}

#[test]
fn an_off_that_was_turned_back_on_leaves_nothing_pending() {
    let mut toggle = Toggle::default();
    toggle.turn(true);
    let ticket = toggle.turn(false).unwrap();
    let wait = toggle.turn(true).unwrap();
    assert_eq!(toggle.owed(ticket), None);
    assert_eq!(toggle.owed(wait), None);
    assert_eq!(toggle.turn(true), None);
}

fn read_only(index: &SessionIndex, on: bool) {
    let pragma = if on { "ON" } else { "OFF" };
    index
        .db
        .as_ref()
        .unwrap()
        .hold()
        .execute_batch(&format!("PRAGMA query_only = {pragma}"))
        .unwrap();
}

#[test]
fn a_clear_that_failed_is_tried_again_by_the_next_off() {
    let dir = TempDir::new("fence-retry");
    let db = warmed(&dir);
    let index = SessionIndex::with_range(Some(db.clone()), None);
    read_only(&index, true);
    assert!(index.set_sessions_on(false).is_err());
    read_only(&index, false);
    index.set_sessions_on(false).unwrap();
    assert_eq!(cached(&db), 0);
    assert!(sessions_off(&index));
}

#[test]
fn a_page_clear_that_failed_is_tried_again_by_the_next_off() {
    let dir = TempDir::new("fence-retry-pages");
    let library = page_library(&dir);
    let index = SessionIndex::with_range(Some(dir.path("search.db")), None);
    assert_eq!(index.refresh_pages(&library).read, 1);
    assert_eq!(index.search_pages(&["quokka"], 100.0, 30).len(), 1);
    read_only(&index, true);
    assert!(index.set_pages_on(false).is_err());
    read_only(&index, false);
    index.set_pages_on(false).unwrap();
    assert!(index.search_pages(&["quokka"], 100.0, 30).is_empty());
}
