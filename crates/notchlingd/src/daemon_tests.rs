//! The daemon's start order, in process. The first read of the session list can be slow (a long
//! list, a cold disk, a loaded machine), and a client that starts the daemon gives its socket 3 s,
//! so the socket must not wait for that read: it answers at once, `sessions.list` waits for the
//! first read (bounded) and a read that lands later follows as `sessions.changed`. A test holds the
//! first read at `DaemonOptions::before_scan` to stand in for the slow one. The data is a copy of
//! `crates/app/fixtures/notch` in /tmp with a socket of its own, never the real folders.
#![cfg(unix)]

use crate::daemon::{run, DaemonError, DaemonOptions};
use crate::layout::Layout;
use notchling_local_api::{ops, Client, ClientKind, ConnectOptions};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// A hold for `before_scan` that blocks its first call until the sender is dropped (or sent to).
/// Later calls pass. A test that is lost can't keep the thread for more than 30 s.
fn hold() -> (Arc<dyn Fn() + Send + Sync>, Sender<()>) {
    let (release, wait) = mpsc::channel::<()>();
    let wait = Mutex::new(Some(wait));
    let hold: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let first = wait.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(wait) = first {
            let _ = wait.recv_timeout(Duration::from_secs(30));
        }
    });
    (hold, release)
}

/// A daemon running on a thread of this process over a fixture copy.
struct Running {
    layout: Layout,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<(), DaemonError>>>,
    release: Option<Sender<()>>,
    root: PathBuf,
}

impl Running {
    /// Starts it; with `held` the first read of the list waits until [`Running::release`].
    fn start(tag: &str, mut options: DaemonOptions, held: bool) -> Running {
        let root = PathBuf::from(format!("/tmp/nl-dt-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/fixtures/notch"),
            &root,
        );
        let socket = root.join("s/notchlingd.sock");
        let layout = Layout::resolve(Some(&socket), Some(root.as_os_str()), || None).unwrap();
        let release = held.then(|| {
            let (before_scan, release) = hold();
            options.before_scan = Some(before_scan);
            release
        });
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (layout, stop) = (layout.clone(), stop.clone());
            std::thread::spawn(move || run(layout, options, &|| stop.load(Ordering::SeqCst)))
        };
        Running {
            layout,
            stop,
            thread: Some(thread),
            release,
            root,
        }
    }

    /// Lets the held first read go on.
    fn release(&mut self) {
        self.release.take();
    }

    fn socket(&self) -> &Path {
        &self.layout.socket
    }

    /// True once something accepts connections on the socket.
    fn answers_within(&self, wait: Duration) -> bool {
        let deadline = Instant::now() + wait;
        while Instant::now() < deadline {
            if std::os::unix::net::UnixStream::connect(self.socket()).is_ok() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// A client of this daemon. Its server is this test binary, not a `notchling`, so the check on
    /// who serves is off.
    fn client(&self) -> Client {
        let options = ConnectOptions {
            timeout: Duration::from_secs(20),
            check_server: false,
        };
        Client::connect_with(self.socket(), ClientKind::Cli, "0.1.0", &options).unwrap()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.release.take();
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn ids(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_socket_answers_while_the_first_read_is_still_held() {
    let daemon = Running::start("bind", DaemonOptions::default(), true);
    assert!(
        daemon.answers_within(Duration::from_secs(10)),
        "no socket while the first read was held: the daemon reads the list before it binds"
    );
    assert!(daemon.client().status().is_ok());
}

#[test]
fn sessions_list_waits_for_the_first_read_and_returns_it() {
    let mut daemon = Running::start("wait", DaemonOptions::default(), true);
    assert!(daemon.answers_within(Duration::from_secs(10)));
    let mut client = daemon.client();
    // Subscribe, then list: the order that loses no change.
    client.subscribe(&["sessions"]).unwrap();
    let (answer, answered) = mpsc::channel();
    let asking = std::thread::spawn(move || {
        let _ = answer.send(client.call(ops::SESSIONS_LIST, json!({})));
    });
    assert!(
        answered.recv_timeout(Duration::from_millis(400)).is_err(),
        "answered before the first read was done"
    );
    daemon.release();
    let list = answered
        .recv_timeout(Duration::from_secs(20))
        .expect("an answer once the first read is done")
        .unwrap();
    assert!(ids(&list).contains(&"sess-notch".to_string()), "{list}");
    assert!(list.as_array().unwrap().len() >= 5, "{list}");
    asking.join().unwrap();
}

#[test]
fn a_list_that_runs_out_of_patience_gets_what_there_is_and_the_read_follows_as_an_event() {
    let options = DaemonOptions {
        first_list_wait: Duration::from_millis(150),
        ..DaemonOptions::default()
    };
    let mut daemon = Running::start("late", options, true);
    assert!(daemon.answers_within(Duration::from_secs(10)));
    let mut client = daemon.client();
    client.subscribe(&["sessions"]).unwrap();
    let started = Instant::now();
    let early = client.call(ops::SESSIONS_LIST, json!({})).unwrap();
    assert!(started.elapsed() >= Duration::from_millis(150));
    assert_eq!(early, json!([]), "what there is while the read is held");

    daemon.release();
    let event = client
        .next_event(Duration::from_secs(20))
        .unwrap()
        .expect("the first read, as sessions.changed");
    assert_eq!(event.event, "sessions.changed");
    assert!(ids(&event.data).contains(&"sess-notch".to_string()));
    // The list now is the same one.
    assert_eq!(
        client.call(ops::SESSIONS_LIST, json!({})).unwrap(),
        event.data
    );
}

#[test]
fn a_second_daemon_is_refused_before_it_reads_the_list() {
    let first = Running::start("lock", DaemonOptions::default(), false);
    assert!(first.answers_within(Duration::from_secs(10)));

    let read = Arc::new(AtomicBool::new(false));
    let options = DaemonOptions {
        before_scan: Some({
            let read = read.clone();
            Arc::new(move || read.store(true, Ordering::SeqCst))
        }),
        ..DaemonOptions::default()
    };
    let result = run(first.layout.clone(), options, &|| true);
    assert!(
        matches!(result, Err(DaemonError::AlreadyRunning(_))),
        "{result:?}"
    );
    assert!(
        !read.load(Ordering::SeqCst),
        "the second daemon read the list before the lock refused it"
    );
    // The first goes on serving.
    assert!(first.client().status().is_ok());
}
