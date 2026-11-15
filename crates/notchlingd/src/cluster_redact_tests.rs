use crate::cluster_redact::{mask_for, MaskedLog, TailFilters, TailMasks};
use notchling_engine::cluster::grant_files::GrantValue;
use std::io::Write;
use std::sync::{Arc, Mutex};

const PEM: &str = "-----BEGIN NL-MARKER-7-----\nNL-MARKER-7-LINE-AAAAAAAA\nNL-MARKER-7-LINE-BBBBBBBB\n-----END NL-MARKER-7-----";

fn grant(name: &str, value: &str) -> (String, GrantValue) {
    (name.to_string(), GrantValue::new(value.to_string()))
}

fn masks_with(job: &str, values: Vec<(String, GrantValue)>) -> TailMasks {
    let masks = TailMasks::new();
    masks.set(job, values);
    masks
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn tail_masks_json_escaped_multiline_value() {
    let masks = masks_with("j1", vec![grant("deploy-key", PEM)]);
    let filters = TailFilters::new();
    let line = format!(
        "{{\"type\":\"assistant\",\"text\":{}}}\n",
        serde_json::to_string(PEM).unwrap()
    );
    let mut out = filters.filter(&masks, "m2", "t.jsonl", "j1", line.as_bytes());
    out.extend(filters.finish("m2", "t.jsonl", "j1"));
    let out = text(out);
    assert!(!out.contains("NL-MARKER-7"));
    assert!(!out.contains("-----BEGIN"));
    assert!(out.contains("[secret deploy-key]"));
    assert!(out.contains("\"type\":\"assistant\""));
}

#[test]
fn single_marker_lines_are_masked_alone() {
    let masks = masks_with("j1", vec![grant("deploy-key", PEM)]);
    let filters = TailFilters::new();
    let mut out = filters.filter(
        &masks,
        "m2",
        "log",
        "j1",
        b"cat said NL-MARKER-7-LINE-BBBBBBBB and \"NL-MARKER-7-LINE-AAAAAAAA\"\n",
    );
    out.extend(filters.finish("m2", "log", "j1"));
    let out = text(out);
    assert!(!out.contains("NL-MARKER-7"));
    assert!(out.contains("[secret deploy-key]"));
}

#[test]
fn mask_holds_across_chunk_boundaries() {
    let masks = masks_with(
        "j1",
        vec![grant("api-token", "NL-MARKER-3-TOKEN-0123456789")],
    );
    let filters = TailFilters::new();
    let first = filters.filter(&masks, "m2", "log", "j1", b"before NL-MARKER-3-TOK");
    assert!(!text(first.clone()).contains("NL-MARKER-3"));
    let second = filters.filter(&masks, "m2", "log", "j1", b"EN-0123456789 after\n");
    let mut all = first;
    all.extend(second);
    all.extend(filters.finish("m2", "log", "j1"));
    let all = text(all);
    assert!(!all.contains("NL-MARKER-3"));
    assert_eq!(all, "before [secret api-token] after\n");
}

#[test]
fn streams_for_different_files_do_not_share_held_bytes() {
    let masks = masks_with(
        "j1",
        vec![grant("api-token", "NL-MARKER-4-TOKEN-0123456789")],
    );
    let filters = TailFilters::new();
    let a = filters.filter(&masks, "m2", "a", "j1", b"NL-MARKER-4-TOK");
    let b = filters.filter(&masks, "m2", "b", "j1", b"EN-0123456789");
    assert!(text(b).contains("EN-0123456789"));
    assert!(!text(a).contains("NL-MARKER-4"));
    assert_eq!(filters.count(), 2);
}

#[test]
fn a_grant_added_mid_stream_masks_from_then_on_and_keeps_held_bytes() {
    let masks = masks_with("j1", vec![grant("one", "NL-MARKER-5-ONE-AAAAAAAA")]);
    let filters = TailFilters::new();
    let mut out = filters.filter(&masks, "m2", "log", "j1", b"x NL-MARKER-5-ONE-AAA");
    masks.set(
        "j1",
        vec![
            grant("one", "NL-MARKER-5-ONE-AAAAAAAA"),
            grant("two", "NL-MARKER-5-TWO-BBBBBBBB"),
        ],
    );
    out.extend(filters.filter(
        &masks,
        "m2",
        "log",
        "j1",
        b"AAAAA y NL-MARKER-5-TWO-BBBBBBBB z",
    ));
    out.extend(filters.finish("m2", "log", "j1"));
    assert_eq!(text(out), "x [secret one] y [secret two] z");
}

#[test]
fn shell_log_is_masked_at_write() {
    let masks = Arc::new(masks_with(
        "j1",
        vec![grant("pem", PEM), grant("tok", "NL-MARKER-6-TOKEN-9876")],
    ));
    let sink = Arc::new(Mutex::new(Vec::<u8>::new()));

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

    let mut log = MaskedLog::new(Shared(sink.clone()), masks, "j1");
    log.write_all(PEM.as_bytes()).unwrap();
    log.write_all(b"\nthen NL-MARKER-6-TOK").unwrap();
    assert!(!text(sink.lock().unwrap().clone()).contains("NL-MARKER"));
    log.write_all(b"EN-9876 done NL-MARK").unwrap();
    log.close().unwrap();
    let written = text(sink.lock().unwrap().clone());
    assert!(!written.contains("NL-MARKER-7"));
    assert!(!written.contains("NL-MARKER-6"));
    assert!(written.contains("[secret pem]"));
    assert!(written.contains("[secret tok] done NL-MARK"));
}

#[test]
fn closing_the_log_emits_the_held_tail_once() {
    let masks = Arc::new(masks_with(
        "j1",
        vec![grant("tok", "NL-MARKER-6-TOKEN-9876")],
    ));
    let mut log = MaskedLog::new(Vec::<u8>::new(), masks, "j1");
    log.write_all(b"ends with NL-MARKER-6-TO").unwrap();
    assert_eq!(text(log.get_ref().clone()), "ends with ");
    log.close().unwrap();
    log.close().unwrap();
    assert_eq!(text(log.get_ref().clone()), "ends with NL-MARKER-6-TO");
}

#[test]
fn job_without_grants_is_served_as_is() {
    let masks = TailMasks::new();
    let filters = TailFilters::new();
    assert!(masks.for_job("j1").is_none());
    let raw: Vec<u8> = vec![0, 159, 146, 150, b'N', b'L', b'\n', 255];
    let out = filters.filter(&masks, "m2", "log", "j1", &raw);
    assert_eq!(out, raw);
    assert_eq!(filters.count(), 0);
    assert!(filters.finish("m2", "log", "j1").is_empty());

    let mut log = MaskedLog::new(Vec::<u8>::new(), Arc::new(TailMasks::new()), "j1");
    log.write_all(&raw).unwrap();
    log.close().unwrap();
    assert_eq!(log.get_ref(), &raw);
}

#[test]
fn drop_job_makes_for_job_none() {
    let masks = masks_with("j1", vec![grant("tok", "NL-MARKER-6-TOKEN-9876")]);
    assert!(masks.for_job("j1").is_some());
    assert!(masks.for_job("j2").is_none());
    masks.drop_job("j1");
    assert!(masks.for_job("j1").is_none());
    assert!(masks.generation("j1").is_none());
}

#[test]
fn an_empty_grant_list_clears_the_job() {
    let masks = masks_with("j1", vec![grant("tok", "NL-MARKER-6-TOKEN-9876")]);
    masks.set("j1", Vec::new());
    assert!(masks.for_job("j1").is_none());
}

#[test]
fn for_job_gives_a_fresh_redactor_each_time() {
    let masks = masks_with("j1", vec![grant("tok", "NL-MARKER-6-TOKEN-9876")]);
    let mut a = masks.for_job("j1").unwrap();
    assert!(a.feed(b"NL-MARKER-6-TO").is_empty());
    let mut b = masks.for_job("j1").unwrap();
    assert_eq!(b.feed(b"plain"), b"plain");
}

#[test]
fn filters_drop_with_their_job() {
    let masks = masks_with("j1", vec![grant("tok", "NL-MARKER-6-TOKEN-9876")]);
    let filters = TailFilters::new();
    filters.filter(&masks, "m2", "a", "j1", b"x");
    filters.filter(&masks, "m3", "b", "j1", b"y");
    assert_eq!(filters.count(), 2);
    filters.drop_job("j1");
    assert_eq!(filters.count(), 0);
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
    assert_eq!(mask_for("tok"), "[secret tok]");
}
