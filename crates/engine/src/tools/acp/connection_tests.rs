use super::*;
use crate::tools::acp::fake::{FakeHandle, FakeSpawner};
#[cfg(unix)]
use crate::tools::acp::RealSpawner;
use crate::tools::acp::{Spawner, METHOD_NOT_FOUND};
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
enum Heard {
    Note(String, Value),
    Request(Value, String, Value),
    Exit(Option<String>),
}

struct Recorder(Mutex<Sender<Heard>>);

impl Handler for Recorder {
    fn notification(&self, method: &str, params: &Value) {
        self.send(Heard::Note(method.into(), params.clone()));
    }
    fn request(&self, id: &Value, method: &str, params: &Value) {
        self.send(Heard::Request(id.clone(), method.into(), params.clone()));
    }
    fn exit(&self, message: Option<String>) {
        self.send(Heard::Exit(message));
    }
}

impl Recorder {
    fn send(&self, heard: Heard) {
        let _ = self.0.lock().unwrap().send(heard);
    }
}

fn rig() -> (Connection, FakeHandle, std::sync::mpsc::Receiver<Heard>) {
    let spawner = FakeSpawner::new();
    let spawned = spawner.spawn("agy", &["acp".into()], "/tmp").unwrap();
    let handle = spawner.spawned().remove(0);
    let (tx, rx) = channel();
    let conn = Connection::new(spawned, "Antigravity");
    conn.start(Arc::new(Recorder(Mutex::new(tx))));
    (conn, handle, rx)
}

fn heard(rx: &std::sync::mpsc::Receiver<Heard>) -> Heard {
    rx.recv_timeout(Duration::from_secs(10)).expect("a message")
}

fn call(conn: &Connection, method: &str) -> std::sync::mpsc::Receiver<Result<Value, String>> {
    let (tx, rx) = channel();
    conn.request(
        method,
        json!({"a": 1}),
        Box::new(move |r| {
            let _ = tx.send(r);
        }),
    );
    rx
}

fn done(rx: &std::sync::mpsc::Receiver<Result<Value, String>>) -> Result<Value, String> {
    rx.recv_timeout(Duration::from_secs(10)).expect("an answer")
}

#[test]
fn a_request_goes_out_with_a_counting_id_and_its_result_comes_back() {
    let (conn, process, _rx) = rig();
    let first = call(&conn, "initialize");
    let second = call(&conn, "session/new");
    let sent = process.written();
    assert_eq!(sent[0]["id"], 1);
    assert_eq!(sent[0]["jsonrpc"], "2.0");
    assert_eq!(sent[0]["method"], "initialize");
    assert_eq!(sent[1]["id"], 2);
    process.push(&json!({"jsonrpc":"2.0","id":2,"result":{"sessionId":"s"}}));
    process.push(&json!({"jsonrpc":"2.0","id":1,"result":{"ok":true}}));
    assert_eq!(done(&second).unwrap()["sessionId"], "s");
    assert_eq!(done(&first).unwrap()["ok"], true);
}

#[test]
fn an_error_response_and_an_id_sent_back_as_text_both_resolve() {
    let (conn, process, _rx) = rig();
    let first = call(&conn, "session/load");
    let second = call(&conn, "session/prompt");
    process
        .push(&json!({"jsonrpc":"2.0","id":"1","error":{"code":-1,"message":"no such session"}}));
    process.push(&json!({"jsonrpc":"2.0","id":2,"error":{"code":-1}}));
    assert_eq!(done(&first), Err("no such session".to_string()));
    assert_eq!(
        done(&second),
        Err("Antigravity returned an error.".to_string())
    );
}

#[test]
fn a_result_that_is_not_an_object_reads_as_empty() {
    let (conn, process, _rx) = rig();
    let reply = call(&conn, "session/cancel");
    process.push(&json!({"jsonrpc":"2.0","id":1,"result":null}));
    assert_eq!(done(&reply).unwrap(), json!({}));
}

#[test]
fn notifications_and_agent_requests_reach_the_handler_in_order() {
    let (_conn, process, rx) = rig();
    process.push(&json!({"jsonrpc":"2.0","method":"session/update","params":{"update":{"a":1}}}));
    process.push(
        &json!({"jsonrpc":"2.0","method":"session/request_permission","params":{"x":1},"id":"1"}),
    );
    process.push(&json!({"jsonrpc":"2.0","method":"x/number","id":7}));
    process.push(&json!({"jsonrpc":"2.0","method":"x/bare"}));
    assert_eq!(
        heard(&rx),
        Heard::Note("session/update".into(), json!({"update": {"a": 1}}))
    );
    assert_eq!(
        heard(&rx),
        Heard::Request(
            json!("1"),
            "session/request_permission".into(),
            json!({"x": 1})
        )
    );
    assert_eq!(
        heard(&rx),
        Heard::Request(json!(7), "x/number".into(), json!({}))
    );
    assert_eq!(heard(&rx), Heard::Note("x/bare".into(), json!({})));
}

#[test]
fn answers_to_the_agent_keep_its_id_type_and_unknown_calls_get_32601() {
    let (conn, process, _rx) = rig();
    conn.respond(
        &json!("1"),
        json!({"outcome": {"outcome": "selected", "optionId": "allow"}}),
    );
    conn.respond(&json!(5), json!({}));
    conn.respond_error(&json!("fs-1"), METHOD_NOT_FOUND, "Not supported");
    let sent = process.written();
    assert_eq!(sent[0]["id"], json!("1"));
    assert_eq!(sent[0]["result"]["outcome"]["optionId"], "allow");
    assert_eq!(sent[1]["id"], json!(5));
    assert_eq!(sent[2]["id"], json!("fs-1"));
    assert_eq!(sent[2]["error"]["code"], -32601);
    assert_eq!(sent[2]["error"]["message"], "Not supported");
    assert!(sent[2].get("method").is_none());
}

