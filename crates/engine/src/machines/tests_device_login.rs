use super::device_login::{
    is_device_code, is_device_link, strip_ansi, DeviceFilter, DevicePrompt, PromptSink, Scanner,
    WITHHELD,
};
use super::runner::{Channel, CommandResult, OutputSink};
use std::sync::{Arc, Mutex};

const PLAIN: &str = include_str!("../../tests/fixtures/codex-login/device-auth.txt");
const ANSI: &str = include_str!("../../tests/fixtures/codex-login/device-auth-ansi.txt");
const URL: &str = "https://auth.openai.com/codex/device";
const CODE: &str = "TEST-00000";
const STEPS: &str = "Follow these steps to sign in with ChatGPT using device code authorization:\n1. Open this link in your browser\n   https://auth.openai.com/codex/device\n2. Enter this one-time code (expires in 15 minutes)\n";

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

fn log_after(text: &str) -> String {
    let (filter, _seen, log) = rig();
    filter.feed(Channel::Stdout, text);
    filter.finish();
    let shown = lock(&log).clone();
    shown
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
fn the_filter_offers_the_prompt_once_and_withholds_the_rest_from_the_log() {
    for text in [PLAIN, ANSI] {
        let (filter, seen, log) = rig();
        filter.feed(Channel::Stdout, text);
        filter.finish();
        let seen = lock(&seen);
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].url(), URL);
        assert_eq!(seen[0].code(), CODE);
        let log = lock(&log).clone();
        assert!(!log.contains("openai"), "{log}");
        assert!(!log.contains(CODE), "{log}");
        assert!(!log.contains('\u{1b}'), "{log}");
        assert!(!log.contains("Device codes"), "{log}");
        assert!(!log.contains("Welcome"), "{log}");
        assert_eq!(log, format!("{WITHHELD}\n"));
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
    assert_eq!(lock(&seen)[0].code(), CODE);
    let log = lock(&log).clone();
    assert!(!log.contains(CODE), "{log}");
    assert!(!log.contains("openai.com/codex"), "{log}");
}

#[test]
fn a_code_in_any_shape_is_withheld_even_when_it_is_not_recognised() {
    let forms = [
        "zq7-abcd",
        "zq7abcd",
        "`ZQ7-ABCD`",
        "[ZQ7-ABCD]",
        "code=ZQ7-ABCD",
        "ZQ7--ABCD",
        "ZQ7 ABCD",
        "ZQ7",
        "<ZQ7-ABCD>",
        "\u{1b}[94mZQ7-ABCD\u{1b}[0m",
    ];
    for form in forms {
        let (filter, _seen, log) = rig();
        filter.feed(
            Channel::Stdout,
            &format!("{STEPS}   {form}\nerror: {form} expired\n"),
        );
        filter.finish();
        let shown = lock(&log).to_ascii_lowercase();
        assert!(!shown.contains("zq7"), "{form}: {shown}");
        assert!(!shown.contains("abcd"), "{form}: {shown}");
        assert!(!shown.contains("openai"), "{form}: {shown}");
    }
}

#[test]
fn a_code_line_without_a_newline_is_offered_after_a_short_pause_and_not_before() {
    let (filter, seen, log) = rig();
    filter.feed(Channel::Stdout, &format!("{STEPS}   {CODE}"));
    assert!(lock(&seen).is_empty());
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while lock(&seen).is_empty() && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(lock(&seen).last().unwrap().code(), CODE);
    filter.finish();
    assert!(!lock(&log).contains(CODE));
}

#[test]
fn half_a_code_is_never_offered_when_more_bytes_follow() {
    let (filter, seen, _log) = rig();
    filter.feed(Channel::Stdout, &format!("{STEPS}   ABCD"));
    filter.feed(Channel::Stdout, "-12345\n");
    std::thread::sleep(std::time::Duration::from_millis(400));
    let seen = lock(&seen);
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].code(), "ABCD-12345");
}

#[test]
fn the_first_code_is_kept_whatever_the_output_says_afterwards() {
    let (filter, seen, _log) = rig();
    filter.feed(
        Channel::Stdout,
        &format!(
            "{STEPS}   {CODE}\nDevice codes are a common phishing target. Never share this code.\nWAITING\nHTTP-200\n"
        ),
    );
    filter.finish();
    let seen = lock(&seen);
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].code(), CODE);
}

#[test]
fn a_line_with_the_link_and_the_code_word_still_finds_the_code() {
    let mut scanner = Scanner::default();
    assert!(scanner
        .line("Open https://auth.openai.com/codex/device and enter this one-time code:")
        .is_none());
    let found = scanner.line(CODE).unwrap();
    assert_eq!(found.url(), URL);
    assert_eq!(found.code(), CODE);
    let mut scanner = Scanner::default();
    let found = scanner
        .line("Open https://auth.openai.com/codex/device and enter one-time code: ABCD-12345")
        .unwrap();
    assert_eq!(found.code(), "ABCD-12345");
}

