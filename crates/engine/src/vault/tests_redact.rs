use super::redact::Redactor;
use super::runner;
use std::collections::BTreeMap;

fn hide(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(v, m)| (v.to_string(), m.to_string()))
        .collect()
}

fn run_chunks(h: &BTreeMap<String, String>, chunks: &[&[u8]]) -> Vec<u8> {
    let mut r = Redactor::new(h);
    let mut out = Vec::new();
    for c in chunks {
        out.extend(r.feed(c));
    }
    out.extend(r.finish());
    out
}

fn run(pairs: &[(&str, &str)], chunks: &[&str]) -> String {
    let chunks: Vec<&[u8]> = chunks.iter().map(|c| c.as_bytes()).collect();
    String::from_utf8(run_chunks(&hide(pairs), &chunks)).unwrap()
}

#[test]
fn hides_a_value() {
    assert_eq!(
        run(
            &[("s3cr3t-tok", "[secret:gh]")],
            &["token is s3cr3t-tok, again s3cr3t-tok\n"]
        ),
        "token is [secret:gh], again [secret:gh]\n"
    );
}

#[test]
fn hides_a_value_split_across_reads() {
    assert_eq!(
        run(&[("abcdefgh", "[x]")], &["one abc", "def", "gh two"]),
        "one [x] two"
    );
}

#[test]
fn keeps_short_strings_and_plain_output() {
    assert_eq!(run(&[("ab", "[x]")], &["ab cd"]), "ab cd");
    assert_eq!(run(&[("longsecret", "[x]")], &["a", "b", "c"]), "abc");
    assert_eq!(run(&[], &["plain"]), "plain");
}

#[test]
fn longest_first() {
    assert_eq!(
        run(
            &[("pass", "[u]"), ("password1", "[p]")],
            &["password1 pass"]
        ),
        "[p] [u]"
    );
}

#[test]
fn only_a_possible_start_is_held() {
    let h = hide(&[("abcdefgh", "[x]")]);
    let mut r = Redactor::new(&h);
    assert_eq!(r.feed(b"hello ab"), b"hello ".to_vec(), "ab could start it");
    assert_eq!(
        r.feed(b"x then "),
        b"abx then ".to_vec(),
        "not a start any more"
    );
    assert_eq!(r.feed(b"abcdefg"), b"".to_vec());
    assert_eq!(
        r.finish(),
        b"abcdefg".to_vec(),
        "the end of the stream lets it go"
    );
}

#[test]
fn overlapping_values_are_hidden_whole() {
    let pairs = [("abcd", "[A]"), ("cdefgh", "[B]")];
    assert_eq!(run(&pairs, &["xabcdefghy"]), "x[A][B]y");
    assert_eq!(run(&[("aaaa", "[a]")], &["aaaaaa"]), "[a][a]");
    assert_eq!(run(&[("abab", "[w]")], &["ababab!"]), "[w][w]!");
    assert_eq!(
        run(&[("hunter22", "[p]"), ("hunt", "[u]")], &["xhunter22hunt"]),
        "x[p][u]"
    );
}

#[test]
fn a_mask_is_never_matched_again() {
    let pairs = [("token-value", "[secret:gh]"), ("secret", "[secret:s]")];
    assert_eq!(
        run(&pairs, &["token-value"]),
        "[secret:gh]",
        "\"secret\" in the mask is left alone"
    );
    let pairs = [("abcdefgh", "[x]"), ("x]zz", "[y]")];
    assert_eq!(run(&pairs, &["abcdefghzz"]), "[x]zz");
    assert_eq!(run(&pairs, &["abcdefgh", "zz"]), "[x]zz");
}

fn every_cut(input: &[u8]) -> Vec<Vec<&[u8]>> {
    let n = input.len();
    let mut cuts = vec![vec![input], input.chunks(1).collect()];
    for i in 0..=n {
        cuts.push(vec![&input[..i], &input[i..]]);
        for j in i..=n {
            cuts.push(vec![&input[..i], &input[i..j], &input[j..]]);
        }
    }
    cuts
}

