use serde_json::{json, Value};

use notchling_engine::bridge::net::files::iso8601;

pub const WORKING_JOB: &str = "bn000001";
pub const PROJECT: &str = "-Users-me-work-bench";
pub const WORKING_TRANSCRIPT: &str = "claude/projects/-Users-me-work-bench/bench-working.jsonl";

const LINE_GAP_MS: i64 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Plain,
    Tall,
    Busy,
}

pub fn files(turns: usize, now_ms: i64) -> Vec<(String, String)> {
    files_shaped(turns, now_ms, Shape::Plain)
}

pub fn files_shaped(turns: usize, now_ms: i64, shape: Shape) -> Vec<(String, String)> {
    let mut out = Vec::new();
    out.push(job(WORKING_JOB, "bench-working", "working", now_ms));
    out.push((
        WORKING_TRANSCRIPT.to_owned(),
        transcript("bench-working", turns, now_ms, shape),
    ));
    for n in 1..=3 {
        let (id, session) = (format!("bd00000{n}"), format!("bench-done-{n}"));
        let at = now_ms - n as i64 * 3_600_000;
        out.push(job(&id, &session, "done", at));
        out.push((
            format!("claude/projects/{PROJECT}/{session}.jsonl"),
            transcript(&session, 2, at, Shape::Plain),
        ));
    }
    out
}

fn job(id: &str, session: &str, state: &str, at_ms: i64) -> (String, String) {
    let state = json!({
        "state": state,
        "sessionId": session,
        "name": session.replace('-', " "),
        "cwd": "/Users/me/work/bench",
        "updatedAt": iso8601(at_ms),
        "linkScanPath": format!("claude/projects/{PROJECT}/{session}.jsonl"),
    });
    (format!("claude/jobs/{id}/state.json"), state.to_string())
}

fn transcript(session: &str, turns: usize, end_ms: i64, shape: Shape) -> String {
    let lines = turns as i64 * 4;
    let start = end_ms - lines * LINE_GAP_MS;
    let mut out = String::new();
    for turn in 0..turns {
        let at = |k: i64| iso8601(start + (turn as i64 * 4 + k) * LINE_GAP_MS);
        for line in turn_lines(session, turn, shape, &at) {
            out.push_str(&line.to_string());
            out.push('\n');
        }
    }
    out
}

