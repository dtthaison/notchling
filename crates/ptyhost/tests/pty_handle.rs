#![cfg(unix)]

mod common;

use common::{wait_until, Scratch};
use notchling_ptyhost::host::{Handle, Listener};
use notchling_ptyhost::pty_handle::PtyHandle;
use std::path::Path;
use std::sync::mpsc::channel;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

struct Session(Arc<PtyHandle>);

impl Drop for Session {
    fn drop(&mut self) {
        self.0.terminate();
    }
}

fn shell(script: &str, scratch: &Scratch) -> Session {
    let environment = [("PATH".to_string(), "/usr/bin:/bin".to_string())];
    let handle = PtyHandle::spawn(
        Path::new("/bin/sh"),
        &["-c".to_string(), script.to_string()],
        &scratch.dir,
        None,
        &environment,
    )
    .expect("sh starts");
    Session(handle)
}

type Bytes = Arc<Mutex<Vec<u8>>>;

fn text(bytes: &Bytes) -> String {
    String::from_utf8_lossy(&bytes.lock().expect("lock")).into_owned()
}

fn collector() -> (Listener, Bytes) {
    let got: Bytes = Arc::default();
    let sink = got.clone();
    let listener: Listener = Arc::new(move |chunk: &[u8]| {
        sink.lock().expect("lock").extend_from_slice(chunk);
    });
    (listener, got)
}

struct Stuck {
    listener: Listener,
    got: Bytes,
    gate: Arc<(Mutex<bool>, Condvar)>,
}

impl Stuck {
    fn new() -> Stuck {
        let got: Bytes = Arc::default();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (sink, wait) = (got.clone(), gate.clone());
        let listener: Listener = Arc::new(move |chunk: &[u8]| {
            let (open, changed) = &*wait;
            let mut open = open.lock().expect("lock");
            while !*open {
                open = changed.wait(open).expect("wait");
            }
            drop(open);
            sink.lock().expect("lock").extend_from_slice(chunk);
        });
        Stuck {
            listener,
            got,
            gate,
        }
    }

    fn release(&self) {
        *self.gate.0.lock().expect("lock") = true;
        self.gate.1.notify_all();
    }
}

#[test]
fn a_client_that_stops_reading_does_not_hold_up_another() {
    let scratch = Scratch::new();
    let session = shell(
        "read go; echo one; sleep 0.3; echo two; sleep 0.3; echo three; exec sleep 30",
        &scratch,
    );
    let stuck = Stuck::new();
    let (reader, got) = collector();
    let stuck_token = session.0.add_listener(stuck.listener.clone());
    session.0.add_listener(reader);
    session.0.write(b"go\n");
    wait_until(|| text(&got).contains("three"));

    let handle = session.0.clone();
    let (done, finished) = channel();
    std::thread::spawn(move || {
        handle.remove_listener(stuck_token);
        let _ = done.send(());
    });
    finished
        .recv_timeout(Duration::from_secs(2))
        .expect("remove_listener returned while the client was still stuck");
    stuck.release();
}

#[test]
fn a_listener_added_late_gets_the_scrollback_then_the_live_bytes_in_order() {
    let scratch = Scratch::new();
    let session = shell(
        "i=0; while [ $i -lt 60 ]; do echo line$i; i=$((i+1)); sleep 0.01; done; exec sleep 30",
        &scratch,
    );
    std::thread::sleep(Duration::from_millis(200));
    let (listener, got) = collector();
    session.0.add_listener(listener);
    let expected: String = (0..60).map(|i| format!("line{i}\r\n")).collect();
    wait_until(|| text(&got).len() >= expected.len());
    assert_eq!(text(&got), expected, "nothing lost, repeated or reordered");
}

#[test]
fn a_client_that_falls_far_behind_is_dropped_and_the_others_get_everything() {
    use notchling_ptyhost::pty_handle::LISTENER_BACKLOG_LIMIT;
    let scratch = Scratch::new();
    let total = 2 * LISTENER_BACKLOG_LIMIT + 1024;
    let session = shell(
        &format!("read go; head -c {total} /dev/zero | tr '\\0' x; echo; echo END; exec sleep 30"),
        &scratch,
    );
    let stuck = Stuck::new();
    let (reader, got) = collector();
    session.0.add_listener(stuck.listener.clone());
    session.0.add_listener(reader);
    session.0.write(b"go\n");
    wait_until(|| text(&got).contains("END"));
    assert!(got.lock().expect("lock").len() > total, "all of it");

    stuck.release();
    std::thread::sleep(Duration::from_millis(300));
    let delivered = stuck.got.lock().expect("lock").len();
    assert!(
        delivered < total / 2,
        "the stuck client was given {delivered} of {total} bytes: its backlog is not bounded"
    );
}
