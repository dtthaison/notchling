use notchling_engine::terminal::shell_output::output_below;
use std::path::Path;

const COLS: usize = 200;

fn shot(name: &str) -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/shell-live")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut lines = text.lines();
    assert!(lines.next().unwrap().starts_with("cursor "), "{name}");
    lines.map(String::from).collect()
}

fn text(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/shell-live")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

const LOGIN: &str = "gcloud auth application-default login --scopes=https://www.example.invalid/auth/cloud-platform";

#[test]
fn a_login_that_waits_shows_its_link_whole_under_the_command() {
    let url = text("login-running.url");
    let out = output_below(&shot("login-running.txt"), LOGIN, COLS);
    assert_eq!(
        out,
        [
            "Your browser has been opened to visit:".to_string(),
            String::new(),
            format!("    {url}"),
        ]
    );
}

#[test]
fn a_running_commands_lines_come_without_chrome() {
    let out = output_below(&shot("running-lines.txt"), "npm run build", COLS);
    assert_eq!(
        out,
        [
            "> app@1.0.0 build",
            "> tsc -p .",
            "",
            "src/main.ts(3,7): error TS2322: Type 'string' is not assignable to type 'number'.",
            "  3 | const n: number = \"x\";",
        ]
    );
}

#[test]
fn a_command_that_printed_nothing_yet_has_no_lines() {
    assert!(output_below(&shot("no-output-yet.txt"), "npm run build", COLS).is_empty());
}

#[test]
fn a_finished_commands_output_stops_before_the_recap() {
    let out = output_below(&shot("finished.txt"), "npm run build", COLS);
    assert_eq!(out, ["done in 3s", "2 files changed"]);
}

#[test]
fn the_same_command_run_twice_shows_the_newest_run() {
    let out = output_below(&shot("same-command-twice.txt"), "npm run build", COLS);
    assert_eq!(out, ["fresh output of the second run"]);
}

#[test]
fn the_command_typed_into_the_box_is_not_its_echo() {
    assert!(output_below(&shot("typed-in-box.txt"), "npm run build", COLS).is_empty());
}

#[test]
fn a_command_that_wraps_over_rows_is_found_whole() {
    let command = text("long-command.cmd");
    let out = output_below(&shot("long-command.txt"), &command, COLS);
    assert_eq!(out, ["a1b2c3d fix the build", "e4f5a6b add the thing"]);
}

#[test]
fn a_command_that_is_not_on_the_screen_has_no_lines() {
    assert!(output_below(&shot("running-lines.txt"), "ls -la", COLS).is_empty());
    assert!(output_below(&[], "ls", COLS).is_empty());
    assert!(output_below(&shot("running-lines.txt"), "", COLS).is_empty());
}

fn rows(v: &[&str]) -> Vec<String> {
    v.iter().map(|r| r.to_string()).collect()
}

#[test]
fn output_drawn_without_the_connector_is_taken_to_the_next_element() {
    let screen = rows(&["! echo hi", "hi", "there", "", "✻ Cooked for 1s", "❯ next"]);
    assert_eq!(output_below(&screen, "echo hi", COLS), ["hi", "there"]);
}

#[test]
fn a_status_line_and_a_collapsed_notice_are_not_output() {
    let screen = rows(&[
        "! make",
        "  ⎿  Running… (3s · timeout 2m)",
        "     cc a.c",
        "     … +40 lines (ctrl+o to expand)",
        "     (3s · timeout 2m)",
        "     cc z.c",
    ]);
    assert_eq!(output_below(&screen, "make", COLS), ["cc a.c", "cc z.c"]);
}

#[test]
fn a_long_word_that_is_not_a_link_stays_on_its_own_row() {
    let word = "x".repeat(195);
    let screen = rows(&[
        "! cat blob",
        "  ⎿  head",
        &format!("     {word}"),
        "     tail",
    ]);
    let out = output_below(&screen, "cat blob", COLS);
    assert_eq!(out, vec!["head".to_string(), word, "tail".to_string()]);
}

#[test]
fn a_link_wrapped_by_the_terminal_itself_is_joined_too() {
    let url = format!("https://example.invalid/{}", "a".repeat(260));
    let (first, rest) = url.split_at(194);
    let screen = rows(&[
        "! open-login",
        &format!("  ⎿  Visit {first}"),
        rest,
        "",
        "✽ Running…",
    ]);
    let out = output_below(&screen, "open-login", COLS);
    assert_eq!(out, [format!("Visit {url}")]);
}

#[test]
fn only_the_tail_of_a_very_long_output_is_kept() {
    let mut screen = rows(&["! seq 1 500", "  ⎿  1"]);
    for n in 2..=500 {
        screen.push(format!("     {n}"));
    }
    let out = output_below(&screen, "seq 1 500", COLS);
    assert_eq!(out.len(), 120);
    assert_eq!(out.last().map(String::as_str), Some("500"));
}

#[test]
fn a_screen_drawn_by_the_terminal_keeps_full_rows_whole() {
    use notchling_engine::terminal::TerminalScreen;
    let mut screen = TerminalScreen::new(50, COLS);
    for row in shot("login-running.txt") {
        screen.feed(row.as_bytes());
        screen.feed(b"\r\n");
    }
    let url = text("login-running.url");
    let out = output_below(&screen.lines(), LOGIN, COLS);
    assert_eq!(
        out.last().map(String::as_str),
        Some(format!("    {url}").as_str())
    );
}

#[test]
fn reading_a_session_types_nothing_and_returns_the_commands_lines() {
    use notchling_engine::reply::fake::FakePrompt;
    use notchling_engine::reply::read::read_shell_output;
    let screen = shot("running-lines.txt");
    let refs: Vec<&str> = screen.iter().map(String::as_str).collect();
    let mut pty = FakePrompt::new(&refs);
    let out = read_shell_output(&mut pty, "npm run build");
    assert_eq!(out.first().map(String::as_str), Some("> app@1.0.0 build"));
    assert_eq!(out.len(), 5);
    assert!(pty.writes.is_empty());
}

#[test]
fn a_session_still_waking_has_no_lines() {
    use notchling_engine::reply::fake::FakePrompt;
    use notchling_engine::reply::read::read_shell_output;
    let screen = shot("running-lines.txt");
    let refs: Vec<&str> = screen.iter().map(String::as_str).collect();
    let mut pty = FakePrompt::new(&refs).starting_for(60_000);
    assert!(read_shell_output(&mut pty, "npm run build").is_empty());
    assert!(pty.writes.is_empty());
}