fn whole(h: &BTreeMap<String, String>, input: &[u8]) -> Vec<u8> {
    run_chunks(h, &[input])
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn every_cut_gives_the_same_output() {
    let sets: Vec<BTreeMap<String, String>> = vec![
        hide(&[("abcdefgh", "<1>")]),
        hide(&[("pass", "<u>"), ("password1", "<p>")]),
        hide(&[("abcd", "<A>"), ("cdefgh", "<B>"), ("ghij", "<C>")]),
        hide(&[("aaaa", "<a>"), ("aaab", "<b>")]),
        hide(&[("abab", "<w>"), ("babab", "<v>")]),
    ];
    let inputs: [&[u8]; 9] = [
        b"one abcdefgh two",
        b"password1 pass passw",
        b"xabcdefghijy cdef abcdefg",
        b"aaaaaaab aaa",
        b"bababababa",
        b"",
        b"abc",
        b"\xff\xfeabcdefgh\x00",
        b"pass",
    ];
    for h in &sets {
        for input in inputs {
            let expected = whole(h, input);
            for cut in every_cut(input) {
                assert_eq!(
                    run_chunks(h, &cut),
                    expected,
                    "{h:?} {:?} cut as {cut:?}",
                    String::from_utf8_lossy(input)
                );
            }
            for value in h.keys() {
                assert!(
                    !contains(&expected, value.as_bytes()),
                    "{value} shows in {:?}",
                    String::from_utf8_lossy(&expected)
                );
            }
        }
    }
}

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> usize {
        (self.next() % n) as usize
    }
}

#[test]
fn random_texts_never_show_a_value_however_they_are_cut() {
    let mut rng = XorShift(0x5eed_1234_abcd_0001);
    for _ in 0..300 {
        let alphabet = b"ab";
        let word = |rng: &mut XorShift, len: usize| -> String {
            (0..len).map(|_| alphabet[rng.below(2)] as char).collect()
        };
        let count = 1 + rng.below(3);
        let mut h = BTreeMap::new();
        for i in 0..count {
            let len = 4 + rng.below(4);
            h.insert(word(&mut rng, len), format!("<{i}>"));
        }
        let len = rng.below(40);
        let input = word(&mut rng, len).into_bytes();
        let expected = whole(&h, &input);
        for value in h.keys() {
            assert!(!contains(&expected, value.as_bytes()), "{h:?} {input:?}");
        }
        for i in 0..=input.len() {
            assert_eq!(run_chunks(&h, &[&input[..i], &input[i..]]), expected);
        }
        for _ in 0..5 {
            let mut cut = Vec::new();
            let mut at = 0;
            while at < input.len() {
                let step = 1 + rng.below(5);
                let end = (at + step).min(input.len());
                cut.push(&input[at..end]);
                at = end;
            }
            assert_eq!(run_chunks(&h, &cut), expected);
        }
    }
}

#[cfg(unix)]
#[test]
fn the_runner_hides_values_in_both_streams_and_keeps_the_exit_code() {
    let env = hide(&[("FAKE_TOKEN", "not-a-real-token-1234")]);
    let h = hide(&[("not-a-real-token-1234", "[secret:fake]")]);
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = runner::run(
        &[
            "printf 'out %s\\n' \"$FAKE_TOKEN\"; printf 'err %s' \"$FAKE_TOKEN\" >&2; exit 7"
                .into(),
        ],
        &env,
        &h,
        &mut out,
        &mut err,
    );
    assert_eq!(code, 7);
    assert_eq!(String::from_utf8(out).unwrap(), "out [secret:fake]\n");
    assert_eq!(String::from_utf8(err).unwrap(), "err [secret:fake]");
}

#[cfg(unix)]
#[test]
fn the_runner_runs_many_arguments_without_a_shell_and_reports_signals() {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let args: Vec<String> = ["printf", "%s|%s", "a b", "$HOME"]
        .map(String::from)
        .to_vec();
    let code = runner::run(
        &args,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &mut out,
        &mut err,
    );
    assert_eq!(code, 0);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "a b|$HOME",
        "no shell expanded it"
    );
    let mut out = Vec::new();
    let code = runner::run(
        &["kill -TERM $$".into()],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &mut out,
        &mut err,
    );
    assert_eq!(code, 128 + 15);
    let code = runner::run(
        &["/nonexistent/fake-tool".into(), "x".into()],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &mut out,
        &mut err,
    );
    assert_eq!(code, 127, "env's own code for a missing program");
    assert_eq!(
        runner::run(&[], &BTreeMap::new(), &BTreeMap::new(), &mut out, &mut err),
        2
    );
}

#[cfg(unix)]
#[test]
fn a_value_split_across_the_commands_writes_is_still_hidden() {
    let h = hide(&[("fake-value-abcdef", "[secret:x]")]);
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = runner::run(
        &["printf 'fake-va'; sleep 0.2; printf 'lue-abcdef done'".into()],
        &BTreeMap::new(),
        &h,
        &mut out,
        &mut err,
    );
    assert_eq!(code, 0);
    assert_eq!(String::from_utf8(out).unwrap(), "[secret:x] done");
}

