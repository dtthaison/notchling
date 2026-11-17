use super::*;
use crate::sessions::iso::parse_iso_ms;
use crate::sessions::testutil::TempDir;
use crate::transcript::message::Message;
use crate::transcript::queue_state::{queue_lists, queue_state};
use serde_json::{json, Value};

fn stamp(second: u32, ms: u32) -> String {
    format!("2026-11-17T16:16:{second:02}.{ms:03}Z")
}

fn since(second: u32) -> i64 {
    parse_iso_ms(&stamp(second, 0)).unwrap()
}

fn message(text: &str) -> Message {
    Message::parse(text, |_| false)
}

fn picture(text: &str) -> Message {
    Message {
        pictures: 1,
        ..message(text)
    }
}

fn seen(enqueued: usize, taken: usize) -> Option<Seen> {
    Some(Seen { enqueued, taken })
}

fn write(lines: &[String]) -> (TempDir, String) {
    let dir = TempDir::new("sent-big");
    let path = dir.write("t.jsonl", &format!("{}\n", lines.join("\n")));
    let path = path.to_string_lossy().into_owned();
    (dir, path)
}

fn enqueue(content: &str, second: u32, ms: u32) -> String {
    json!({"type": "queue-operation", "operation": "enqueue",
        "timestamp": stamp(second, ms), "sessionId": "s", "content": content})
    .to_string()
}

fn absorbed(content: &str, second: u32, ms: u32) -> String {
    json!({"type": "queue-operation", "operation": "remove",
        "timestamp": stamp(second, ms), "sessionId": "s", "content": content,
        "reason": "absorbed_mid_turn", "commandUuid": "c-1", "deliveryId": "d-1"})
    .to_string()
}

fn taken(prompt: Value, origin: &str, second: u32, ms: u32) -> String {
    let at = stamp(second, ms);
    format!(
        r#"{{"parentUuid":"p","isSidechain":false,"attachment":{{"type":"queued_command","prompt":{prompt},"source_uuid":"c-1","delivery_id":"d-1","commandMode":"prompt","origin":{{"kind":"{origin}"}},"timestamp":"{at}","humanTurn":true}},"type":"attachment","timestamp":"{at}","sessionId":"s"}}"#
    )
}

fn old(second: u32) -> String {
    json!({"type": "assistant", "timestamp": stamp(second, 0),
        "message": {"role": "assistant", "content": [{"type": "text", "text": "earlier"}]}})
    .to_string()
}

fn tool_result(bytes: usize, second: u32, ms: u32) -> String {
    json!({"type": "user", "timestamp": stamp(second, ms),
        "toolUseResult": {"stdout": "x".repeat(bytes)},
        "message": {"role": "user", "content": [
            {"tool_use_id": "t", "type": "tool_result", "content": "x".repeat(bytes)}]}})
    .to_string()
}

fn blob(bytes: usize) -> String {
    json!({"type": "api-request-blob", "sessionId": "s", "hash": "h",
        "message": {"role": "user", "content": [{"type": "text", "text": "y".repeat(bytes)}]}})
    .to_string()
}

fn image(bytes: usize) -> Value {
    json!({"type": "image", "source": {"type": "base64", "media_type": "image/jpeg",
        "data": "A".repeat(bytes)}})
}

#[test]
fn a_picture_take_survives_the_copies_of_its_image_written_after_it() {
    let text = "please check the layout";
    let prompt = json!([{"type": "text", "text": format!("[Image #4]{text}")}, image(700_000)]);
    let lines = [
        old(0),
        enqueue(&format!("[Image #4]{text}"), 20, 198),
        absorbed(&format!("[Image #4]{text}"), 20, 401),
        taken(prompt, "human", 20, 197),
        blob(700_000),
        old(25),
        old(26),
    ];
    let (_dir, path) = write(&lines);
    assert_eq!(
        count_since(&path, &picture(text), since(18)),
        seen(1, 1),
        "a 700 KB image on the attachment and again in the blob must not push the take out of the window"
    );
}