#[test]
fn output_before_the_prompt_is_withheld_too() {
    for said in [
        "Welcome\nuser_code=ZQ7-ABCD\n",
        "ZQ7-ABCD\n",
        "error: ZQ7-ABCD\n",
    ] {
        let shown = log_after(said);
        assert_eq!(shown, format!("{WITHHELD}\n"), "{said}");
    }
}

#[test]
fn carriage_return_spinners_split_lines_too() {
    let text = format!("{STEPS}   {CODE}\rWaiting {CODE} |\rWaiting {CODE} /\r");
    let shown = log_after(&text);
    assert!(!shown.contains(CODE), "{shown}");
}

#[test]
fn a_line_longer_than_the_limit_is_cut_and_never_logged() {
    let long = format!("{STEPS}   {}{CODE}", "x".repeat(6000));
    let shown = log_after(&long);
    assert!(!shown.contains(CODE), "{shown}");
    assert!(!shown.contains(&"x".repeat(100)), "{}", shown.len());
    let link = format!("see https://auth.openai.com/{}\n", "a".repeat(500));
    let shown = log_after(&link);
    assert!(!shown.contains("openai"), "{shown}");
    assert!(!shown.contains(&"a".repeat(50)), "{}", shown.len());
}

#[test]
fn the_code_may_come_before_the_link_and_the_link_may_be_a_terminal_hyperlink() {
    let osc = format!("\u{1b}]8;;{URL}\u{1b}\\{URL}\u{1b}]8;;\u{1b}\\");
    let lines = [
        "2. Enter this one-time code (expires in 15 minutes)",
        CODE,
        "1. Open this link",
        osc.as_str(),
    ];
    let mut scanner = Scanner::default();
    let found = lines.iter().find_map(|l| scanner.line(l)).unwrap();
    assert_eq!(found.url(), URL);
    assert_eq!(found.code(), CODE);
}

#[test]
fn other_tokens_in_the_banner_are_not_taken_for_the_code() {
    let lines = [
        "Using UTF-8 with SHA-256 checks",
        "Follow these steps to sign in with ChatGPT using device code authorization:",
        "Verified SHA-256",
        "1. Open this link",
        URL,
        "SHA-256",
        "2. Enter this one-time code (expires in 15 minutes)",
        CODE,
    ];
    let mut scanner = Scanner::default();
    let found = lines.iter().find_map(|l| scanner.line(l)).unwrap();
    assert_eq!(found.code(), CODE);
}

#[test]
fn a_label_with_the_code_on_the_same_line_is_read() {
    let mut scanner = Scanner::default();
    assert!(scanner.line(URL).is_none());
    let found = scanner.line("one-time code: abcd-12345").unwrap();
    assert_eq!(found.code(), "abcd-12345");
}

#[test]
fn a_prompt_needs_a_known_host_and_a_code_shape() {
    let mut scanner = Scanner::default();
    assert!(scanner.line("Enter code TEST-00000").is_none());
    let mut scanner = Scanner::default();
    assert!(scanner.line("open https://evil.example/device").is_none());
    assert!(scanner.line("one-time code").is_none());
    assert!(scanner.line("TEST-00000").is_none());
    assert!(!is_device_link("https://auth.openai.com.evil.example/x"));
    assert!(!is_device_link("http://auth.openai.com/x"));
    assert!(!is_device_link("https://auth.openai.com/x y"));
    assert!(is_device_link(URL));
    assert!(is_device_code("ABCD-12345"));
    assert!(is_device_code("abcd-12345"));
    assert!(is_device_code("ABCD12345"));
    assert!(!is_device_code("abcd"));
    assert!(!is_device_code("-ABCD"));
    assert!(!is_device_code("ab"));
    assert!(!is_device_code("AB CD"));
}

#[test]
fn escape_sequences_are_removed() {
    assert_eq!(strip_ansi("\u{1b}[1mhi\u{1b}[0m there"), "hi there");
    assert_eq!(strip_ansi("a\u{1b}]0;title\u{7}b"), "ab");
    assert_eq!(strip_ansi("a\u{1b}]0;title\u{1b}\\b"), "ab");
}

#[test]
fn the_prompt_never_prints_its_values() {
    let found = DevicePrompt::new(URL, CODE).unwrap();
    let text = format!("{found:?}");
    assert!(!text.contains("openai"), "{text}");
    assert!(!text.contains("TEST"), "{text}");
    assert!(DevicePrompt::new("https://example.test/x", CODE).is_none());
}

#[test]
fn a_command_result_never_prints_its_output() {
    let result = CommandResult::new(3, "ZQ7-OUT https://auth.openai.com/x", "ZQ7-ERR");
    let text = format!("{result:?}");
    assert!(!text.contains("ZQ7"), "{text}");
    assert!(!text.contains("openai"), "{text}");
    assert!(text.contains('3'), "{text}");
}