#[test]
fn framing_faults_are_dropped_and_the_connection_carries_on() {
    let (conn, process, rx) = rig();
    let reply = call(&conn, "initialize");
    process.push_line("this is not json");
    process.push_line("[1,2,3]");
    process.push_line("");
    process.push_line("{\"jsonrpc\":\"2.0\"}");
    process.push_oversize();
    process.push(&json!({"jsonrpc":"2.0","id":99,"result":{}}));
    process.push(&json!({"jsonrpc":"2.0","id":true,"result":{}}));
    process.push(&json!({"jsonrpc":"2.0","id":1.5,"result":{}}));
    process.push(&json!({"jsonrpc":"2.0","id":1,"result":{"fine":1}}));
    assert_eq!(done(&reply).unwrap()["fine"], 1);
    process.push(&json!({"jsonrpc":"2.0","method":"session/update","params":{}}));
    assert!(matches!(heard(&rx), Heard::Note(..)));
    assert!(!conn.is_closed());
}

#[test]
fn a_process_that_dies_fails_everything_waiting_and_tells_the_handler_once() {
    let (conn, process, rx) = rig();
    let a = call(&conn, "session/prompt");
    let b = call(&conn, "session/set_config_option");
    process.die(Some("agy: auth expired"));
    assert_eq!(done(&a), Err("agy: auth expired".to_string()));
    assert_eq!(done(&b), Err("agy: auth expired".to_string()));
    assert_eq!(heard(&rx), Heard::Exit(Some("agy: auth expired".into())));
    assert!(conn.is_closed());
    let late = call(&conn, "session/prompt");
    assert_eq!(done(&late), Err("Antigravity isn't running.".to_string()));
    let writes = process.written().len();
    conn.notify("session/cancel", json!({}));
    conn.respond(&json!(1), json!({}));
    assert_eq!(process.written().len(), writes);
    assert!(
        rx.recv_timeout(Duration::from_millis(100)).is_err(),
        "one exit only"
    );
}

#[test]
fn a_death_with_no_stderr_says_the_tool_stopped() {
    let (conn, process, _rx) = rig();
    let a = call(&conn, "session/prompt");
    process.die(None);
    assert_eq!(done(&a), Err("Antigravity stopped.".to_string()));
}

#[test]
fn closing_kills_the_process_fails_waiters_and_is_not_an_exit() {
    let (conn, process, rx) = rig();
    let a = call(&conn, "session/prompt");
    conn.close();
    conn.close();
    assert_eq!(done(&a), Err("Antigravity was closed.".to_string()));
    assert!(process.killed());
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
    assert!(conn.is_closed());
}

#[test]
fn a_broken_pipe_ends_the_connection_like_a_death() {
    let (conn, process, rx) = rig();
    let a = call(&conn, "session/prompt");
    process.break_pipe();
    conn.notify("session/cancel", json!({}));
    assert_eq!(done(&a), Err("Antigravity stopped.".to_string()));
    assert_eq!(heard(&rx), Heard::Exit(None));
}

#[cfg(unix)]
struct Stuck {
    conn: Connection,
    writer: std::sync::mpsc::Receiver<Result<Value, String>>,
}

#[cfg(unix)]
fn stuck() -> Stuck {
    let spawner = RealSpawner::new(&std::env::temp_dir());
    let args = vec!["-c".to_string(), "exec sleep 60".to_string()];
    let spawned = spawner
        .spawn("/bin/sh", &args, &std::env::temp_dir().to_string_lossy())
        .unwrap();
    let conn = Connection::new(spawned, "Antigravity");
    let (tx, _rx) = channel();
    conn.start(Arc::new(Recorder(Mutex::new(tx))));
    let (done_tx, writer) = channel();
    let sender = conn.clone();
    std::thread::spawn(move || {
        sender.request(
            "session/prompt",
            json!({"text": "x".repeat(8 << 20)}),
            Box::new(move |r| {
                let _ = done_tx.send(r);
            }),
        );
    });
    let until = Instant::now() + Duration::from_secs(30);
    while !writing(&conn) {
        assert!(Instant::now() < until, "the prompt's write never started");
        std::thread::sleep(Duration::from_millis(5));
    }
    Stuck { conn, writer }
}

#[cfg(unix)]
fn writing(conn: &Connection) -> bool {
    matches!(conn.inner.process.try_lock(), Err(TryLockError::WouldBlock))
}

#[cfg(unix)]
fn returns_within(limit: Duration, f: impl FnOnce() + Send + 'static) -> bool {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        f();
        let _ = tx.send(());
    });
    rx.recv_timeout(limit).is_ok()
}

#[cfg(unix)]
#[test]
fn closing_ends_a_process_that_is_not_reading_and_frees_the_blocked_write() {
    let s = stuck();
    let conn = s.conn.clone();
    assert!(
        returns_within(Duration::from_secs(3), move || conn.close()),
        "close waited on the blocked write"
    );
    assert!(s.conn.is_closed());
    assert_eq!(
        s.writer.recv_timeout(Duration::from_secs(5)).unwrap(),
        Err("Antigravity was closed.".to_string())
    );
}

#[cfg(unix)]
#[test]
fn a_message_behind_a_blocked_write_gives_up_and_ends_the_connection() {
    let s = stuck();
    let conn = s.conn.clone();
    assert!(
        returns_within(Duration::from_secs(10), move || {
            conn.notify("session/cancel", json!({}));
        }),
        "the cancel waited on the blocked write for good"
    );
    assert!(s.conn.is_closed());
    assert!(s
        .writer
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .is_err());
}