#[test]
fn a_text_take_survives_big_tool_results_written_after_it() {
    let text = "change the position of the home banner";
    let lines = [
        old(0),
        enqueue(text, 20, 480),
        tool_result(1_300_000, 20, 978),
        absorbed(text, 20, 983),
        blob(1_000_000),
        tool_result(1_300_000, 20, 978),
        taken(json!(text), "human", 20, 478),
        old(25),
    ];
    let (_dir, path) = write(&lines);
    assert_eq!(
        count_since(&path, &message(text), since(18)),
        seen(1, 1),
        "the take is the attachment stamped before the enqueue, written after the remove"
    );
}

#[test]
fn a_take_inside_a_window_that_does_not_reach_back_is_still_a_take() {
    let text = "ship the fix";
    let filler = json!({"type": "assistant", "timestamp": stamp(40, 0),
        "message": {"role": "assistant", "content": [{"type": "text", "text": "w".repeat(900)}]}})
    .to_string();
    let mut lines: Vec<String> = (0..1_300).map(|_| filler.clone()).collect();
    lines.push(taken(json!(text), "human", 41, 0));
    let (_dir, path) = write(&lines);
    assert_eq!(count_since(&path, &message(text), since(0)), seen(0, 1));
    assert_eq!(count_since(&path, &message("other words"), since(0)), None);
}

#[test]
fn a_late_attachment_stamped_before_its_enqueue_is_a_take_after_the_remove() {
    let text = "move the tab bar";
    let lines = [
        old(0),
        enqueue(text, 20, 280),
        absorbed(text, 20, 983),
        taken(json!(text), "human", 20, 278),
    ];
    let (_dir, path) = write(&lines);
    assert_eq!(count_since(&path, &message(text), since(18)), seen(1, 1));
    let state = queue_state(&path).unwrap();
    assert_eq!(state.waiting, Vec::<String>::new());
    assert_eq!(state.unknown, 0);
}

#[test]
fn a_remove_with_no_take_is_not_delivered() {
    let text = "move the tab bar";
    let plain = json!({"type": "queue-operation", "operation": "remove",
        "timestamp": stamp(21, 0), "sessionId": "s", "content": text})
    .to_string();
    let pop = json!({"type": "queue-operation", "operation": "popAll",
        "timestamp": stamp(21, 500), "sessionId": "s", "content": text})
    .to_string();
    for end in [plain, pop] {
        let lines = [old(0), enqueue(text, 20, 280), end];
        let (_dir, path) = write(&lines);
        assert_eq!(count_since(&path, &message(text), since(18)), seen(1, 0));
        assert_eq!(queue_state(&path).unwrap().waiting, Vec::<String>::new());
    }
}

#[test]
fn an_absorbed_remove_alone_is_not_a_take() {
    let text = "move the tab bar";
    let lines = [old(0), enqueue(text, 20, 280), absorbed(text, 20, 983)];
    let (_dir, path) = write(&lines);
    assert_eq!(count_since(&path, &message(text), since(18)), seen(1, 0));
}

