use super::redact::{hide_forms, Redactor};
use std::collections::BTreeMap;

fn hide(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(v, m)| (v.to_string(), m.to_string()))
        .collect()
}

fn closed(h: &BTreeMap<String, String>, text: &str) -> String {
    let mut r = Redactor::new(h);
    let mut out = r.feed(text.as_bytes());
    out.extend(r.finish_closed());
    String::from_utf8(out).unwrap()
}

#[test]
fn a_window_starting_inside_a_value_masks_the_rest_of_it() {
    let r = Redactor::new(&hide(&[("NL-MARKER-16-VALUE", "[x]")]));
    let text = "pre NL-MARKER-16-VALUE post";
    let (out, held) = r.mask_window(&text.as_bytes()[..7], &text.as_bytes()[7..], false);
    assert!(String::from_utf8(out).unwrap() == "[x] post");
    assert!(held == 0);
    let (blind, _) = r.mask_window(&[], &text.as_bytes()[7..], false);
    assert!(String::from_utf8(blind).unwrap().contains("ER-16-VALUE"));
}

#[test]
fn a_window_starting_mid_line_of_a_multiline_value_masks_the_line() {
    let value = "NL-MARKER-17-LINE-AAAAAAAA\nNL-MARKER-17-LINE-BBBBBBBB";
    let forms = hide_forms(value, "[x]");
    let r = Redactor::new(&forms);
    let text = "head\nNL-MARKER-17-LINE-BBBBBBBB\ntail";
    let at = 5 + 12;
    let lead = &text.as_bytes()[..at];
    let (out, _) = r.mask_window(lead, &text.as_bytes()[at..], true);
    let out = String::from_utf8(out).unwrap();
    assert!(!out.contains("BBBBBBBB"));
    assert!(out.contains("[x]"));
    assert!(out.ends_with("\ntail"));
}

#[test]
fn a_window_emits_nothing_for_bytes_inside_the_lead() {
    let r = Redactor::new(&hide(&[("NL-MARKER-16-VALUE", "[x]")]));
    let text = "a NL-MARKER-16-VALUE";
    let (out, held) = r.mask_window(text.as_bytes(), b"", true);
    assert!(out.is_empty() && held == 0);
}

#[test]
fn a_complete_value_after_the_lead_is_masked_once() {
    let r = Redactor::new(&hide(&[("NL-MARKER-16-VALUE", "[x]")]));
    let (out, held) = r.mask_window(b"zz", b" NL-MARKER-16-VALUE end NL-MARK", false);
    assert!(String::from_utf8(out).unwrap() == " [x] end ");
    assert!(held == 7);
}

#[test]
fn closed_masks_a_straddled_prefix_without_leaking() {
    let h = hide(&[("abcd", "[o]"), ("cdefgh12345678XYZ", "[t]")]);
    assert!(closed(&h, "abcdefgh12345678") == "[o][t]");

    let h = hide(&[("abcdefgh", "[v]"), ("efghijkl", "[w]")]);
    assert!(closed(&h, "abcdefgh") == "[v]");

    let h = hide(&[("MARKER-18-ABC", "[m]"), ("RKER-18-ABCDEFGHIJKL", "[l]")]);
    assert!(closed(&h, "x MARKER-18-ABCDEFGHI") == "x [m][l]");
}

#[test]
fn a_self_overlapping_value_does_not_stall_a_stream() {
    let h = hide(&[("abababab", "[x]")]);
    let mut r = Redactor::new(&h);
    let chunk = "ab".repeat(35_000);
    let out = r.feed(chunk.as_bytes());
    assert!(!out.is_empty());
    assert!(r.held().len() < 100);
    let mut all = out;
    all.extend(r.finish_closed());
    assert!(!String::from_utf8(all).unwrap().contains("abab"));
}

#[test]
fn a_window_over_a_periodic_chunk_is_all_held_without_a_closed_pass() {
    let r = Redactor::new(&hide(&[("abababab", "[x]")]));
    let chunk = "ab".repeat(100);
    let (_, held) = r.mask_window(b"", chunk.as_bytes(), false);
    assert!(held > r.lookback());
    let (out, held) = r.mask_window(b"", chunk.as_bytes(), true);
    assert!(held == 0);
    assert!(!String::from_utf8(out).unwrap().contains("abab"));
}

#[test]
fn lookback_is_the_longest_form_minus_one() {
    let r = Redactor::new(&hide(&[("abcd", "[a]"), ("abcdefgh", "[b]")]));
    assert!(r.lookback() == 7);
    assert!(Redactor::new(&BTreeMap::new()).lookback() == 0);
}

#[test]
fn trimmed_and_cased_forms_are_covered() {
    let forms = hide_forms("  NL-MARKER-19-TOKEN \n", "[m]");
    assert!(forms.contains_key("NL-MARKER-19-TOKEN"));

    let lines = hide_forms("   NL-MARKER-19-LINE-AAAA   \nsecond-line-here", "[m]");
    assert!(lines.contains_key("NL-MARKER-19-LINE-AAAA"));

    let wide = hide_forms("NL-MARKER-20-\u{e9}\u{4e2d}", "[m]");
    assert!(wide.contains_key("NL-MARKER-20-\\u00e9\\u4e2d"));
    assert!(wide.contains_key("NL-MARKER-20-\\u00E9\\u4E2D"));

    let ctl = hide_forms("NL-MARKER-21-\u{8}-\u{c}-end", "[m]");
    assert!(ctl.contains_key("NL-MARKER-21-\\b-\\f-end"));
    assert!(ctl.contains_key("NL-MARKER-21-\\u0008-\\u000c-end"));
}

#[test]
fn no_start_offset_shows_a_piece_of_a_value() {
    let value = "NL-MARKER-22-QWERTYUIOP";
    let r = Redactor::new(&hide(&[(value, "[x]")]));
    let text = format!("zz {value} zz {value}");
    let bytes = text.as_bytes();
    for from in 0..=bytes.len() {
        let lead = &bytes[from.saturating_sub(r.lookback())..from];
        let (out, held) = r.mask_window(lead, &bytes[from..], true);
        let out = String::from_utf8(out).unwrap();
        assert!(held == 0);
        for k in 0..value.len() - 3 {
            assert!(!out.contains(&value[k..]), "a piece of the value leaked");
        }
    }
}
