use crate::cluster_redact::{gone_message, mask_for, mask_tail, JobMasks, MaskedLog, TailMasks};
use notchling_engine::cluster::grant_files::GrantValue;
use std::io::Write;
use std::sync::{Arc, Mutex};

const PEM: &str = "-----BEGIN NL-MARKER-7-----\nNL-MARKER-7-LINE-AAAAAAAA\nNL-MARKER-7-LINE-BBBBBBBB\n-----END NL-MARKER-7-----";
const TOKEN: &str = "NL-MARKER-3-TOKEN-0123456789";

fn grant(name: &str, value: &str) -> (String, GrantValue) {
    (name.to_string(), GrantValue::new(value.to_string()))
}

fn masks_with(job: &str, values: Vec<(String, GrantValue)>) -> TailMasks {
    let masks = TailMasks::new();
    masks.add(job, values).unwrap();
    masks
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

fn read(
    masks: &TailMasks,
    job: &str,
    data: &[u8],
    from: usize,
    to: usize,
    at_end: bool,
) -> (String, u64) {
    let (out, next) = mask_tail(
        masks,
        job,
        from as u64,
        &data[..from],
        &data[from..to],
        at_end,
    )
    .unwrap();
    (text(out), next)
}

fn whole(masks: &TailMasks, job: &str, data: &[u8]) -> String {
    read(masks, job, data, 0, data.len(), true).0
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

fn logged(sink: &Arc<Mutex<Vec<u8>>>) -> String {
    text(sink.lock().unwrap().clone())
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
    let masks = masks_with("j1", vec![grant("api-token", TOKEN)]);
    let data = format!("before {TOKEN} after\n").into_bytes();
    let (first, next) = read(&masks, "j1", &data, 0, 22, false);
    assert!(!first.contains("NL-MARKER-3"), "partial leaked");
    assert!(next == 7, "held bytes are re-read");
    let (second, end) = read(&masks, "j1", &data, next as usize, data.len(), true);
    assert!(end == data.len() as u64, "ends at the file end");
    assert!(
        format!("{first}{second}") == "before [secret api-token] after\n",
        "split value was not masked"
    );
}

#[test]
fn a_retried_call_gives_the_same_output() {
    let masks = masks_with("j1", vec![grant("api-token", TOKEN)]);
    let data = format!("x {TOKEN}").into_bytes();
    let part = &data[..24];
    let one = mask_tail(&masks, "j1", 40, b"", part, false).unwrap();
    let two = mask_tail(&masks, "j1", 40, b"", part, false).unwrap();
    assert!(one == two, "retry changed the answer");
    assert!(one.1 == 42, "next is the start of the held bytes");
}

#[test]
fn a_read_starting_inside_a_value_still_masks_it() {
    let masks = masks_with("j1", vec![grant("api-token", TOKEN)]);
    let data = format!("pre {TOKEN} post\n").into_bytes();
    let (out, _) = read(&masks, "j1", &data, 5, data.len(), true);
    assert!(!out.contains("ARKER"), "the rest of the value leaked");
    assert!(out == "[secret api-token] post\n", "unexpected output");
    let (blind, _) = mask_tail(&masks, "j1", 5, b"", &data[5..], true).unwrap();
    assert!(text(blind).contains("ARKER"), "the lead made no difference");
}

#[test]
fn a_read_starting_mid_line_of_a_multiline_value_masks_the_line() {
    let masks = masks_with("j1", vec![grant("deploy-key", PEM)]);
    let data = b"x\nNL-MARKER-7-LINE-AAAAAAAA\ny\n";
    let (out, _) = read(&masks, "j1", data, 12, data.len(), true);
    assert!(!out.contains("AAAAAAAA"), "the rest of the line leaked");
    assert!(out.contains("[secret deploy-key]"), "mask missing");
    assert!(out.ends_with("\ny\n"), "text after the line was lost");
}

#[test]
fn a_grant_added_between_calls_masks_the_reread_bytes() {
    let masks = masks_with("j1", vec![grant("one", "NL-MARKER-5-ONE-AAAAAAAA")]);
    let data = b"x NL-MARKER-5-TWO-BBBBBBBB y";
    let (first, next) = read(&masks, "j1", data, 0, 14, false);
    assert!(first == "x ", "held bytes were emitted");
    masks
        .add("j1", vec![grant("two", "NL-MARKER-5-TWO-BBBBBBBB")])
        .unwrap();
    let (second, _) = read(&masks, "j1", data, next as usize, data.len(), true);
    assert!(second == "[secret two] y", "the added value was not masked");
}

#[test]
fn adding_never_unmasks_and_forget_job_clears() {
    let masks = masks_with("j1", vec![grant("a", "NL-MARKER-12-AAAAAAAA")]);
    masks
        .add("j1", vec![grant("b", "NL-MARKER-12-BBBBBBBB")])
        .unwrap();
    masks.add("j1", Vec::new()).unwrap();
    let out = whole(
        &masks,
        "j1",
        b"NL-MARKER-12-AAAAAAAA and NL-MARKER-12-BBBBBBBB",
    );
    assert!(!out.contains("NL-MARKER-12"), "a value was unmasked");
    assert!(out.contains("[secret a]") && out.contains("[secret b]"));
    masks
        .add("j1", vec![grant("b", "NL-MARKER-12-BBBBBBBB")])
        .unwrap();
    assert!(
        format!("{masks:?}").contains("2 values"),
        "duplicate stored"
    );
    masks.forget_job("j1");
    assert!(masks.for_job("j1").is_none());
    assert!(masks.generation("j1").is_none());
    assert!(masks.state("j1") == JobMasks::Never);
}

#[test]
fn masks_outlive_the_job_until_it_is_forgotten() {
    let masks = masks_with("j1", vec![grant("api-token", TOKEN)]);
    masks.end_job("j1");
    assert!(masks.is_ended("j1"));
    assert!(masks.state("j1") == JobMasks::Masked);
    let out = whole(&masks, "j1", format!("old {TOKEN}\n").as_bytes());
    assert!(!out.contains("NL-MARKER-3"), "an ended job leaked");
    assert!(out.contains("[secret api-token]"), "mask missing");
}

#[test]
fn a_job_known_to_have_had_grants_is_not_served_without_masks() {
    let masks = TailMasks::new();
    masks.mark_granted("j1");
    assert!(masks.state("j1") == JobMasks::Gone);
    let refused = mask_tail(&masks, "j1", 0, b"", b"anything", true);
    assert!(refused == Err(gone_message("j1")));
    assert!(gone_message("j1") == "the masks for job j1 are gone; its output isn't served");

    masks.add("j1", vec![grant("api-token", TOKEN)]).unwrap();
    assert!(masks.state("j1") == JobMasks::Masked);
    masks.mark_granted("j1");
    assert!(masks.state("j1") == JobMasks::Masked);

    masks.forget_job("j1");
    masks.mark_granted("j1");
    assert!(mask_tail(&masks, "j1", 0, b"", b"x", false).is_err());
    masks.forget_job("j1");
    assert!(mask_tail(&masks, "j1", 0, b"", b"x", false).is_ok());
}

#[test]
fn a_short_held_prefix_at_the_end_is_masked_and_a_later_read_cannot_rebuild_it() {
    let masks = masks_with("j1", vec![grant("api-token", TOKEN)]);
    let data = format!("x NL-MAR{}", &TOKEN[6..]).into_bytes();
    let (out, next) = read(&masks, "j1", &data, 0, 8, true);
    assert!(out == "x [secret api-token]", "short prefix was not masked");
    let (rest, _) = read(&masks, "j1", &data, next as usize, data.len(), true);
    assert!(!rest.contains("KER-3"), "the rest of the value leaked");
    assert!(rest == "[secret api-token]", "straddling value not masked");
}

#[test]
fn a_periodic_chunk_makes_progress() {
    let masks = masks_with("j1", vec![grant("pat", "abababab")]);
    let chunk = "ab".repeat(100).into_bytes();
    let (out, next) = read(&masks, "j1", &chunk, 0, chunk.len(), false);
    assert!(next == 200, "the read did not advance");
    assert!(!out.contains("abab"), "the pattern leaked");
}

#[test]
fn values_over_the_cap_are_refused_and_nothing_is_added() {
    let masks = TailMasks::new();
    let big = "q".repeat(32 * 1024 + 1);
    let refused = masks.add("j1", vec![grant("fits", TOKEN), grant("big", &big)]);
    assert!(refused.is_err());
    assert!(masks.state("j1") == JobMasks::Never);
    let exact = "q".repeat(32 * 1024);
    assert!(masks.add("j1", vec![grant("exact", &exact)]).is_ok());
    assert!(masks.state("j1") == JobMasks::Masked);
}

#[test]
fn job_without_grants_is_served_as_is() {
    let masks = TailMasks::new();
    assert!(masks.for_job("j1").is_none());
    let raw: Vec<u8> = vec![0, 159, 146, 150, b'N', b'L', b'\n', 255];
    let (out, next) = mask_tail(&masks, "j1", 10, b"lead", &raw, false).unwrap();
    assert!(out == raw, "bytes changed");
    assert!(next == 18, "next skipped or repeated bytes");
    let (out, next) = mask_tail(&masks, "j1", 10, b"", &raw, true).unwrap();
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
    assert!(!logged(&sink).contains("NL-MARKER"));
    log.write_all(b"EN-9876 done NL-MARK").unwrap();
    log.close().unwrap();
    let written = logged(&sink);
    assert!(!written.contains("NL-MARKER-7"), "pem leaked");
    assert!(!written.contains("NL-MARKER-6"), "token leaked");
    assert!(!written.contains("NL-MARK"), "a held prefix leaked");
    assert!(written.contains("[secret pem]"), "pem mask missing");
    assert!(
        written.contains("[secret tok] done [secret "),
        "token mask missing"
    );
}

#[test]
fn a_log_started_before_any_grant_masks_once_one_arrives() {
    let masks = Arc::new(TailMasks::new());
    let sink = Arc::new(Mutex::new(Vec::<u8>::new()));
    let mut log = MaskedLog::new(Shared(sink.clone()), masks.clone(), "j1");
    log.write_all(b"plain line\n").unwrap();
    masks
        .add("j1", vec![grant("tok", "NL-MARKER-6-TOKEN-9876")])
        .unwrap();
    log.write_all(b"got NL-MARKER-6-TOK").unwrap();
    log.write_all(b"EN-9876 ok\n").unwrap();
    log.close().unwrap();
    let written = logged(&sink);
    assert!(!written.contains("NL-MARKER-6"), "token leaked");
    assert!(written.contains("plain line"), "plain text lost");
}

#[test]
fn a_log_for_a_job_that_lost_its_masks_refuses_to_write() {
    let masks = Arc::new(TailMasks::new());
    masks.mark_granted("j1");
    let sink = Arc::new(Mutex::new(Vec::<u8>::new()));
    let mut log = MaskedLog::new(Shared(sink.clone()), masks, "j1");
    assert!(log.write_all(b"NL-MARKER-6-TOKEN-9876\n").is_err());
    assert!(logged(&sink).is_empty(), "bytes were written without masks");
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
    assert!(!logged(&sink).contains("NL-MARKER-6"), "token after close");
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
        "held bytes early"
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