#[test]
fn the_new_line_types_change_nothing() {
    let text = "move the tab bar";
    let extras: Vec<String> = [
        json!({"type": "api-request", "timestamp": stamp(20, 300), "sessionId": "s"}),
        json!({"type": "api-request-shape", "timestamp": stamp(20, 301)}),
        json!({"type": "frame-link", "timestamp": stamp(20, 302), "sessionId": "s"}),
        json!({"type": "session-owner", "sessionId": "s"}),
        json!({"type": "atis-latch", "sessionId": "s"}),
        json!({"type": "last-prompt", "lastPrompt": "x", "sessionId": "s"}),
        json!({"type": "custom-title", "customTitle": "t", "sessionId": "s"}),
        json!({"type": "agent-name", "agentName": "a", "sessionId": "s"}),
        json!({"type": "mode", "mode": "normal", "sessionId": "s"}),
        json!({"type": "permission-mode", "permissionMode": "auto", "sessionId": "s"}),
        json!({"type": "tools-used", "sessionId": "s"}),
        json!({"type": "cost-state", "sessionId": "s"}),
    ]
    .iter()
    .map(Value::to_string)
    .collect();
    let mut lines = vec![old(0), enqueue(text, 20, 280)];
    lines.extend(extras.clone());
    lines.push(blob(2_000));
    let (_dir, path) = write(&lines);
    let state = queue_state(&path).unwrap();
    assert_eq!(state.waiting, vec![text.to_string()]);
    assert_eq!(
        state.unknown, 0,
        "new benign line types must not turn the safe rule on"
    );
    assert_eq!(count_since(&path, &message(text), since(18)), seen(1, 0));
    lines.push(absorbed(text, 21, 0));
    lines.extend(extras);
    lines.push(taken(json!(text), "human", 20, 278));
    let (_dir, path) = write(&lines);
    let state = queue_state(&path).unwrap();
    assert_eq!(state.waiting, Vec::<String>::new());
    assert_eq!(state.unknown, 0);
    assert_eq!(count_since(&path, &message(text), since(18)), seen(1, 1));
}

#[test]
fn a_queued_entry_stays_listed_under_big_tool_results() {
    let text = "keep the sidebar narrow";
    let mut lines = vec![old(0), enqueue(text, 20, 0)];
    for second in 21..24 {
        lines.push(tool_result(1_200_000, second, 0));
    }
    let (_dir, path) = write(&lines);
    let state = queue_state(&path).unwrap();
    assert_eq!(state.waiting, vec![text.to_string()]);
    assert_eq!(queue_lists(&path, &message(text)), Some(true));
}

fn big_prompt_line(text: &str, second: u32) -> String {
    let pictured = json!([{"type": "text", "text": text}, image(900_000)]);
    format!(
        r#"{{"parentUuid":"p","isSidechain":false,"type":"user","message":{{"role":"user","content":{pictured}}},"timestamp":"{}","origin":{{"kind":"human"}}}}"#,
        stamp(second, 0)
    )
}

#[test]
fn a_picture_entry_stays_listed_under_blobs_and_leaves_on_its_own_big_line() {
    let text = "check the badge";
    let content = format!("[Image #2]{text}");
    let mut lines = vec![
        old(0),
        enqueue(&content, 20, 0),
        blob(900_000),
        blob(900_000),
    ];
    let (_dir, path) = write(&lines);
    assert_eq!(queue_lists(&path, &picture(text)), Some(true));
    lines.push(big_prompt_line(text, 21));
    lines.push(blob(900_000));
    let (_dir, path) = write(&lines);
    assert_eq!(
        queue_lists(&path, &picture(text)),
        Some(false),
        "the 900 KB typed line that names the entry takes it, however big"
    );
}

#[test]
fn a_big_typed_line_after_a_take_counts_once_with_its_pictures() {
    let text = "check the badge";
    let lines = [
        old(0),
        big_prompt_line(text, 20),
        blob(900_000),
        blob(900_000),
    ];
    let (_dir, path) = write(&lines);
    assert_eq!(count_since(&path, &picture(text), since(18)), seen(0, 1));
    assert_eq!(count_since(&path, &message(text), since(18)), seen(0, 1));
    let mut two = picture(text);
    two.pictures = 2;
    assert_eq!(count_since(&path, &two, since(18)), seen(0, 0));
}

fn raw_user(text: &str, second: u32) -> String {
    let body = serde_json::to_string(text).unwrap();
    format!(
        r#"{{"parentUuid":"p","isSidechain":false,"type":"user","message":{{"role":"user","content":{body}}},"uuid":"u","timestamp":"{}","origin":{{"kind":"human"}}}}"#,
        stamp(second, 0)
    )
}

fn raw_enqueue(content: &str, second: u32, ms: u32) -> String {
    let body = serde_json::to_string(content).unwrap();
    format!(
        r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"{}","sessionId":"s","content":{body}}}"#,
        stamp(second, ms)
    )
}