const PEM: &str = "-----BEGIN NL-MARKER-7-----\nNL-MARKER-7-LINE-AAAAAAAA\nNL-MARKER-7-LINE-BBBBBBBB\n-----END NL-MARKER-7-----";

#[test]
fn hide_forms_cover_json_escaped_and_lines() {
    let forms = super::redact::hide_forms(PEM, "[secret pem]");
    assert!(forms.contains_key(PEM));
    assert!(forms.contains_key(&PEM.replace('\n', "\\n")));
    assert!(forms.contains_key("NL-MARKER-7-LINE-AAAAAAAA"));
    assert!(forms.contains_key("NL-MARKER-7-LINE-BBBBBBBB"));
    assert!(forms.contains_key("-----BEGIN NL-MARKER-7-----"));
    assert!(forms.values().all(|m| m == "[secret pem]"));

    let crlf = super::redact::hide_forms(
        "NL-MARKER-8-LINE-AAAAAAAA\r\nNL-MARKER-8-LINE-BBBBBBBB",
        "[m]",
    );
    assert!(crlf.contains_key("NL-MARKER-8-LINE-AAAAAAAA"));
    assert!(crlf.contains_key("NL-MARKER-8-LINE-BBBBBBBB"));
    assert!(crlf.contains_key("NL-MARKER-8-LINE-AAAAAAAA\\r\\nNL-MARKER-8-LINE-BBBBBBBB"));

    let short = super::redact::hide_forms("ab\n= =\n\nlonger-than-eight", "[m]");
    for trivial in ["ab", "= =", "=", " ", ""] {
        assert!(!short.contains_key(trivial), "{trivial:?}");
    }
    assert!(short.contains_key("longer-than-eight"));

    let quoted = super::redact::hide_forms("NL-MARKER-9\"q/x\\y", "[m]");
    assert!(quoted.contains_key("NL-MARKER-9\\\"q/x\\\\y"));
    assert!(quoted.contains_key("NL-MARKER-9\\\"q\\/x\\\\y"));

    let wide = super::redact::hide_forms("NL-MARKER-10-\u{e9}", "[m]");
    assert!(wide.contains_key("NL-MARKER-10-\\u00e9"));
}

#[test]
fn hidden_forms_mask_a_json_line() {
    let forms = super::redact::hide_forms(PEM, "[secret pem]");
    let line = format!("{{\"text\":{}}}\n", serde_json::to_string(PEM).unwrap());
    let out = String::from_utf8(run_chunks(&forms, &[line.as_bytes()])).unwrap();
    assert!(!out.contains("NL-MARKER-7"));
    assert!(out.contains("[secret pem]"));
}

#[test]
fn a_cloned_redactor_keeps_its_own_state() {
    let h = hide(&[("abcdefgh", "[x]")]);
    let mut a = Redactor::new(&h);
    let mut b = a.clone();
    let mut out = a.feed(b"one abc");
    assert!(!a.held().is_empty());
    assert!(b.held().is_empty());
    out.extend(a.feed(b"defgh two"));
    out.extend(a.finish());
    assert_eq!(String::from_utf8(out).unwrap(), "one [x] two");
    assert_eq!(b.feed(b"plain"), b"plain");
}

#[test]
fn hide_forms_cover_html_and_double_escaped_json() {
    let value = "NL-MARKER-13<&>\"q\"\\z";
    let forms = super::redact::hide_forms(value, "[m]");
    assert!(forms.contains_key("NL-MARKER-13\\u003c\\u0026\\u003e\\\"q\\\"\\\\z"));
    assert!(forms.contains_key("NL-MARKER-13<&>\\\"q\\\"\\\\z"));
    assert!(forms.contains_key("NL-MARKER-13<&>\\\\\\\"q\\\\\\\"\\\\\\\\z"));

    let lines = super::redact::hide_forms("first line of text\nNL-MARKER-14-<LINE>-\"x\"", "[m]");
    assert!(lines.contains_key("NL-MARKER-14-\\u003cLINE\\u003e-\\\"x\\\""));
    assert!(lines.contains_key("NL-MARKER-14-<LINE>-\\\\\\\"x\\\\\\\""));
}

#[test]
fn finish_closed_masks_any_held_prefix() {
    let h = hide(&[("NL-MARKER-15-VALUE", "[x]")]);
    let mut r = Redactor::new(&h);
    let mut out = r.feed(b"end NL-MARKER-1");
    out.extend(r.finish_closed());
    assert!(String::from_utf8(out).unwrap() == "end [x]");

    let mut r = Redactor::new(&h);
    let mut out = r.feed(b"end NL-MAR");
    out.extend(r.finish_closed());
    assert!(String::from_utf8(out).unwrap() == "end [x]");

    let mut r = Redactor::new(&h);
    let mut out = r.feed(b"a NL-MARKER-15-VALUE b NL-MARKER-15-VALUE");
    out.extend(r.finish_closed());
    assert!(String::from_utf8(out).unwrap() == "a [x] b [x]");

    let mut r = Redactor::new(&h);
    let mut out = r.feed(b"plain");
    out.extend(r.finish());
    assert!(String::from_utf8(out).unwrap() == "plain");
}

