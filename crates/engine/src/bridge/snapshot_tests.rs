use super::snapshot::*;
use super::wire::BridgeFailure;
use crate::viz::url::Url;
use notchling_protocol::generated::ErrorCode;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::connection::lock;

const JPEG: [u8; 4] = [0xFF, 0xD8, 0xFF, 0xD9];

fn local(url: &str) -> Url {
    Url::parse(url).expect("url")
}

#[derive(Default)]
struct Counting {
    calls: AtomicUsize,
    urls: Mutex<Vec<String>>,
    answer: Vec<u8>,
}

struct Fake(Arc<Counting>);

impl Snapshotter for Fake {
    fn capture(&self, request: &SnapshotRequest) -> Result<Vec<u8>, BridgeFailure> {
        self.0.calls.fetch_add(1, Ordering::SeqCst);
        lock(&self.0.urls).push(request.url.clone());
        Ok(self.0.answer.clone())
    }
}

fn snapshots(answer: &[u8], reachable: bool) -> (WebSnapshots, Arc<Counting>) {
    let counting = Arc::new(Counting {
        calls: AtomicUsize::new(0),
        urls: Mutex::new(Vec::new()),
        answer: answer.to_vec(),
    });
    let web = WebSnapshots::new(Box::new(Fake(counting.clone())))
        .with_reachable(Box::new(move |_| reachable));
    (web, counting)
}

#[test]
fn the_page_is_laid_out_phone_shaped() {
    let request = SnapshotRequest {
        url: "http://localhost:3000".into(),
        width: 390,
    };
    assert_eq!(request.view_size(), (390, 702));
    let tiny = SnapshotRequest {
        url: String::new(),
        width: -5,
    };
    assert_eq!(tiny.view_size(), (1, 2));
}

#[test]
fn only_web_schemes_may_be_followed() {
    assert!(allows_navigation("http"));
    assert!(allows_navigation("https"));
    assert!(allows_navigation("about"));
    assert!(!allows_navigation("file"));
    assert!(!allows_navigation("javascript"));
    assert!(!allows_navigation("data"));
}

#[test]
fn the_not_running_message_names_host_and_port() {
    assert_eq!(
        not_running_message(&local("http://localhost:3000/x")),
        "localhost:3000 không chạy trên Mac (không trả lời trong 1 giây)."
    );
    assert!(not_running_message(&local("http://localhost/")).contains("localhost:80 "));
}

#[test]
fn a_page_that_answers_http_is_reachable() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        let mut buffer = [0u8; 256];
        let n = socket.read(&mut buffer).unwrap_or(0);
        let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        String::from_utf8_lossy(&buffer[..n]).into_owned()
    });
    let url = local(&format!("http://localhost:{port}/app"));
    assert!(http_reachable(&url, Duration::from_secs(2)));
    let request = server.join().expect("server");
    assert!(request.starts_with("HEAD /app HTTP/1.1\r\n"), "{request}");
}

#[test]
fn something_that_is_not_http_is_not_a_page() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        let _ = socket.write_all(b"SSH-2.0-OpenSSH\r\n");
    });
    let url = local(&format!("http://localhost:{port}/"));
    assert!(!http_reachable(&url, Duration::from_secs(2)));
    server.join().expect("server");
}

#[test]
fn a_closed_port_is_not_reachable() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("addr").port()
    };
    let url = local(&format!("http://127.0.0.1:{port}/"));
    assert!(!http_reachable(&url, Duration::from_millis(500)));
}

#[test]
fn a_silent_server_is_given_up_on_at_the_limit() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let url = local(&format!("http://localhost:{port}/"));
    let started = std::time::Instant::now();
    assert!(!http_reachable(&url, Duration::from_millis(200)));
    assert!(started.elapsed() < Duration::from_secs(3));
    drop(listener);
}

#[test]
fn a_reachable_page_is_handed_to_the_snapshotter() {
    let (web, counting) = snapshots(&JPEG, true);
    let jpeg = web.snapshot("http://localhost:5173/", 390).expect("jpeg");
    assert_eq!(jpeg, JPEG);
    assert_eq!(counting.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_page_nobody_serves_is_not_reachable_and_is_not_shot() {
    let (web, counting) = snapshots(&JPEG, false);
    let failure = web
        .snapshot("http://localhost:3000", 390)
        .expect_err("refused");
    assert_eq!(failure.code, ErrorCode::NotReachable);
    assert!(
        failure.message.contains("localhost:3000"),
        "{}",
        failure.message
    );
    assert_eq!(counting.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn only_local_http_pages_are_shot() {
    let (web, counting) = snapshots(&JPEG, true);
    for url in [
        "http://example.com/",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "http://localhost.evil.com/",
        "",
    ] {
        let failure = web.snapshot(url, 390).expect_err(url);
        assert_eq!(failure.code, ErrorCode::Invalid, "{url}");
    }
    assert_eq!(counting.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn the_snapshotter_gets_the_checked_url_without_user_info() {
    let (web, counting) = snapshots(&JPEG, true);
    web.snapshot("http://user:pw@localhost:3000/a?x=1#top", 390)
        .expect("jpeg");
    web.snapshot("http://evil.com@localhost:3000", 390)
        .expect("jpeg");
    let urls = lock(&counting.urls).clone();
    assert_eq!(
        urls,
        ["http://localhost:3000/a?x=1#top", "http://localhost:3000"]
    );
}

#[test]
fn bytes_that_are_not_a_jpeg_are_an_internal_error() {
    let (web, _) = snapshots(b"not a jpeg", true);
    let failure = web.snapshot("http://127.0.0.1:8080", 390).expect_err("bad");
    assert_eq!(failure.code, ErrorCode::Internal);
}

#[test]
fn without_a_renderer_the_answer_says_so() {
    let web = WebSnapshots::new(Box::new(Unsupported)).with_reachable(Box::new(|_| true));
    let failure = web
        .snapshot("http://localhost:3000", 390)
        .expect_err("none");
    assert_eq!(failure.code, ErrorCode::NotReachable);
    assert!(failure.message.contains("http://localhost:3000"));
}
