pub const RULE: char = '─';
pub const RULE_MIN: usize = 16;
pub const HOME_COL: usize = 2;
pub const MASK: char = '·';
pub const MARKS: [char; 2] = ['❯', '!'];
pub const MIN_PART: usize = 10;

pub const STARTING_MARKERS: [&str; 10] = [
    "Ctrl+Z to detach",
    "Waking session",
    "Session is starting",
    "is starting…",
    "is restarting…",
    "restarting it",
    "resuming it…",
    "to resume…",
    "Agent is updating",
    "Migrating job",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub held: Vec<String>,
    pub empty: bool,
}

fn squeezed(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn letters(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).collect()
}

fn strip_chips(text: &str) -> (String, usize) {
    let mut out = String::new();
    let mut chips = 0;
    let mut rest = text;
    while let Some(at) = rest.find("[Image#") {
        out.push_str(&rest[..at]);
        let tail = &rest[at + "[Image#".len()..];
        let digits = tail.chars().take_while(char::is_ascii_digit).count();
        match tail[digits..].strip_prefix(']') {
            Some(after) if digits > 0 => {
                chips += 1;
                rest = after;
            }
            _ => {
                out.push_str("[Image#");
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    (out, chips)
}

const PICTURE_EXTENSIONS: [&str; 5] = [".png", ".jpg", ".jpeg", ".gif", ".webp"];
const PICTURES_MAX: usize = 8;

fn path_starts(text: &str) -> bool {
    let text = text.strip_prefix(['"', '\'']).unwrap_or(text);
    let mut chars = text.chars();
    match chars.next() {
        Some('/') => true,
        Some(c) if c.is_ascii_alphabetic() => {
            chars.next() == Some(':') && chars.next() == Some('\\')
        }
        _ => false,
    }
}

fn path_pieces(line: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut from = 0;
    for (at, c) in line.char_indices() {
        if c == ' ' && path_starts(&line[at + 1..]) {
            pieces.push(&line[from..at]);
            from = at + 1;
        }
    }
    pieces.push(&line[from..]);
    pieces.retain(|piece| !piece.trim().is_empty());
    pieces
}

fn is_picture(piece: &str) -> bool {
    let piece = piece.trim();
    if !path_starts(piece) {
        return false;
    }
    let bare = match piece.as_bytes() {
        [first @ (b'"' | b'\''), .., last] if first == last => &piece[1..piece.len() - 1],
        _ => piece,
    };
    let bare = bare.to_ascii_lowercase();
    PICTURE_EXTENSIONS.iter().any(|ext| bare.ends_with(ext))
}

struct Message {
    pieces: Vec<(String, bool)>,
}

impl Message {
    fn of(text: &str) -> Message {
        let mut pieces = Vec::new();
        for line in text.split('\n') {
            let parts = path_pieces(line);
            if parts.len() > 1 && !parts.iter().all(|part| path_starts(part.trim_start())) {
                pieces.push((letters(line), false));
                continue;
            }
            pieces.extend(parts.iter().map(|part| (letters(part), is_picture(part))));
        }
        Message { pieces }
    }

    fn pictures(&self) -> usize {
        self.pieces.iter().filter(|(_, picture)| *picture).count()
    }

    fn kept(&self, gone: impl Fn(usize) -> bool) -> String {
        let mut nth = 0;
        let mut kept = String::new();
        for (words, picture) in &self.pieces {
            let read = *picture && gone(nth);
            nth += usize::from(*picture);
            if !read {
                kept.push_str(words);
            }
        }
        kept
    }

    fn without(&self, chips: usize) -> Vec<String> {
        let pictures = self.pictures();
        if chips > pictures {
            return Vec::new();
        }
        if pictures > PICTURES_MAX {
            return match chips {
                0 => vec![self.kept(|_| false)],
                n if n == pictures => vec![self.kept(|_| true)],
                _ => Vec::new(),
            };
        }
        (0..1usize << pictures)
            .filter(|mask| mask.count_ones() as usize == chips)
            .map(|mask| self.kept(|nth| mask >> nth & 1 == 1))
            .collect()
    }
}

fn folded(typed: &str) -> bool {
    typed.starts_with("[Pastedtext#") && typed.ends_with(']') && typed.matches(']').count() == 1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    Empty,
    Part,
    Whole,
    Folded,
    Foreign,
}

impl Input {
    pub fn typed(&self) -> String {
        let mut rows: Vec<&str> = self
            .held
            .iter()
            .enumerate()
            .map(|(n, row)| match n {
                0 => row
                    .strip_prefix(MARKS)
                    .unwrap_or(row)
                    .trim_start_matches(char::is_whitespace),
                _ => row.trim_start_matches(char::is_whitespace),
            })
            .collect();
        while rows.last().is_some_and(|row| row.is_empty()) {
            rows.pop();
        }
        rows.join("\n")
    }

    fn shown(&self) -> String {
        self.held.iter().map(|row| squeezed(row)).collect()
    }

    pub fn leftover_of(&self, earlier: &[String]) -> bool {
        let (shown, chips) = strip_chips(&self.shown());
        let typed: String = shown.chars().skip(1).collect();
        if self.empty || folded(&typed) || folded(&shown) {
            return false;
        }
        let have = letters(&shown);
        let size = have.chars().count();
        size > 0
            && earlier.iter().any(|text| {
                let mut wants = Message::of(text).without(chips);
                if chips > 0 {
                    wants.push(letters(text));
                }
                wants.iter().any(|want| {
                    have == *want
                        || (size >= MIN_PART && (want.starts_with(&have) || want.ends_with(&have)))
                })
            })
    }

    pub fn shows(&self, text: &str) -> bool {
        let want = squeezed(text);
        if want.is_empty() {
            return false;
        }
        let marked: String = self.held.iter().map(|row| squeezed(row)).collect();
        let typed: String = marked.chars().skip(1).collect();
        want == typed || want == marked
    }

    pub fn holds(&self, text: &str) -> bool {
        !self.empty || self.shows(text)
    }

    pub fn carries(&self, text: &str) -> bool {
        match self.fill(text) {
            Fill::Whole | Fill::Folded => true,
            Fill::Empty => false,
            Fill::Part | Fill::Foreign => {
                let (shown, chips) = strip_chips(&self.shown());
                let message = Message::of(text);
                let have = letters(&shown);
                message
                    .without(chips.min(message.pictures()))
                    .iter()
                    .any(|want| !want.is_empty() && have.contains(want))
            }
        }
    }

    pub fn fill(&self, text: &str) -> Fill {
        if self.empty && !self.shows(text) {
            return Fill::Empty;
        }
        let (shown, chips) = strip_chips(&self.shown());
        let message = Message::of(text);
        if chips > message.pictures() {
            return Fill::Foreign;
        }
        let typed: String = shown.chars().skip(1).collect();
        if folded(&typed) || folded(&shown) {
            return Fill::Folded;
        }
        let wants = message.without(chips);
        if chips == 0 && wants.iter().all(String::is_empty) {
            return Fill::Whole;
        }
        let have = letters(&shown);
        if wants.contains(&have) {
            Fill::Whole
        } else if wants.iter().any(|want| match have.is_empty() {
            true => chips > 0 && !want.is_empty(),
            false => want.starts_with(&have),
        }) {
            Fill::Part
        } else {
            Fill::Foreign
        }
    }
}

pub fn typed_text(lines: &[String], cursor: Option<(usize, usize)>) -> Option<String> {
    let input = read(lines, cursor).filter(|input| !input.empty)?;
    let text = input.typed();
    (!text.trim().is_empty()).then_some(text)
}

pub fn masked(lines: &[String]) -> Vec<String> {
    let Some((top, bottom)) = span(lines) else {
        return lines.to_vec();
    };
    let mask = |row: &String, first: bool| {
        row.chars()
            .enumerate()
            .map(
                |(n, c)| match c.is_whitespace() || (first && n == 0 && MARKS.contains(&c)) {
                    true => c,
                    false => MASK,
                },
            )
            .collect()
    };
    lines
        .iter()
        .enumerate()
        .map(|(i, row)| match i > top && i < bottom {
            true => mask(row, i == top + 1),
            false => row.clone(),
        })
        .collect()
}

pub fn last_row(lines: &[String]) -> Option<&String> {
    lines.iter().rev().find(|l| !l.trim().is_empty())
}

pub fn starting(lines: &[String], cursor: Option<(usize, usize)>) -> bool {
    let waits = |row: &String| STARTING_MARKERS.iter().any(|m| row.contains(m));
    let typed = read(lines, None).is_some_and(|input| !input.empty);
    !focused(lines, cursor) && !typed && last_row(lines).is_some_and(waits)
}

pub fn focused(lines: &[String], cursor: Option<(usize, usize)>) -> bool {
    match (span(lines), cursor) {
        (Some((top, bottom)), Some((row, _))) => top < row && row < bottom,
        _ => false,
    }
}

pub fn is_rule(line: &str) -> bool {
    line.chars().take_while(|c| *c == RULE).count() >= RULE_MIN
}

pub fn span(lines: &[String]) -> Option<(usize, usize)> {
    let bottom = lines.iter().rposition(|l| is_rule(l))?;
    let top = lines[..bottom].iter().rposition(|l| is_rule(l))?;
    (bottom - top > 1).then_some((top, bottom))
}

fn bare(held: &[String]) -> bool {
    match held {
        [only] => only.chars().skip(1).all(char::is_whitespace),
        _ => false,
    }
}

pub fn read(lines: &[String], cursor: Option<(usize, usize)>) -> Option<Input> {
    let (top, bottom) = span(lines)?;
    let held: Vec<String> = lines[top + 1..bottom]
        .iter()
        .map(|l| l.trim_end().to_string())
        .collect();
    let home = cursor.is_some_and(|(row, col)| row == top + 1 && col <= HOME_COL);
    Some(Input {
        empty: home || bare(&held),
        held,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|l| l.to_string()).collect()
    }

    const RULE_LINE: &str = "────────────────────────────────";

    #[test]
    fn the_box_is_what_the_last_two_rules_hold() {
        let screen = lines(&[
            "⏺ ok",
            "",
            RULE_LINE,
            "❯ hello   ",
            "  there",
            RULE_LINE,
            "  ? for shortcuts",
        ]);
        assert_eq!(span(&screen), Some((2, 5)));
        let input = read(&screen, None).unwrap();
        assert_eq!(input.held, lines(&["❯ hello", "  there"]));
        assert!(!input.empty);
    }

    #[test]
    fn a_box_with_only_its_prompt_mark_is_empty() {
        for mark in ["❯ ", "❯", "! ", ""] {
            let screen = lines(&[RULE_LINE, mark, RULE_LINE]);
            assert!(read(&screen, None).unwrap().empty, "{mark:?}");
        }
        let typed = lines(&[RULE_LINE, "❯ x", RULE_LINE]);
        assert!(!read(&typed, None).unwrap().empty);
        let two = lines(&[RULE_LINE, "❯ ", "  x", RULE_LINE]);
        assert!(!read(&two, None).unwrap().empty);
    }

    fn box_of(rows: &[&str]) -> Input {
        let mut screen = vec![RULE_LINE.to_string()];
        screen.extend(rows.iter().map(|r| r.to_string()));
        screen.push(RULE_LINE.to_string());
        read(&screen, None).unwrap()
    }

    #[test]
    fn what_the_box_holds_is_told_against_the_message_that_went_in() {
        let text = "Fix the \"tests\" -- now! \u{1F44D}";
        assert_eq!(box_of(&["❯"]).fill(text), Fill::Empty);
        assert_eq!(
            box_of(&["❯\u{a0}Fix the \"tests\" --", "  now!"]).fill(text),
            Fill::Whole,
            "wrapping, spacing, punctuation and emoji do not matter"
        );
        assert_eq!(box_of(&["❯ Fix the tests now"]).fill(text), Fill::Whole);
        assert_eq!(box_of(&["❯ Fix the"]).fill(text), Fill::Part);
        assert_eq!(
            box_of(&["❯ Fix the tests now and more"]).fill(text),
            Fill::Foreign
        );
        assert_eq!(box_of(&["❯ a draft of my own"]).fill(text), Fill::Foreign);
        assert_eq!(box_of(&["❯ ?"]).fill(text), Fill::Foreign);
        assert_eq!(
            box_of(&["❯\u{a0}[Pasted text #7 +39 lines]"]).fill(text),
            Fill::Folded
        );
        assert_eq!(
            box_of(&["❯ [Pasted text #7 +39 lines] and mine"]).fill(text),
            Fill::Foreign,
            "words beside a folded paste are somebody else's"
        );
    }

    #[test]
    fn a_box_carries_the_message_even_with_more_typed_beside_it() {
        let text = "Fix the tests";
        assert!(box_of(&["❯ Fix the tests"]).carries(text));
        assert!(box_of(&["❯ Fix the tests and my words"]).carries(text));
        assert!(box_of(&["❯ my words Fix the tests"]).carries(text));
        assert!(!box_of(&["❯ my words"]).carries(text));
        assert!(!box_of(&["❯ Fix the"]).carries(text));
        assert!(!box_of(&["❯"]).carries(text));
    }

    #[test]
    fn a_message_of_no_letters_is_told_by_the_box_not_being_empty() {
        assert_eq!(box_of(&["❯ \u{1F44D}"]).fill("\u{1F44D}"), Fill::Whole);
        assert_eq!(box_of(&["❯"]).fill("\u{1F44D}"), Fill::Empty);
    }

    #[test]
    fn a_hint_in_the_box_is_empty_while_the_cursor_sits_at_its_start() {
        let screen = lines(&[
            "⏺ ok",
            RULE_LINE,
            "❯ Try \"how do I log an error?\"",
            RULE_LINE,
        ]);
        assert!(
            !read(&screen, None).unwrap().empty,
            "words alone look typed"
        );
        assert!(read(&screen, Some((2, 2))).unwrap().empty);
        assert!(!read(&screen, Some((2, 30))).unwrap().empty);
        assert!(
            !read(&screen, Some((1, 2))).unwrap().empty,
            "another row's start"
        );
    }

    #[test]
    fn typed_text_is_never_empty_wherever_it_ends() {
        let screen = lines(&[RULE_LINE, "❯ one", "  two", "  three", RULE_LINE]);
        assert!(!read(&screen, Some((3, 7))).unwrap().empty);
        let shell = lines(&[RULE_LINE, "! ls -la", RULE_LINE, "  ! for shell mode"]);
        assert!(!read(&shell, Some((1, 8))).unwrap().empty);
    }

    #[test]
    fn a_named_session_draws_its_name_in_the_top_rule() {
        let top = format!("{RULE_LINE} menuprobe2 ─");
        let screen = lines(&[&top, "❯ ", RULE_LINE, "  status"]);
        assert!(read(&screen, Some((1, 2))).unwrap().empty);
    }

    #[test]
    fn the_attach_waiting_for_a_session_to_start_is_told_by_its_last_row() {
        let wait = lines(&[
            "⏺ ok",
            "",
            RULE_LINE,
            "❯",
            RULE_LINE,
            "  Session is starting — showing its transcript until it appears. Ctrl+Z to detach",
        ]);
        assert!(starting(&wait, None));
        assert!(
            starting(&wait, Some((5, 81))),
            "its cursor is under the box"
        );
        assert!(
            read(&wait, None).unwrap().empty,
            "it draws a box of its own"
        );
        assert!(!focused(&wait, None));
        assert!(!focused(&wait, Some((5, 81))));
        for notice in [
            "Waking session ab12cd34…",
            "Session ab12cd34 was interrupted while unattended — resuming it…",
            "Session ab12cd34's terminal host died — restarting it on a fresh one…",
            "Session ab12cd34 ended while the background service was off; looking for its saved conversation to resume…",
            "Agent is updating to the new Claude Code… (7s)",
            "Migrating job to attachable PTY…",
            "Session is restarting…",
            "Claude is starting…",
            "  Session is starting — it will appear once ready. Ctrl+Z to detach",
            "  Waiting for session to redraw… Ctrl+Z to detach",
            "   Session can't redraw right now — Ctrl+Z to detach",
            "  Session not responding — restarting it…",
        ] {
            let screen = lines(&["", notice, ""]);
            assert!(starting(&screen, Some((2, 0))), "{notice}");
        }
        let told = lines(&[
            "⏺ The attach says: Session is starting — showing its transcript until it appears.",
            RULE_LINE,
            "❯ ",
            RULE_LINE,
            "  ? for shortcuts",
        ]);
        assert!(!starting(&told, None), "the same words in the conversation");
        assert!(!starting(&[], None));
    }

    #[test]
    fn a_prompt_with_the_cursor_in_its_box_is_never_starting() {
        let prompt = lines(&[
            "⏺ ok",
            RULE_LINE,
            "❯ ",
            RULE_LINE,
            "  1 MCP server is starting…",
        ]);
        assert!(!starting(&prompt, Some((2, 2))));
        assert!(starting(&prompt, None), "only the cursor tells them apart");
        let typed = lines(&[RULE_LINE, "❯ hi", RULE_LINE, "  1 MCP server is starting…"]);
        assert!(!starting(&typed, None), "the attach's own box is bare");
    }

    #[test]
    fn the_cursor_is_in_the_box_only_between_its_rules() {
        let screen = lines(&["⏺ ok", RULE_LINE, "❯ one", "  two", RULE_LINE, "  status"]);
        assert!(focused(&screen, Some((2, 5))));
        assert!(focused(&screen, Some((3, 5))));
        for outside in [(0, 0), (1, 0), (4, 0), (5, 3)] {
            assert!(!focused(&screen, Some(outside)), "{outside:?}");
        }
        assert!(!focused(&screen, None));
        assert!(!focused(&lines(&["no box"]), Some((0, 0))));
    }

    #[test]
    fn a_box_shows_a_message_whatever_its_wrapping_and_spaces() {
        let one = read(
            &lines(&[RULE_LINE, "❯\u{a0}hello  there", RULE_LINE]),
            Some((1, 2)),
        );
        let one = one.unwrap();
        assert!(one.empty, "the cursor says nothing was typed");
        assert!(one.shows("hello there"));
        assert!(one.holds("hello there"));
        assert!(!one.shows("hello"), "a part of it is not the message");
        assert!(!one.shows("hello there, again"));
        assert!(!one.holds("another"));
        let wrapped = read(
            &lines(&[
                RULE_LINE,
                "❯ a long line that the box",
                "  wraps here",
                RULE_LINE,
            ]),
            None,
        )
        .unwrap();
        assert!(wrapped.shows("a long line that the box wraps here"));
        assert!(wrapped.shows("a long line that the box\nwraps here\r\n"));
        let shell = read(&lines(&[RULE_LINE, "!\u{a0}ls -la", RULE_LINE]), None).unwrap();
        assert!(shell.shows("!ls -la"));
        let hint = read(
            &lines(&[RULE_LINE, "❯ Try \"how do I log an error?\"", RULE_LINE]),
            Some((1, 2)),
        )
        .unwrap();
        assert!(!hint.holds("y"));
        assert!(
            hint.shows("Try \"how do I log an error?\""),
            "only a hint that is the whole message reads as it"
        );
        assert!(!hint.shows(""));
        assert!(!hint.shows(" \n"));
    }

    #[test]
    fn masking_hides_what_the_box_holds_and_nothing_else() {
        let screen = lines(&[
            "⏺ ok",
            RULE_LINE,
            "❯ say hi",
            "  to them",
            RULE_LINE,
            "  status",
        ]);
        assert_eq!(
            masked(&screen),
            lines(&[
                "⏺ ok",
                RULE_LINE,
                "❯ ··· ··",
                "  ·· ····",
                RULE_LINE,
                "  status"
            ])
        );
        let none = lines(&["Resume a session", "  1. fix-hook"]);
        assert_eq!(masked(&none), none);
        let markless = lines(&[RULE_LINE, "say hi", RULE_LINE]);
        assert_eq!(masked(&markless)[1], "··· ··", "no mark, nothing kept");
        let shell = lines(&[RULE_LINE, "! ls", RULE_LINE]);
        assert_eq!(masked(&shell)[1], "! ··");
    }

    #[test]
    fn one_rule_or_none_is_no_box() {
        assert_eq!(read(&lines(&["> ", "  ? for shortcuts"]), None), None);
        assert_eq!(
            read(
                &lines(&[RULE_LINE, " Bash command", " Do you want to proceed?"]),
                Some((1, 0))
            ),
            None
        );
        assert_eq!(read(&lines(&[RULE_LINE, RULE_LINE]), None), None);
        assert_eq!(read(&[], None), None);
    }

    #[test]
    fn a_short_or_indented_run_of_dashes_is_not_a_rule() {
        assert!(!is_rule("───────"));
        assert!(!is_rule("  ────────────────────────────────"));
        assert!(!is_rule("- - - - - - - - - - - - - - - - -"));
        assert!(is_rule(RULE_LINE));
    }

    const WORDS: &str = "i want click refresh it will refresh is it ok?";
    const SHOT: &str = "/Users/me/Library/Application Support/Notchling/drops/1794723941465/Screenshot 2026-11-15 at 1.25.35 PM.png";

    fn with_shot(words: &str) -> String {
        format!("{words}\n\n'{SHOT}'")
    }

    #[test]
    fn a_picture_path_the_session_turned_into_a_chip_is_still_the_message() {
        let text = with_shot(WORDS);
        let seen = box_of(&["❯\u{a0}[Image #48]i want click refresh it will refresh is it ok?"]);
        assert_eq!(seen.fill(&text), Fill::Whole);
        assert!(seen.carries(&text));
        let narrow = box_of(&[
            "❯\u{a0}[Image #48]i want click refresh it will",
            "  refresh is it ok?",
        ]);
        assert_eq!(
            narrow.fill(&text),
            Fill::Whole,
            "wrapping and the chip's number do not matter"
        );
        let two = "Say only the word race.\n\n/tmp/shots/one.png /tmp/shots/two.jpg";
        let both = box_of(&["❯\u{a0}[Image #5] [Image #6]Say only the word race."]);
        assert_eq!(both.fill(two), Fill::Whole);
        let before = "'/tmp/a b.PNG'\n\nlook at this";
        assert_eq!(
            box_of(&["❯\u{a0}[Image #1]look at this"]).fill(before),
            Fill::Whole
        );
        assert_eq!(
            box_of(&["❯\u{a0}[Image #1]"]).fill("'/tmp/a b.png'"),
            Fill::Whole
        );
    }

    #[test]
    fn chips_that_are_in_before_their_words_are_a_part_of_the_message() {
        let text = with_shot(WORDS);
        assert_eq!(box_of(&["❯\u{a0}[Image #48]"]).fill(&text), Fill::Part);
        assert_eq!(
            box_of(&["❯\u{a0}[Image #48]i want click"]).fill(&text),
            Fill::Part
        );
        assert_eq!(box_of(&["❯\u{a0}[Image #48]"]).fill("hello"), Fill::Foreign);
    }

    #[test]
    fn a_picture_the_session_could_not_read_stays_a_path_in_the_box() {
        let text = with_shot(WORDS);
        let unread = box_of(&[
            "❯\u{a0}i want click refresh it will refresh is it ok?",
            &format!("  '{SHOT}'"),
        ]);
        assert_eq!(
            unread.fill(&text),
            Fill::Whole,
            "the whole paste went in as it was"
        );
        let two = "look\n\n/tmp/a.png /tmp/b.png";
        assert_eq!(
            box_of(&["❯\u{a0}[Image #3]look", "  /tmp/b.png"]).fill(two),
            Fill::Whole,
            "one of two was read"
        );
        assert_eq!(
            box_of(&["❯\u{a0}[Image #3]look", "  /tmp/a.png"]).fill(two),
            Fill::Whole
        );
        assert_eq!(
            box_of(&["❯\u{a0}[Image #3]look", "  /tmp/c.png"]).fill(two),
            Fill::Foreign
        );
    }

    #[test]
    fn many_pictures_are_all_read_or_none_of_them() {
        let paths: Vec<String> = (0..9).map(|n| format!("/tmp/p{n}.png")).collect();
        let text = format!("look\n\n{}", paths.join(" "));
        let chips: String = (1..=9).map(|n| format!("[Image #{n}]")).collect();
        let all = format!("❯\u{a0}{chips}look");
        assert_eq!(box_of(&[&all]).fill(&text), Fill::Whole);
        assert_eq!(
            box_of(&["❯\u{a0}[Image #1]look"]).fill(&text),
            Fill::Foreign
        );
        let some = format!("❯\u{a0}[Image #1] [Image #2]look {}", paths[2..].join(" "));
        assert_eq!(box_of(&[&some]).fill(&text), Fill::Foreign);
    }

    #[test]
    fn a_chip_that_is_not_one_of_our_pictures_is_somebody_elses() {
        let text = with_shot(WORDS);
        assert_eq!(
            box_of(&["❯\u{a0}[Image #9]Fix the tests"]).fill("Fix the tests"),
            Fill::Foreign,
            "a message with no picture"
        );
        assert_eq!(
            box_of(&["❯\u{a0}[Image #1] [Image #2]i want click refresh it will refresh is it ok?"])
                .fill(&text),
            Fill::Foreign,
            "more chips than pictures"
        );
        assert_eq!(
            box_of(&["❯\u{a0}[Image #48]i want click refresh it will refresh is it ok? and mine"])
                .fill(&text),
            Fill::Foreign
        );
        assert_eq!(
            box_of(&["❯\u{a0}[Image #48]a draft of my own"]).fill(&text),
            Fill::Foreign
        );
        let not_a_picture = "see /tmp/notes.png now";
        assert_eq!(
            box_of(&["❯\u{a0}[Image #1]see now"]).fill(not_a_picture),
            Fill::Foreign,
            "a path inside a sentence is never read as a picture"
        );
    }

    #[test]
    fn a_long_message_with_a_picture_folds_after_its_chip() {
        let text = with_shot(&"word ".repeat(300));
        let seen = box_of(&["❯\u{a0}[Image #2] [Pasted text #3 +20 lines]"]);
        assert_eq!(seen.fill(&text), Fill::Folded);
        assert!(seen.carries(&text));
        let foreign = box_of(&["❯\u{a0}[Image #2] [Image #3] [Pasted text #4 +20 lines]"]);
        assert_eq!(foreign.fill(&text), Fill::Foreign);
    }

    #[test]
    fn a_box_with_chips_carries_the_message_with_more_typed_beside_it() {
        let text = with_shot(WORDS);
        let more =
            box_of(&["❯\u{a0}[Image #48]i want click refresh it will refresh is it ok?more"]);
        assert_eq!(more.fill(&text), Fill::Foreign);
        assert!(more.carries(&text));
        let own = box_of(&["❯\u{a0}[Image #48]my own words"]);
        assert!(!own.carries(&text));
        assert!(!box_of(&["❯"]).carries(&text));
    }

    #[test]
    fn our_own_message_left_with_its_chips_is_a_leftover_even_when_short() {
        let earlier = [with_shot("ok")];
        assert!(box_of(&["❯\u{a0}[Image #47]ok"]).leftover_of(&earlier));
        let long = [with_shot(WORDS)];
        let left = box_of(&["❯\u{a0}[Image #47]i want click refresh it will refresh is it ok?"]);
        assert!(left.leftover_of(&long));
        assert!(left.leftover_of(&[with_shot("ok"), with_shot(WORDS)]));
        assert!(!box_of(&["❯\u{a0}[Image #47]fine"]).leftover_of(&earlier));
        assert!(
            !box_of(&["❯\u{a0}[Image #47]"]).leftover_of(&[format!("'{SHOT}'")]),
            "a chip alone could be a picture the person pasted"
        );
        assert!(!box_of(&["❯\u{a0}[Image #47] [Pasted text #48 +20 lines]"]).leftover_of(&long));
    }

    #[test]
    fn the_recorded_chip_screens_read_as_the_message_that_made_them() {
        let two = "Say only the word race.\n\n/tmp/shots/one.png /tmp/shots/two.jpg";
        let recorded = Input {
            held: vec!["❯\u{a0}[Image #5] [Image #6]Say only the word race.".to_string()],
            empty: false,
        };
        assert_eq!(recorded.fill(two), Fill::Whole);
        assert_eq!(recorded.fill("Say only the word ok."), Fill::Foreign);
    }
}
