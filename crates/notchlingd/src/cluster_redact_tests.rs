use crate::cluster_redact::{mask_for, mask_tail, MaskedLog, TailMasks};
use notchling_engine::cluster::grant_files::GrantValue;
use std::io::Write;
use std::sync::{Arc, Mutex};

const PEM: &str = "-----BEGIN NL-MARKER-7-----\nNL-MARKER-7-LINE-AAAAAAAA\nNL-MARKER-7-LINE-BBBBBBBB\n-----END NL-MARKER-7-----";

fn grant(name: &str, value: &str) -> (String, GrantValue) {
    (name.to_string(), GrantValue::new(value.to_string()))
}

fn masks_with(job: &str, values: Vec<(String, GrantValue)>) -> TailMasks {
    let masks = TailMasks::new();
    masks.add(job, values);
    masks
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

fn whole(masks: &TailMasks, job: &str, data: &[u8]) -> String {
    text(mask_tail(masks, job, 0, data, true).0)
}

struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn tail_masks_json_escaped_multiline_value() {
    let masks = masks_with("j1", vec![grant("deploy-key", PEM)]);
    let line = format!(
        "{{\"type\":\"assistant\",\"text\":{}}}\n",
        serde_json::to_string(PEM).unwrap()
    );
    let out = whole(&masks, "j1", line.as_bytes());
    assert!(!out.contains("NL-MARKER-7"), "marker leaked");
    assert!(!out.contains("-----BEGIN"), "begin line leaked");
    assert!(out.contains("[secret deploy-key]"), "mask missing");
    assert!(out.contains("\"type\":\"assistant\""), "frame lost");
}

#[test]
fn single_marker_lines_are_masked_alone() {
    let masks = masks_with("j1", vec![grant("deploy-key", PEM)]);
    let out = whole(
        &masks,
        "j1",
        b"cat said NL-MARKER-7-LINE-BBBBBBBB and \"NL-MARKER-7-LINE-AAAAAAAA\"\n",
    );
    assert!(!out.contains("NL-MARKER-7"), "marker leaked");
    assert!(out.contains("[secret deploy-key]"), "mask missing");
}

#[test]
fn html_and_double_escaped_forms_are_masked() {
    let value = "NL-MARKER-11<&>\"q\"\\z/path";
    let masks = masks_with("j1", vec![grant("odd", value)]);
    let json = serde_json::to_string(value).unwrap();
    let inner = &json[1..json.len() - 1];
    let go = inner
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    let doubled = serde_json::to_string(inner).unwrap();
    let slashed = inner.replace('/', "\\/");
    for form in [value.to_string(), inner.to_string(), go, doubled, slashed] {
        let out = whole(&masks, "j1", format!("a {form} b\n").as_bytes());
        assert!(!out.contains("NL-MARKER-11"), "a form leaked");
        assert!(out.contains("[secret odd]"), "mask missing");
    }
}

#[test]
fn mask_holds_across_chunk_boundaries() {
    let masks = masks_with(
        "j1",
        vec![grant("api-token", "NL-MARKER-3-TOKEN-0123456789")],
    );
    let data = b"before NL-MARKER-3-TOKEN-0123456789 after\n";
    let (first, next) = mask_tail(&masks, "j1", 0, &data[..22], false);
    assert!(
        !text(first.clone()).contains("NL-MARKER-3"),
        "partial leaked"
    );
    assert!(next == 7, "held bytes are re-read");
    let (second, end) = mask_tail(&masks, "j1", next, &data[next as usize..], true);
    assert!(end == data.len() as u64, "ends at the file end");
    let mut all = first;
    all.extend(second);
    let all = text(all);
    assert!(
        all == "before [secret api-token] after\n",
        "split value was not masked"
    );
}

#[test]
fn a_retried_call_gives_the_same_output() {
    let masks = masks_with(
        "j1",
        vec![grant("api-token", "NL-MARKER-3-TOKEN-0123456789")],
    );
    let data = b"x NL-MARKER-3-TOKEN-01";
    let one = mask_tail(&masks, "j1", 40, data, false);
    let two = mask_tail(&masks, "j1", 40, data, false);
    assert!(one == two, "retry changed the answer");
    assert!(one.1 == 42, "next is the start of the held bytes");
}

#[test]
fn a_grant_added_between_calls_masks_the_reread_bytes() {
    let masks = masks_with("j1", vec![grant("one", "NL-MARKER-5-ONE-AAAAAAAA")]);
    let data = b"x NL-MARKER-5-TWO-BBBBBBBB y";
    let (first, next) = mask_tail(&masks, "j1", 0, &data[..14], false);
    assert!(text(first).as_str() == "x ", "held bytes were emitted");
    masks.add("j1", vec![grant("two", "NL-MARKER-5-TWO-BBBBBBBB")]);
    let (second, _) = mask_tail(&masks, "j1", next, &data[next as usize..], true);
    assert!(
        text(second) == "[secret two] y",
        "the added value was not masked"
    );
}

#[test]
fn adding_never_unmasks_and_drop_job_clears() {
    let masks = masks_with("j1", vec![grant("a", "NL-MARKER-12-AAAAAAAA")]);
    masks.add("j1", vec![grant("b", "NL-MARKER-12-BBBBBBBB")]);
    masks.add("j1", Vec::new());
    let out = whole(
        &masks,
        "j1",
        b"NL-MARKER-12-AAAAAAAA and NL-MARKER-12-BBBBBBBB",
    );
    assert!(!out.contains("NL-MARKER-12"), "a value was unmasked");
    assert!(out.contains("[secret a]") && out.contains("[secret b]"));
    masks.add("j1", vec![grant("b", "NL-MARKER-12-BBBBBBBB")]);
    assert!(
        format!("{masks:?}").contains("2 values"),
        "duplicate stored"
    );
    masks.drop_job("j1");
    assert!(masks.for_job("j1").is_none());
    assert!(masks.generation("j1").is_none());
}

#[test]
fn at_the_end_a_held_prefix_of_eight_bytes_or_more_is_masked() {
    let masks = masks_with(
        "j1",
        vec![grant("api-token", "NL-MARKER-3-TOKEN-0123456789")],
    );
    let long = whole(&masks, "j1", b"tail NL-MARKER-3-TOKE");
    assert!(!long.contains("NL-MARKER-3"), "long prefix leaked");
    assert!(long.contains("[secret api-token]"), "mask missing");
    let short = whole(&masks, "j1", b"tail NL-MAR");
    assert!(short == "tail NL-MAR", "a short prefix changed");
}

#[test]
fn job_without_grants_is_served_as_is() {
    let masks = TailMasks::new();
    assert!(masks.for_job("j1").is_none());
    let raw: Vec<u8> = vec![0, 159, 146, 150, b'N', b'L', b'\n', 255];
    let (out, next) = mask_tail(&masks, "j1", 10, &raw, false);
    assert!(out == raw, "bytes changed");
    assert!(next == 18, "next skipped or repeated bytes");
    let (out, next) = mask_tail(&masks, "j1", 10, &raw, true);
    assert!(out == raw && next == 18, "end of file changed bytes");

    let sink = Arc::new(Mutex::new(Vec::new()));
    let mut log = MaskedLog::new(Shared(sink.clone()), Arc::new(TailMasks::new()), "j1");
    log.write_all(&raw).unwrap();
    log.close().unwrap();
    assert!(*sink.lock().unwrap() == raw, "log bytes changed");
}

#[test]
fn shell_log_is_masked_at_write() {
    let masks = Arc::new(masks_with(
        "j1",
        vec![grant("pem", PEM), grant("tok", "NL-MARKER-6-TOKEN-9876")],
    ));
    let sink = Arc::new(Mutex::new(Vec::<u8>::new()));
    let mut log = MaskedLog::new(Shared(sink.clone()), masks, "j1");
    log.write_all(PEM.as_bytes()).unwrap();
    log.write_all(b"\nthen NL-MARKER-6-TOK").unwrap();
    assert!(!text(sink.lock().unwrap().clone()).contains("NL-MARKER"));
    log.write_all(b"EN-9876 done NL-MARK").unwrap();
    log.close().unwrap();
    let written = text(sink.lock().unwrap().clone());
    assert!(!written.contains("NL-MARKER-7"), "pem leaked");
    assert!(!written.contains("NL-MARKER-6"), "token leaked");
    assert!(written.contains("[secret pem]"), "pem mask missing");
    assert!(
        written.contains("[secret tok] done NL-MARK"),
        "token mask missing"
    );
}

#[test]
fn a_log_started_before_any_grant_masks_once_one_arrives() {
    let masks = Arc::new(TailMasks::new());
    let sink = Arc::new(Mutex::new(Vec::<u8>::new()));
    let mut log = MaskedLog::new(Shared(sink.clone()), masks.clone(), "j1");
    log.write_all(b"plain line\n").unwrap();
    masks.add("j1", vec![grant("tok", "NL-MARKER-6-TOKEN-9876")]);
    log.write_all(b"got NL-MARKER-6-TOK").unwrap();
    log.write_all(b"EN-9876 ok\n").unwrap();
    log.close().unwrap();
    let written = text(sink.lock().unwrap().clone());
    assert!(!written.contains("NL-MARKER-6"), "token leaked");
    assert!(written.contains("plain line"), "plain text lost");
}

#[test]
fn writing_after_close_is_an_error_and_writes_nothing() {
    let masks = Arc::new(masks_with(
        "j1",
        vec![grant("tok", "NL-MARKER-6-TOKEN-9876")],
    ));
    let sink = Arc::new(Mutex::new(Vec::<u8>::new()));
    let mut log = MaskedLog::new(Shared(sink.clone()), masks, "j1");
    log.write_all(b"one\n").unwrap();
    log.close().unwrap();
    assert!(log.write_all(b"NL-MARKER-6-TOKEN-9876\n").is_err());
    let written = text(sink.lock().unwrap().clone());
    assert!(
        !written.contains("NL-MARKER-6"),
        "token written after close"
    );
}

#[test]
fn closing_the_log_masks_a_held_prefix_once() {
    let masks = Arc::new(masks_with(
        "j1",
        vec![grant("tok", "NL-MARKER-6-TOKEN-9876")],
    ));
    let mut log = MaskedLog::new(Vec::<u8>::new(), masks, "j1");
    log.write_all(b"ends with NL-MARKER-6-TO").unwrap();
    assert!(
        text(log.get_ref().clone()) == "ends with ",
        "held bytes written early"
    );
    log.close().unwrap();
    log.close().unwrap();
    assert!(
        text(log.get_ref().clone()) == "ends with [secret tok]",
        "held prefix not masked once"
    );
}

#[test]
fn for_job_gives_a_fresh_redactor_each_time() {
    let masks = masks_with("j1", vec![grant("tok", "NL-MARKER-6-TOKEN-9876")]);
    let mut a = masks.for_job("j1").unwrap();
    assert!(a.feed(b"NL-MARKER-6-TO").is_empty());
    let mut b = masks.for_job("j1").unwrap();
    assert!(b.feed(b"plain") == b"plain");
}

#[test]
fn debug_lists_jobs_and_counts_only() {
    let masks = masks_with(
        "j1",
        vec![grant("tok", "NL-MARKER-6-TOKEN-9876"), grant("pem", PEM)],
    );
    let shown = format!("{masks:?}");
    assert!(shown.contains("j1"));
    assert!(shown.contains("2 values"));
    assert!(!shown.contains("NL-MARKER"));
    assert!(!format!("{:?}", grant("tok", "NL-MARKER-6-TOKEN-9876")).contains("NL-MARKER"));
    assert!(mask_for("tok") == "[secret tok]");
}
