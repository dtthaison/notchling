//! The redactor (Swift `SecretVaultTests`' first four, then the guarantees the Swift tests didn't
//! pin: the output never depends on how the input was cut, overlapping values are hidden whole,
//! a mask is never matched again) and the runner, with `/bin/sh` only.

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
    // A value inside another, and one that repeats into itself.
    assert_eq!(run(&[("aaaa", "[a]")], &["aaaaaa"]), "[a][a]");
    assert_eq!(run(&[("abab", "[w]")], &["ababab!"]), "[w][w]!");
    assert_eq!(
        run(&[("hunter22", "[p]"), ("hunt", "[u]")], &["xhunter22hunt"]),
        "x[p][u]"
    );
}

#[test]
fn a_mask_is_never_matched_again() {
    // A value that is part of another's mask, and a value made of a mask's tail and what follows.
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

/// The cuts of `input` into 1 to 3 reads, every way, plus one byte at a time.
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

/// The output for the whole input in one read, which every cut must equal.
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

/// A small deterministic generator: no `rand` crate in the engine.
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
        // Values and text over a two-letter alphabet overlap often; masks use other bytes.
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
        // Every two-read cut, and a few random many-read ones.
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
