use super::jsonl_view::{lines_before, summarize, tail};
use crate::testutil::Scratch;

#[test]
fn a_claude_line_shows_its_type_and_the_start_of_the_text() {
    let s =
        summarize(r#"{"type":"user","message":{"role":"user","content":"Fix the\nlogin bug"}}"#);
    assert_eq!(s.tag, "user");
    assert_eq!(s.preview, "Fix the login bug");
    let s = summarize(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"On it."}]}}"#,
    );
    assert_eq!(s.tag, "assistant");
    assert_eq!(s.preview, "On it.");
}

#[test]
fn the_role_stands_in_for_a_missing_type() {
    let s = summarize(r#"{"role":"tool","content":"ok"}"#);
    assert_eq!(s.tag, "tool");
    assert_eq!(s.preview, "ok");
}

#[test]
fn a_line_without_known_fields_previews_its_json_and_a_long_one_is_cut() {
    let s = summarize(r#"{"x":1}"#);
    assert_eq!(s.tag, "");
    assert_eq!(s.preview, r#"{"x":1}"#);
    let long = format!(r#"{{"type":"note","text":"{}"}}"#, "é".repeat(300));
    let s = summarize(&long);
    assert_eq!(s.tag, "note");
    assert!(s.preview.chars().count() <= 121);
    assert!(s.preview.ends_with('…'));
}

#[test]
fn a_line_that_is_not_json_is_marked() {
    let s = summarize("plain words");
    assert_eq!(s.tag, "invalid");
    assert_eq!(s.preview, "plain words");
}

#[test]
fn the_tail_is_the_last_lines_and_pages_walk_back_to_the_start() {
    let s = Scratch::new("jsonl-tail");
    let body: String = (1..=10).map(|i| format!("{{\"n\":{i}}}\n")).collect();
    let path = s.write("t.jsonl", &body);
    let len = body.len() as u64;
    let last = tail(&path, len, 4, 1 << 20).unwrap();
    assert_eq!(
        last.lines,
        ["{\"n\":7}", "{\"n\":8}", "{\"n\":9}", "{\"n\":10}"]
    );
    assert_eq!(lines_before(&path, last.start), 6);
    let before = tail(&path, last.start, 4, 1 << 20).unwrap();
    assert_eq!(
        before.lines,
        ["{\"n\":3}", "{\"n\":4}", "{\"n\":5}", "{\"n\":6}"]
    );
    let first = tail(&path, before.start, 4, 1 << 20).unwrap();
    assert_eq!(first.start, 0);
    assert_eq!(first.lines, ["{\"n\":1}", "{\"n\":2}"]);
}

#[test]
fn a_byte_budget_drops_the_cut_line_and_a_missing_last_newline_is_fine() {
    let s = Scratch::new("jsonl-bytes");
    let path = s.write("t.jsonl", "{\"a\":1}\n{\"a\":2}\n{\"a\":3}");
    let len = std::fs::metadata(&path).unwrap().len();
    let page = tail(&path, len, 100, 12).unwrap();
    assert_eq!(page.lines, ["{\"a\":3}"]);
    assert_eq!(page.start, 16);
    assert!(tail(&s.path().join("gone.jsonl"), 5, 4, 100).is_none());
}

fn jsonl_with_long_line(name: &str, line: usize, after: &[&str]) -> (Scratch, std::path::PathBuf) {
    let s = Scratch::new(name);
    let mut text = String::from("{\"n\":1}\n{\"n\":2}\n");
    text.push_str(&format!(
        "{{\"type\":\"big\",\"text\":\"{}\"}}\n",
        "x".repeat(line)
    ));
    for l in after {
        text.push_str(l);
        text.push('\n');
    }
    let path = s.write("big.jsonl", &text);
    (s, path)
}

fn numbers(j: &super::jsonl_view::Jsonl) -> Vec<usize> {
    j.rows.iter().map(|r| r.number).collect()
}

#[test]
fn a_last_line_longer_than_the_page_shows_as_one_cut_row_with_its_number() {
    let (_s, path) = jsonl_with_long_line("jl-last", 3 * 1024 * 1024, &[]);
    let j = super::jsonl_view::open(&path).unwrap();
    assert_eq!(numbers(&j), vec![3]);
    assert!(j.rows[0].cut);
    assert_eq!(j.rows[0].summary.tag, "big");
    assert!(j.more());
}

#[test]
fn load_more_steps_past_a_long_line_without_repeating_numbers() {
    let (_s, path) = jsonl_with_long_line("jl-step", 3 * 1024 * 1024, &["{\"n\":4}", "{\"n\":5}"]);
    let j = super::jsonl_view::open(&path).unwrap();
    assert_eq!(numbers(&j), vec![4, 5]);
    let j = super::jsonl_view::earlier(&path, &j).unwrap();
    assert_eq!(numbers(&j), vec![3, 4, 5]);
    assert!(j.rows[0].cut);
    assert_eq!(j.rows[0].summary.tag, "big");
    let j = super::jsonl_view::earlier(&path, &j).unwrap();
    assert_eq!(numbers(&j), vec![1, 2, 3, 4, 5]);
    assert!(!j.more());
}

#[test]
fn a_long_line_between_short_ones_is_reached_by_load_more() {
    let s = Scratch::new("jl-mid");
    let mut text = String::new();
    for n in 1..=3 {
        text.push_str(&format!("{{\"n\":{n}}}\n"));
    }
    text.push_str(&format!(
        "{{\"text\":\"{}\"}}\n",
        "y".repeat(3 * 1024 * 1024)
    ));
    text.push_str("{\"n\":5}\n");
    let path = s.write("mid.jsonl", &text);
    let j = super::jsonl_view::open(&path).unwrap();
    assert_eq!(numbers(&j), vec![5]);
    let j = super::jsonl_view::earlier(&path, &j).unwrap();
    assert_eq!(numbers(&j), vec![4, 5]);
    assert!(j.rows[0].cut);
    let j = super::jsonl_view::earlier(&path, &j).unwrap();
    assert_eq!(numbers(&j), vec![1, 2, 3, 4, 5]);
}

#[test]
fn a_trailing_run_of_blank_lines_does_not_empty_the_view() {
    let s = Scratch::new("jl-blank");
    let mut text = String::from("{\"n\":1}\n{\"n\":2}\n");
    text.push_str(&"\n".repeat(5000));
    let path = s.write("blank.jsonl", &text);
    let j = super::jsonl_view::open(&path).unwrap();
    assert_eq!(numbers(&j), vec![1, 2]);
}

#[test]
fn a_cut_line_with_a_multibyte_character_at_the_tag_limit_does_not_panic() {
    let s = Scratch::new("jl-multibyte");
    let line = format!(
        "{}é{}",
        "x".repeat(4095),
        "y".repeat(super::jsonl_view::MAX_LINE + 10)
    );
    let path = s.write("m.jsonl", &format!("{line}\n"));
    let j = super::jsonl_view::open(&path).unwrap();
    assert_eq!(j.rows.len(), 1);
    assert!(j.rows[0].cut);
}

#[test]
fn stepping_back_into_a_long_blank_run_still_reaches_the_lines_before_it() {
    let s = Scratch::new("jl-blank-step");
    let mut text = String::from("{\"n\":1}\n{\"n\":2}\n");
    text.push_str(&"\n".repeat(500));
    text.push_str("{\"n\":3}\n{\"n\":4}\n");
    let path = s.write("blank.jsonl", &text);
    let j = super::jsonl_view::open(&path).unwrap();
    assert_eq!(numbers(&j), vec![503, 504]);
    let j = super::jsonl_view::earlier(&path, &j).unwrap();
    assert_eq!(numbers(&j), vec![1, 2, 503, 504]);
}

#[test]
fn a_line_longer_than_the_scan_cap_shows_as_one_cut_row_without_a_number() {
    let s = Scratch::new("jl-capped");
    let mut text = String::from("{\"n\":1}\n");
    text.push_str(&"x".repeat(super::jsonl_view::SCAN_CAP as usize + 4 * 1024 * 1024));
    text.push('\n');
    let path = s.write("huge.jsonl", &text);
    let j = super::jsonl_view::open(&path).unwrap();
    assert_eq!(j.rows.len(), 1);
    assert!(j.rows[0].cut);
    assert_eq!(j.rows[0].number, 0);
    assert!(!j.more());
}
