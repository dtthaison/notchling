use crate::{Conn, LinkEvents};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> Scratch {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let path = PathBuf::from(format!("/tmp/nlcn-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub enum Ev {
    Opened(Arc<dyn Conn>),
    Line(String, Vec<u8>),
    Closed(String),
}

pub struct Rec {
    tx: Mutex<Sender<Ev>>,
}

impl Rec {
    pub fn new() -> (Arc<Rec>, Receiver<Ev>) {
        let (tx, rx) = channel();
        (Arc::new(Rec { tx: Mutex::new(tx) }), rx)
    }

    fn push(&self, ev: Ev) {
        let _ = self.tx.lock().unwrap().send(ev);
    }
}

impl LinkEvents for Rec {
    fn opened(&self, conn: Arc<dyn Conn>) {
        self.push(Ev::Opened(conn));
    }

    fn line(&self, peer_fp: &str, line: Vec<u8>) {
        self.push(Ev::Line(peer_fp.to_string(), line));
    }

    fn closed(&self, peer_fp: &str, _conn_id: u64) {
        self.push(Ev::Closed(peer_fp.to_string()));
    }
}

pub const WAIT: Duration = Duration::from_secs(10);

pub fn opened(rx: &Receiver<Ev>) -> Arc<dyn Conn> {
    loop {
        match rx.recv_timeout(WAIT).expect("an opened link") {
            Ev::Opened(conn) => return conn,
            _ => continue,
        }
    }
}

pub fn line(rx: &Receiver<Ev>) -> (String, Vec<u8>) {
    loop {
        match rx.recv_timeout(WAIT).expect("a line") {
            Ev::Line(fp, line) => return (fp, line),
            _ => continue,
        }
    }
}

pub fn closed(rx: &Receiver<Ev>) -> String {
    loop {
        match rx.recv_timeout(WAIT).expect("a closed link") {
            Ev::Closed(fp) => return fp,
            _ => continue,
        }
    }
}

pub fn quiet(rx: &Receiver<Ev>, wait: Duration) -> bool {
    rx.recv_timeout(wait).is_err()
}
