use super::device_login::{
    is_device_code, is_device_link, strip_ansi, DeviceFilter, DevicePrompt, PromptSink, Scanner,
    HIDDEN_CODE, HIDDEN_LINK,
};
use super::runner::{Channel, OutputSink};
use std::sync::{Arc, Mutex};

const PLAIN: &str = include_str!("../../tests/fixtures/codex-login/device-auth.txt");
const ANSI: &str = include_str!("../../tests/fixtures/codex-login/device-auth-ansi.txt");
const URL: &str = "https://auth.openai.com/codex/device";
const CODE: &str = "TEST-00000";

type Seen = Arc<Mutex<Vec<DevicePrompt>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn rig() -> (DeviceFilter, Seen, Arc<Mutex<String>>) {
    let seen: Seen = Arc::default();
    let log = Arc::new(Mutex::new(String::new()));
    let (s, l) = (seen.clone(), log.clone());
    let prompt: PromptSink = Arc::new(move |p| lock(&s).push(p));
    let sink: Arc<OutputSink> = Arc::new(move |_c: Channel, t: &str| lock(&l).push_str(t));
    (DeviceFilter::new(sink, prompt), seen, log)
}

#[test]
fn the_recorded_output_gives_the_link_and_the_code() {
    for text in [PLAIN, ANSI] {
        let mut scanner = Scanner::default();
        let found = text.lines().find_map(|l| scanner.line(l)).unwrap();
        assert_eq!(found.url(), URL);
        assert_eq!(found.code(), CODE);
    }
}

#[test]
fn the_filter_offers_the_prompt_once_and_hides_both_from_the_log() {
    for text in [PLAIN, ANSI] {
        let (filter, seen, log) = rig();
        filter.feed(Channel::Stdout, text);
        filter.finish();
        let seen = lock(&seen);
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].url(), URL);
        assert_eq!(seen[0].code(), CODE);
        let log = lock(&log).clone();
        assert!(!log.contains("auth.openai.com"), "{log}");
        assert!(!log.contains(CODE), "{log}");
        assert!(!log.contains('\u{1b}'), "{log}");
        assert!(log.contains(HIDDEN_LINK), "{log}");
        assert!(log.contains(HIDDEN_CODE), "{log}");
        assert!(log.contains("Welcome to Codex"), "{log}");
    }
}

#[test]
fn output_split_in_the_middle_of_a_line_is_still_found_and_hidden() {
    let (filter, seen, log) = rig();
    for chunk in PLAIN.as_bytes().chunks(7) {
        filter.feed(Channel::Stdout, std::str::from_utf8(chunk).unwrap());
    }
    filter.finish();
    assert_eq!(lock(&seen).len(), 1);
    let log = lock(&log).clone();
    assert!(!log.contains(CODE), "{log}");
    assert!(!log.contains("openai.com/codex"), "{log}");
}

#[test]
fn a_late_error_that_repeats_the_code_or_link_is_scrubbed() {
    let (filter, _seen, _log) = rig();
    filter.feed(Channel::Stdout, PLAIN);
    filter.finish();
    let said = format!("error: code {CODE}. expired at {URL}, try (TEST-00000)\nTEST-99999 too");
    let shown = filter.scrub(&said);
    assert!(!shown.contains("TEST-"), "{shown}");
    assert!(!shown.contains("openai.com"), "{shown}");
    assert!(shown.contains("error:"), "{shown}");
}

#[test]
fn a_prompt_needs_the_link_before_the_code_and_a_known_host() {
    let mut scanner = Scanner::default();
    assert!(scanner.line("Enter code TEST-00000").is_none());
    let mut scanner = Scanner::default();
    assert!(scanner.line("open https://evil.example/device").is_none());
    assert!(scanner.line("TEST-00000").is_none());
    assert!(!is_device_link("https://auth.openai.com.evil.example/x"));
    assert!(!is_device_link("http://auth.openai.com/x"));
    assert!(!is_device_link("https://auth.openai.com/x y"));
    assert!(is_device_link(URL));
    assert!(is_device_code("ABCD-12345"));
    assert!(!is_device_code("abcd-12345"));
    assert!(!is_device_code("device-auth"));
    assert!(!is_device_code("ABCD12345"));
}

#[test]
fn escape_sequences_are_removed() {
    assert_eq!(strip_ansi("\u{1b}[1mhi\u{1b}[0m there"), "hi there");
    assert_eq!(strip_ansi("a\u{1b}]8;;x\u{7}b"), "ab");
}

#[test]
fn the_prompt_never_prints_its_values() {
    let found = DevicePrompt::new(URL, CODE).unwrap();
    let text = format!("{found:?}");
    assert!(!text.contains("openai"), "{text}");
    assert!(!text.contains("TEST"), "{text}");
    assert!(DevicePrompt::new("https://example.test/x", CODE).is_none());
}