fn turn_lines(session: &str, turn: usize, shape: Shape, at: &dyn Fn(i64) -> String) -> Vec<Value> {
    let tool = format!("{session}-t{turn}");
    let lines = if shape == Shape::Tall && turn % 7 == 3 {
        3000
    } else {
        20
    };
    let output = (1..=lines)
        .map(|i| {
            format!(
                "   Compiling bench-crate-{turn}-{i} v0.{i}.0 (/Users/me/work/bench/crates/c{i})"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut lines = vec![
        json!({
            "type": "user",
            "timestamp": at(0),
            "gitBranch": "bench",
            "message": {"role": "user", "content": format!(
                "Turn {turn}: run the checks on the parser crate, then tell me which of them failed and why, in a few lines."
            )},
        }),
        json!({
            "type": "assistant",
            "timestamp": at(1),
            "effort": "high",
            "message": {
                "id": format!("{session}-a{turn}"),
                "model": "claude-opus-5-5",
                "content": [{
                    "type": "tool_use",
                    "id": tool,
                    "name": "Bash",
                    "input": {"command": "cargo test -p parser 2>&1 | tail -30", "description": "Run the parser tests"},
                }],
                "usage": {"input_tokens": 1200, "output_tokens": 40},
            },
        }),
        json!({
            "type": "user",
            "timestamp": at(2),
            "message": {"role": "user", "content": [{
                "type": "tool_result",
                "tool_use_id": tool,
                "content": output,
            }]},
        }),
        json!({
            "type": "assistant",
            "timestamp": at(3),
            "effort": "high",
            "message": {
                "id": format!("{session}-b{turn}"),
                "model": "claude-opus-5-5",
                "content": [{"type": "text", "text": if shape == Shape::Tall && turn % 4 == 1 { long_reply(turn) } else { reply(turn) }}],
                "usage": {"input_tokens": 1400, "output_tokens": 180},
            },
        }),
    ];
    if shape == Shape::Busy && turn % 6 == 2 {
        let extra: Vec<Value> = (0..60)
            .flat_map(|k| busy_call(session, turn, k, at(2)))
            .collect();
        lines.splice(3..3, extra);
    }
    lines
}

fn busy_call(session: &str, turn: usize, k: usize, when: String) -> [Value; 2] {
    let tool = format!("{session}-x{turn}-{k}");
    [
        json!({
            "type": "assistant",
            "timestamp": when,
            "effort": "high",
            "message": {
                "id": format!("{tool}-m"),
                "model": "claude-opus-5-5",
                "content": [{
                    "type": "tool_use",
                    "id": tool,
                    "name": "Read",
                    "input": {"file_path": format!("/Users/me/work/bench/crates/parser/src/part{k}.rs")},
                }],
                "usage": {"input_tokens": 1300, "output_tokens": 30},
            },
        }),
        json!({
            "type": "user",
            "timestamp": when,
            "message": {"role": "user", "content": [{
                "type": "tool_result",
                "tool_use_id": tool,
                "content": format!("     1\tpub fn part{k}() {{}}\n     2\t// turn {turn}"),
            }]},
        }),
    ]
}

fn reply(turn: usize) -> String {
    format!(
        "## Result of turn {turn}\n\n\
         The parser tests ran to the end and one of them failed: the table reader keeps a trailing \
         space in the last cell, so the comparison against the expected row never matches.\n\n\
         - the reader trims only the first cell\n\
         - the writer pads every cell to the widest one\n\
         - the test compares the padded text\n\n\
         ```rust\n\
         fn cell(text: &str) -> &str {{\n\
         \x20   let text = text.trim_start();\n\
         \x20   match text.find('|') {{\n\
         \x20       Some(end) => text[..end].trim_end(),\n\
         \x20       None => text.trim_end(),\n\
         \x20   }}\n\
         }}\n\
         // used by every row of every table\n\
         ```\n\n\
         | Check | Result | Time |\n\
         | --- | --- | --- |\n\
         | fmt | ok | 1s |\n\
         | tests | 1 failed | {turn}s |\n\
         | clippy | ok | 41s |\n"
    )
}

fn long_reply(turn: usize) -> String {
    (0..40)
        .map(|section| reply(turn * 100 + section))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use notchling_engine::transcript::conversation::Parser;

    use notchling_engine::transcript::model::ItemKind;

    use super::*;
    use crate::chat::data::DataRoot;
    use crate::testutil::Scratch;

    const T: i64 = 1_794_000_000_000;

    #[test]
    fn the_fixture_lists_one_working_job_first() {
        let dir = Scratch::new("bench-fixture");
        for (path, text) in files(5, T) {
            dir.write(&path, &text);
        }
        let sessions = DataRoot::fixture(dir.path()).sessions(T);
        let names: Vec<&str> = sessions.iter().map(|s| s.agent.name.as_str()).collect();
        assert_eq!(sessions.len(), 4, "{names:?}");
        assert_eq!(names[0], "bench working", "{names:?}");
        assert!(
            sessions[0]
                .transcript
                .as_ref()
                .is_some_and(|path| path.is_file()),
            "the working job's transcript is found"
        );
    }

    #[test]
    fn a_long_fixture_fills_the_conversation() {
        let (_, text) = files(200, T)
            .into_iter()
            .find(|(path, _)| path == WORKING_TRANSCRIPT)
            .expect("the working transcript");
        let mut parser = Parser::default();
        for line in text.lines() {
            parser.consume(line.as_bytes());
        }
        let items = parser.conversation().items.len();
        assert!((400..=500).contains(&items), "{items} items");
    }

    #[test]
    fn a_tall_fixture_has_rows_far_taller_than_a_window() {
        let (_, text) = files_shaped(40, T, Shape::Tall)
            .into_iter()
            .find(|(path, _)| path == WORKING_TRANSCRIPT)
            .expect("the working transcript");
        let plain = files(40, T)
            .into_iter()
            .find(|(path, _)| path == WORKING_TRANSCRIPT)
            .expect("the working transcript")
            .1;
        assert!(
            text.len() > plain.len() * 5,
            "{} vs {}",
            text.len(),
            plain.len()
        );
        let mut parser = Parser::default();
        for line in text.lines() {
            parser.consume(line.as_bytes());
        }
        let longest = parser
            .conversation()
            .items
            .iter()
            .map(|item| format!("{:?}", item.kind).len())
            .max()
            .unwrap_or(0);
        assert!(longest > 20_000, "{longest}");
    }

    #[test]
    fn a_busy_fixture_has_a_group_of_sixty_calls() {
        let (_, text) = files_shaped(40, T, Shape::Busy)
            .into_iter()
            .find(|(path, _)| path == WORKING_TRANSCRIPT)
            .expect("the working transcript");
        let mut parser = Parser::default();
        for line in text.lines() {
            parser.consume(line.as_bytes());
        }
        let largest = parser
            .conversation()
            .items
            .iter()
            .map(|item| match &item.kind {
                ItemKind::Tools(calls) => calls.len(),
                _ => 0,
            })
            .max()
            .unwrap_or(0);
        assert!(largest >= 60, "{largest}");
    }

    #[test]
    fn the_fixture_is_the_same_every_time() {
        assert_eq!(files(3, T), files(3, T));
    }
}
