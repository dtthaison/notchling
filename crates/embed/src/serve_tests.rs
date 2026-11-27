use crate::serve::{serve, Encode, Shared};
use serde_json::{json, Value};
use std::io::{BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Fake;

impl Encode for Fake {
    fn model(&self) -> &str {
        "fake@1/p1"
    }

    fn dims(&self) -> usize {
        2
    }

    fn encode(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        if texts.iter().any(|t| t.starts_with("slow")) {
            std::thread::sleep(Duration::from_millis(400));
        }
        if texts.iter().any(|t| t.starts_with("boom")) {
            return Err("boom".to_string());
        }
        Ok(texts.iter().map(|t| vec![t.len() as f32, 1.0]).collect())
    }
}

struct Feed(Receiver<String>, Vec<u8>);

impl Read for Feed {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.1.is_empty() {
            match self.0.recv() {
                Ok(line) => self.1 = format!("{line}\n").into_bytes(),
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.1.len());
        buf[..n].copy_from_slice(&self.1[..n]);
        self.1.drain(..n);
        Ok(n)
    }
}

#[derive(Clone, Default)]
struct Out(Arc<Mutex<Vec<u8>>>);

impl Write for Out {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Out {
    fn lines(&self) -> Vec<Value> {
        let bytes = self.0.lock().unwrap().clone();
        let whole = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        String::from_utf8(bytes[..whole].to_vec())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn wait_for(&self, n: usize) -> Vec<Value> {
        for _ in 0..200 {
            let lines = self.lines();
            if lines.len() >= n {
                return lines;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.lines()
    }
}

struct Clock {
    base: Instant,
    plus: Arc<Mutex<Duration>>,
}

impl Clock {
    fn new() -> (Clock, Arc<Mutex<Duration>>) {
        let plus = Arc::new(Mutex::new(Duration::ZERO));
        (
            Clock {
                base: Instant::now(),
                plus: plus.clone(),
            },
            plus,
        )
    }

    fn now(&self) -> Instant {
        self.base + *self.plus.lock().unwrap()
    }
}

fn start(
    clock: Clock,
) -> (
    Sender<String>,
    Out,
    std::thread::JoinHandle<Result<(), String>>,
) {
    let (tx, rx) = channel();
    let out = Out::default();
    let writer = out.clone();
    let handle = std::thread::spawn(move || {
        serve(
            Arc::new(Fake),
            BufReader::new(Feed(rx, Vec::new())),
            writer,
            &|| clock.now(),
        )
    });
    (tx, out, handle)
}

fn line(id: u64, lane: &str, texts: &[&str]) -> String {
    json!({"id": id, "lane": lane, "texts": texts}).to_string()
}

fn ids(lines: &[Value], lane_of: &dyn Fn(u64) -> bool) -> Vec<u64> {
    lines
        .iter()
        .filter_map(|l| l["id"].as_u64())
        .filter(|id| lane_of(*id))
        .collect()
}

#[test]
fn serve_answers_in_order_per_lane() {
    let (clock, _) = Clock::new();
    let (tx, out, handle) = start(clock);
    for id in 1..=3 {
        tx.send(line(id, "query", &["ab"])).unwrap();
        tx.send(line(10 + id, "index", &["abc", "d"])).unwrap();
    }
    let lines = out.wait_for(7);
    drop(tx);
    assert_eq!(handle.join().unwrap(), Ok(()));
    assert_eq!(
        lines[0],
        json!({"ready": true, "model": "fake@1/p1", "dims": 2})
    );
    assert_eq!(ids(&lines, &|id| id < 10), vec![1, 2, 3]);
    assert_eq!(ids(&lines, &|id| id > 10), vec![11, 12, 13]);
    assert_eq!(lines.len(), 7);
    let first = lines.iter().find(|l| l["id"] == 11).unwrap();
    assert_eq!(first["vecs"].as_array().unwrap().len(), 2);
}

#[test]
fn a_query_overtakes_a_batch() {
    let (clock, _) = Clock::new();
    let (tx, out, handle) = start(clock);
    tx.send(line(1, "index", &["slow one"])).unwrap();
    std::thread::sleep(Duration::from_millis(60));
    tx.send(line(2, "query", &["fast"])).unwrap();
    let lines = out.wait_for(3);
    drop(tx);
    handle.join().unwrap().unwrap();
    assert_eq!(ids(&lines, &|_| true), vec![2, 1]);
}

#[test]
fn exits_after_five_idle_minutes_on_a_fake_clock() {
    let (clock, plus) = Clock::new();
    let (tx, out, handle) = start(clock);
    tx.send(line(1, "query", &["ab"])).unwrap();
    out.wait_for(2);
    std::thread::sleep(Duration::from_millis(100));
    *plus.lock().unwrap() = Duration::from_secs(299);
    std::thread::sleep(Duration::from_millis(150));
    assert!(!handle.is_finished());
    *plus.lock().unwrap() = Duration::from_secs(301);
    for _ in 0..100 {
        if handle.is_finished() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(handle.is_finished());
    assert_eq!(handle.join().unwrap(), Ok(()));
    drop(tx);
}

#[test]
fn bad_lines_get_an_error_not_a_crash() {
    let (clock, _) = Clock::new();
    let (tx, out, handle) = start(clock);
    tx.send("not json".to_string()).unwrap();
    tx.send(line(5, "elsewhere", &["a"])).unwrap();
    tx.send(line(6, "query", &[])).unwrap();
    tx.send(line(7, "index", &["a"; 9])).unwrap();
    tx.send(line(8, "query", &["boom"])).unwrap();
    tx.send(line(9, "query", &["ok"])).unwrap();
    let lines = out.wait_for(7);
    drop(tx);
    handle.join().unwrap().unwrap();
    let by = |id: u64| lines.iter().find(|l| l["id"] == id).unwrap().clone();
    assert!(by(0)["error"].is_string());
    assert!(by(5)["error"].is_string());
    assert!(by(6)["error"].is_string());
    assert!(by(7)["error"].is_string());
    assert_eq!(by(8)["error"], "boom");
    assert!(by(9)["vecs"].is_array());
}

#[test]
fn eof_never_reads_as_drained_while_a_request_is_queued() {
    for round in 0..20_000 {
        let shared = Arc::new(Shared::new(Arc::new(Mutex::new(Box::new(std::io::sink())))));
        let go = Arc::new(AtomicBool::new(false));
        let reader = {
            let (shared, go) = (shared.clone(), go.clone());
            std::thread::spawn(move || {
                while !go.load(Ordering::SeqCst) {
                    std::hint::spin_loop();
                }
                shared.pending.fetch_add(1, Ordering::SeqCst);
                shared.eof.store(true, Ordering::SeqCst);
            })
        };
        go.store(true, Ordering::SeqCst);
        while !shared.eof.load(Ordering::SeqCst) {
            assert!(
                !shared.drained(),
                "round {round}: drained with a request in flight"
            );
        }
        assert!(!shared.drained(), "round {round}: drained after eof");
        reader.join().expect("reader");
    }
}