fn log_of(bytes: usize) -> String {
    let mut text = String::from("tool_use_id toolUseResult pasted by the person\n");
    let mut n = 0;
    while text.len() < bytes {
        text.push_str(&format!("log line {n} of the pasted output\n"));
        n += 1;
    }
    text
}

#[test]
fn a_pasted_log_over_the_line_limit_is_kept_even_when_it_names_tool_results() {
    let text = log_of(140_000);
    let lines = [old(0), raw_user(&text, 20), old(25)];
    let (_dir, path) = write(&lines);
    assert_eq!(count_since(&path, &message(&text), since(18)), seen(0, 1));
}

#[test]
fn a_real_tool_result_over_the_line_limit_is_still_skipped() {
    let result = format!(
        r#"{{"parentUuid":"p","isSidechain":false,"type":"user","message":{{"role":"user","content":[{{"tool_use_id":"t","type":"tool_result","content":"{}"}}]}},"timestamp":"{}","toolUseResult":{{"stdout":"x"}}}}"#,
        "x".repeat(200_000),
        stamp(20, 0)
    );
    let text = "ship the fix";
    let lines = [old(0), result, raw_user(text, 21)];
    let (_dir, path) = write(&lines);
    assert_eq!(count_since(&path, &message(text), since(18)), seen(0, 1));
}

#[test]
fn an_oversized_enqueue_stays_in_the_replay_so_a_dequeue_pops_the_right_entry() {
    let big = log_of(150_000);
    let lines = [
        old(0),
        raw_enqueue(&big, 20, 0),
        raw_enqueue("second entry", 20, 100),
        operation_line("dequeue", 21),
    ];
    let (_dir, path) = write(&lines);
    let state = queue_state(&path).unwrap();
    assert_eq!(
        state.waiting,
        vec!["second entry".to_string()],
        "the dequeue takes the oversized first entry, not the short one"
    );
    assert_eq!(
        count_since(&path, &message(&big), since(18)),
        seen(1, 0),
        "the oversized enqueue counts as an enqueue"
    );
}

fn operation_line(op: &str, second: u32) -> String {
    json!({"type": "queue-operation", "operation": op,
        "timestamp": stamp(second, 0), "sessionId": "s"})
    .to_string()
}

#[test]
fn big_prompt_lines_past_the_budget_leave_the_answer_unsure_not_wrong() {
    let picture = |second: u32| {
        let pictured = json!([{"type": "text", "text": "look"}, image(7_900_000)]);
        format!(
            r#"{{"parentUuid":"p","isSidechain":false,"type":"user","message":{{"role":"user","content":{pictured}}},"timestamp":"{}","origin":{{"kind":"human"}}}}"#,
            stamp(second, 0)
        )
    };
    let lines = [old(0), picture(21), picture(22), picture(23), picture(24)];
    let (_dir, path) = write(&lines);
    assert_eq!(
        count_since(&path, &message("other words"), since(18)),
        None,
        "four 7.9 MB lines do not fit the budget, so the window cannot say it saw everything since the moment"
    );
}

#[test]
fn an_unchanged_file_is_read_once_and_a_changed_one_again() {
    let text = "keep the sidebar narrow";
    let lines = [old(0), enqueue(text, 20, 0)];
    let (_dir, path) = write(&lines);
    let reads = || crate::transcript::window::reads_of(&path);
    assert_eq!(count_since(&path, &message(text), since(18)), seen(1, 0));
    assert_eq!(count_since(&path, &message(text), since(18)), seen(1, 0));
    assert_eq!(queue_lists(&path, &message(text)), Some(true));
    assert_eq!(
        reads(),
        1,
        "the 1 s facts poll and the watcher reuse one read"
    );
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(file, "{}", taken(json!(text), "human", 21, 0)).unwrap();
    drop(file);
    assert_eq!(count_since(&path, &message(text), since(18)), seen(1, 1));
    assert_eq!(reads(), 2);
}