#[cfg(unix)]
#[test]
fn a_multi_line_value_reaches_the_command_as_it_was_saved_and_every_line_is_hidden() {
    use super::entry::{Kind, VaultEntry, VaultValue};

    let pem = "-----BEGIN PRIVATE KEY-----\r\nNLFAKE0aZk1q2Xx9fPzT0bYw4Lr8mNcV3uHj5Ka1dEo6sGt\r\nNLFAKE1pL0cT8yUeB4nHs2JdX6fMq9ZrW3gVk5Ao7iCtYb1D\r\n-----END PRIVATE KEY-----\r\n";
    let mut entry = VaultEntry::new("deploy-key", Kind::Token);
    entry.secret_var = "DEPLOY_KEY".into();
    let value = VaultValue {
        username: String::new(),
        secret: pem.into(),
    };
    let (env, hide) = super::list::environment(&[(entry, value)]);
    assert_eq!(
        env.get("DEPLOY_KEY").map(String::as_bytes),
        Some(pem.as_bytes())
    );

    let script = "printf '%s' \"$DEPLOY_KEY\" | wc -c; \
                  printf '%s' \"$DEPLOY_KEY\" | sed -n 1p; \
                  printf '%s' \"$DEPLOY_KEY\" | sed -n 2p; \
                  printf '%s' \"$DEPLOY_KEY\" | sed -n 3p; \
                  printf '%s' \"$DEPLOY_KEY\" | sed -n 4p >&2";
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = runner::run(&[script.into()], &env, &hide, &mut out, &mut err);
    assert_eq!(code, 0);
    let out = String::from_utf8_lossy(&out).into_owned();
    let err = String::from_utf8_lossy(&err).into_owned();
    let count = pem.len().to_string();
    assert_eq!(out.lines().next().map(str::trim), Some(count.as_str()));
    for shown in [&out, &err] {
        assert!(!shown.contains("NLFAKE"), "{shown}");
        assert!(!shown.contains("PRIVATE KEY"), "{shown}");
    }
    assert_eq!(out.matches("[secret:deploy-key]").count(), 3, "{out}");
    assert!(err.contains("[secret:deploy-key]"), "{err}");
}

#[test]
fn a_short_line_of_a_multi_line_value_is_hidden_from_the_fourth_character_on() {
    let key = "-----BEGIN NL-MARKER-23-----\nNL-MARKER-23-LINE-AAAAAAAA\nQ9==\n-----END NL-MARKER-23-----\n";
    let forms = super::redact::hide_forms(key, "[secret:k]");
    assert!(forms.contains_key("Q9=="));
    let shown = String::from_utf8(run_chunks(&forms, &[b"tail Q9== end\n"])).unwrap();
    assert_eq!(shown, "tail [secret:k] end\n");

    let tiny = super::redact::hide_forms("NL-MARKER-24-LINE-AAAAAAAA\nQ==\n=\n  \n", "[secret:k]");
    for trivial in ["Q==", "=", "  "] {
        assert!(!tiny.contains_key(trivial), "{trivial:?}");
    }
    let plain = String::from_utf8(run_chunks(&tiny, &[b"a = b Q== c\n"])).unwrap();
    assert_eq!(plain, "a = b Q== c\n");
}

#[test]
fn a_line_of_four_or_more_bytes_is_hidden_whatever_it_holds_and_trimmed_too() {
    let value = "NL-MARKER-25-LINE-AAAAAAAA\nA B C   \na b c\n\u{4e2d}\u{6587}\nQ9==\n==\n      \n";
    let forms = super::redact::hide_forms(value, "[secret:k]");
    for line in ["A B C   ", "A B C", "a b c", "\u{4e2d}\u{6587}", "Q9=="] {
        assert!(forms.contains_key(line), "{line:?}");
    }
    assert!(!forms.contains_key("=="));
    assert!(!forms.contains_key("      "));
    let shown = String::from_utf8(run_chunks(
        &forms,
        &[b"x A B C y a b c z Q9== w == v\n    indented\n"],
    ))
    .unwrap();
    assert_eq!(
        shown,
        "x [secret:k] y [secret:k] z [secret:k] w == v\n    indented\n"
    );
}
