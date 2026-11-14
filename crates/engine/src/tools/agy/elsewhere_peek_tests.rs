use super::*;
use crate::sessions::testutil::TempDir;

const T0: &str = "2026-11-14T09:00:00Z";
const T1: &str = "2026-11-14T09:00:05Z";
const T2: &str = "2026-11-14T09:00:09Z";

fn line(index: u32, kind: &str, at: &str, extra: &str) -> String {
    format!(
        r#"{{"step_index":{index},"source":"MODEL","type":"{kind}","status":"DONE","created_at":"{at}"{extra}}}"#
    )
}

fn log() -> String {
    [
        line(
            0,
            "USER_INPUT",
            T0,
            r#","content":"<USER_REQUEST>\nCheck the images.\n</USER_REQUEST>""#,
        ),
        line(
            1,
            "PLANNER_RESPONSE",
            T1,
            r#","input_tokens":900,"cache_read_tokens":100,"output_tokens":20,"tool_calls":[{"name":"read_file","args":{"Path":"\"/w/a.md\""}}]"#,
        ),
        line(
            2,
            "PLANNER_RESPONSE",
            T2,
            r#","input_tokens":1000,"cache_read_tokens":500,"output_tokens":50,"tool_calls":[{"name":"run_command","args":{"CommandLine":"\"ls\"","toolSummary":"\"List it\""}}]"#,
        ),
    ]
    .join("\n")
        + "\n"
}

#[test]
fn peek_reads_tokens_and_the_newest_step() {
    let dir = TempDir::new("peek-ok");
    let path = dir.write("log.jsonl", &log());
    let peeked = peek(&path, true).expect("a log");
    assert_eq!(peeked.context, Some(1550));
    let recent = peeked.recent.expect("the recent steps");
    let titles: Vec<String> = recent
        .items
        .iter()
        .flat_map(|i| match &i.kind {
            ItemKind::Tools(calls) => calls.iter().map(|c| c.title.clone()).collect(),
            _ => Vec::new(),
        })
        .collect();
    assert_eq!(titles, ["Read a.md", "List it"]);
    assert!(peek(&path, false).expect("a log").recent.is_none());
}

#[test]
fn a_log_with_no_planner_step_has_no_tokens() {
    let dir = TempDir::new("peek-none");
    let path = dir.write(
        "log.jsonl",
        &(line(0, "USER_INPUT", T0, r#","content":"hi""#) + "\n"),
    );
    assert_eq!(peek(&path, false).expect("a log").context, None);
}

#[test]
fn peek_of_a_missing_log_is_none() {
    let dir = TempDir::new("peek-missing");
    assert!(peek(&dir.path("gone.jsonl"), true).is_none());
}
