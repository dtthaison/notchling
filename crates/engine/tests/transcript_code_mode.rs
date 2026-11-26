mod common;

use common::*;
use notchling_engine::transcript::conversation;
use notchling_engine::transcript::model::*;
use notchling_engine::transcript::strict::{strict_claude, SkipKind};

fn calls(c: &Conversation) -> Vec<ToolCall> {
    c.items
        .iter()
        .flat_map(|i| match &i.kind {
            ItemKind::Tools(calls) => calls.clone(),
            _ => Vec::new(),
        })
        .collect()
}

fn named<'a>(calls: &'a [ToolCall], id: &str) -> &'a ToolCall {
    calls.iter().find(|c| c.id == id).expect(id)
}

fn read_fixture() -> Conversation {
    conversation::read(&s(&notchling_fixture("code-mode.jsonl")))
}

#[test]
fn a_repl_call_is_a_script_row_titled_by_its_description() {
    let all = calls(&read_fixture());
    let repl = named(&all, "toolu_REPL_ONE");
    assert_eq!(repl.kind, ToolKind::Repl);
    assert_eq!(repl.title, "List the project files");
    assert_eq!(repl.input, "await $`ls -la /work/demo | head -20`.text()");
}

#[test]
fn a_dispatched_repl_stays_running_until_its_event_settles() {
    let all = calls(&read_fixture());
    let running = named(&all, "toolu_REPL_THREE");
    assert!(!running.finished);
    assert!(running.output.is_none());
    assert!(!running.is_error);
}

#[test]
fn a_settled_event_finishes_the_repl_with_its_result() {
    let all = calls(&read_fixture());
    let repl = named(&all, "toolu_REPL_ONE");
    assert!(repl.finished);
    assert!(!repl.is_error);
    assert!(repl.output.as_deref().unwrap().contains("Cargo.toml"));
}

#[test]
fn an_error_event_fails_the_repl_and_shows_its_text() {
    let all = calls(&read_fixture());
    let repl = named(&all, "toolu_REPL_TWO");
    assert!(repl.finished);
    assert!(repl.is_error);
    assert_eq!(repl.output.as_deref(), Some("cargo: command not found"));
}

#[test]
fn inner_calls_nest_under_the_repl_of_their_eval() {
    let all = calls(&read_fixture());
    let one = named(&all, "repl_11111111-1111-4111-8111-111111111111");
    assert_eq!(one.parent.as_deref(), Some("toolu_REPL_ONE"));
    assert_eq!(one.kind, ToolKind::Shell);
    let read = named(&all, "repl_22222222-2222-4222-8222-222222222222");
    let build = named(&all, "repl_33333333-3333-4333-8333-333333333333");
    assert_eq!(read.parent.as_deref(), Some("toolu_REPL_TWO"));
    assert_eq!(build.parent.as_deref(), Some("toolu_REPL_TWO"));
    assert!(build.is_error);
    assert!(named(&all, "toolu_REPL_ONE").parent.is_none());
    let order: Vec<&str> = all.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        order,
        [
            "toolu_REPL_ONE",
            "repl_11111111-1111-4111-8111-111111111111",
            "toolu_REPL_TWO",
            "repl_22222222-2222-4222-8222-222222222222",
            "repl_33333333-3333-4333-8333-333333333333",
            "toolu_REPL_THREE"
        ]
    );
}

#[test]
fn code_mode_lines_leave_no_notice_no_queue_entry_and_no_extra_user_row() {
    let c = read_fixture();
    assert!(c.queued.is_empty(), "{:?}", c.queued);
    let users = c
        .items
        .iter()
        .filter(|i| matches!(i.kind, ItemKind::User { .. }))
        .count();
    assert_eq!(users, 2);
    assert!(!c
        .items
        .iter()
        .any(|i| matches!(i.kind, ItemKind::Notice { .. })));
    assert_eq!(c.facts.commands, 2);
}

#[test]
fn the_strict_pass_knows_every_code_mode_line_type() {
    let skips = strict_claude(&notchling_fixture("code-mode.jsonl"), 4_000_000);
    assert_eq!(skips.count("2.1.332", SkipKind::UnknownLine), 0);
    assert_eq!(skips.unknown_total(), 0);
}

#[test]
fn a_repl_result_without_dispatch_finishes_with_its_stdout_and_stderr() {
    let scratch = Scratch::new("repl-sync");
    let lines = [
        r#"{"type":"user","uuid":"u","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","uuid":"a","message":{"id":"m","role":"assistant","content":[{"type":"tool_use","id":"t1","name":"REPL","input":{"code":"1+1"}}]}}"#,
        r#"{"type":"user","uuid":"r","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"2","is_error":false}]},"toolUseResult":{"code":"1+1","result":"2","stdout":"out line","stderr":"warn line","evalId":1}}"#,
    ];
    let path = scratch.write("sync.jsonl", &(lines.join("\n") + "\n"));
    let all = calls(&conversation::read(&s(&path)));
    let repl = &all[0];
    assert_eq!(repl.kind, ToolKind::Repl);
    assert_eq!(repl.title, "Ran a script");
    assert!(repl.finished);
    assert_eq!(repl.output.as_deref(), Some("out line\nwarn line"));
}

#[test]
fn a_repl_event_with_logs_shows_the_log_before_the_result() {
    let scratch = Scratch::new("repl-log");
    let event = "<event kind=\\\"repl-eval\\\" at=\\\"2026-11-25T10:00:30.000Z\\\" eval=\\\"1\\\" ms=\\\"9\\\" status=\\\"settled\\\">\\n<log>step one</log>\\n<result>42 &lt; 43</result>\\n</event>";
    let lines = [
        r#"{"type":"user","uuid":"u","message":{"role":"user","content":"go"}}"#.to_string(),
        r#"{"type":"assistant","uuid":"a","message":{"id":"m","role":"assistant","content":[{"type":"tool_use","id":"t1","name":"REPL","input":{"code":"x","description":"Compute"}}]}}"#.to_string(),
        r#"{"type":"user","uuid":"r","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"(dispatched #1)","is_error":false}]},"toolUseResult":{"result":"(dispatched #1)","stdout":"","stderr":"","evalId":1,"asyncDispatched":true}}"#.to_string(),
        format!(r#"{{"type":"queue-operation","operation":"enqueue","content":"{event}"}}"#),
    ];
    let path = scratch.write("log.jsonl", &(lines.join("\n") + "\n"));
    let all = calls(&conversation::read(&s(&path)));
    assert!(all[0].finished);
    assert_eq!(all[0].output.as_deref(), Some("step one\n42 < 43"));
}

const USER_LINE: &str = r#"{"type":"user","uuid":"u","message":{"role":"user","content":"go"}}"#;

fn repl_call(id: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"a-{id}","message":{{"id":"m-{id}","role":"assistant","content":[{{"type":"tool_use","id":"{id}","name":"REPL","input":{{"code":"x","description":"Script {id}"}}}}]}}}}"#
    )
}

fn dispatched(id: &str, eval: u64) -> String {
    format!(
        r#"{{"type":"user","uuid":"r-{id}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"(dispatched #{eval})","is_error":false}}]}},"toolUseResult":{{"result":"(dispatched #{eval})","stdout":"","stderr":"","evalId":{eval},"asyncDispatched":true}}}}"#
    )
}

fn enqueue(event: &str) -> String {
    format!(r#"{{"type":"queue-operation","operation":"enqueue","content":"{event}"}}"#)
}

fn poll(events: &[&str]) -> String {
    let list: Vec<String> = events.iter().map(|e| format!("\"{e}\"")).collect();
    format!(
        r#"{{"type":"attachment","attachment":{{"type":"poll_events","envelopes":[{}]}}}}"#,
        list.join(",")
    )
}

fn inner_call(id: &str, command: &str) -> Vec<String> {
    vec![
        format!(
            r#"{{"type":"assistant","isVirtual":true,"uuid":"a-{id}","message":{{"id":"m-{id}","model":"<synthetic>","role":"assistant","content":[{{"type":"tool_use","id":"{id}","name":"Bash","input":{{"command":"{command}"}}}}]}}}}"#
        ),
        format!(
            r#"{{"type":"user","isVirtual":true,"uuid":"r-{id}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"","is_error":false}}]}}}}"#
        ),
    ]
}

fn event(eval: u64, status: &str, body: &str) -> String {
    format!(
        "<event kind=\\\"repl-eval\\\" at=\\\"2026-11-25T10:00:30.000Z\\\" eval=\\\"{eval}\\\" ms=\\\"9\\\" status=\\\"{status}\\\">\\n{body}\\n</event>"
    )
}

fn run(tag: &str, lines: Vec<String>) -> Vec<ToolCall> {
    let scratch = Scratch::new(tag);
    let mut all = vec![USER_LINE.to_string()];
    all.extend(lines);
    let path = scratch.write("t.jsonl", &(all.join("\n") + "\n"));
    calls(&conversation::read(&s(&path)))
}

#[test]
fn a_long_queued_prompt_that_splits_a_character_at_byte_200_does_not_panic() {
    let long = format!("Xin chào {}", "ệ".repeat(120));
    assert!(!long.is_char_boundary(200));
    let all = run(
        "repl-viet",
        vec![
            repl_call("t1"),
            dispatched("t1", 1),
            enqueue(&long),
            enqueue(&event(1, "settled", "<result>ok</result>")),
        ],
    );
    assert_eq!(all[0].output.as_deref(), Some("ok"));
}

fn settled_output(body: &str) -> ToolCall {
    let all = run(
        "repl-body",
        vec![
            repl_call("t1"),
            dispatched("t1", 1),
            enqueue(&event(1, "settled", body)),
        ],
    );
    all[0].clone()
}

#[test]
fn repeated_log_blocks_all_show_before_the_result() {
    let call = settled_output("<log>one</log>\\n<log>two</log>\\n<result>three</result>");
    assert_eq!(call.output.as_deref(), Some("one\ntwo\nthree"));
    assert!(!call.is_error);
}

#[test]
fn an_error_tag_inside_a_log_does_not_fail_the_script() {
    let call = settled_output("<log>saw <error>boom</error> here</log>\\n<result>ok</result>");
    assert!(call.finished);
    assert!(!call.is_error);
    assert_eq!(
        call.output.as_deref(),
        Some("saw <error>boom</error> here\nok")
    );
}

#[test]
fn a_top_level_error_element_fails_the_script() {
    let call = settled_output("<log>fine</log>\\n<error>bad</error>");
    assert!(call.is_error);
    assert_eq!(call.output.as_deref(), Some("fine\nbad"));
}

#[test]
fn an_event_end_tag_inside_the_output_does_not_cut_it() {
    let call = settled_output("<result>a </event> b</result>");
    assert!(call.finished);
    assert_eq!(call.output.as_deref(), Some("a </event> b"));
}

#[test]
fn entities_are_decoded_once() {
    let call = settled_output("<result>say &quot;hi&quot; &amp;lt; &gt; &#39;x&#39;</result>");
    assert_eq!(call.output.as_deref(), Some("say \"hi\" &lt; > 'x'"));
}

#[test]
fn a_greater_than_sign_inside_an_attribute_does_not_end_the_tag() {
    let all = run(
        "repl-attr",
        vec![
            repl_call("t1"),
            dispatched("t1", 1),
            enqueue("<event kind=\\\"repl-eval\\\" at=\\\"a>b\\\" eval=\\\"1\\\" status=\\\"settled\\\">\\n<result>ok</result>\\n</event>"),
        ],
    );
    assert!(all[0].finished);
    assert_eq!(all[0].output.as_deref(), Some("ok"));
}

#[test]
fn an_end_tag_that_carries_a_nonce_still_ends_the_event() {
    let all = run(
        "repl-nonce",
        vec![
            repl_call("t1"),
            repl_call("t2"),
            dispatched("t1", 1),
            dispatched("t2", 2),
            enqueue(
                "<event nonce=\\\"abc_1\\\" kind=\\\"repl-eval\\\" eval=\\\"1\\\" status=\\\"settled\\\">\\n<result>first</result>\\n</event nonce=\\\"abc_1\\\">\\n<event nonce=\\\"abc_2\\\" kind=\\\"repl-eval\\\" eval=\\\"2\\\" status=\\\"settled\\\">\\n<result>second</result>\\n</event nonce=\\\"abc_2\\\">",
            ),
        ],
    );
    assert_eq!(all[0].output.as_deref(), Some("first"));
    assert_eq!(all[1].output.as_deref(), Some("second"));
}

#[test]
fn a_status_other_than_settled_fails_the_script() {
    let all = run(
        "repl-status",
        vec![
            repl_call("t1"),
            dispatched("t1", 1),
            enqueue(&event(1, "failed", "<result>partial</result>")),
        ],
    );
    assert!(all[0].finished);
    assert!(all[0].is_error);
}

#[test]
fn a_json_event_body_gives_its_log_result_and_error() {
    let body = r#"{\"eval\":1,\"log\":[\"a\",\"b\"],\"result\":\"r\"}"#;
    let call = settled_output(body);
    assert_eq!(call.output.as_deref(), Some("a\nb\nr"));
    assert!(!call.is_error);
    let call = settled_output(r#"{\"eval\":1,\"error\":\"bad\"}"#);
    assert!(call.is_error);
    assert_eq!(call.output.as_deref(), Some("bad"));
}

fn big(body_head: &str) -> String {
    format!("{body_head}{}", "x".repeat(450_000))
}

#[test]
fn an_oversized_settle_line_still_finishes_the_script() {
    let line = enqueue(&event(1, "settled", &big("<result>start ")));
    assert!(line.len() > 400_000);
    let all = run("repl-big", vec![repl_call("t1"), dispatched("t1", 1), line]);
    assert!(all[0].finished);
    assert!(!all[0].is_error);
    let out = all[0].output.as_deref().unwrap();
    assert!(out.starts_with("start xxx"));
    assert!(out.ends_with("(too large to show)"));
    assert!(out.len() < 10_000);
}

#[test]
fn an_oversized_failed_settle_line_marks_the_script_failed() {
    let line = enqueue(&event(1, "settled", &big("<error>boom ")));
    let all = run(
        "repl-big-err",
        vec![repl_call("t1"), dispatched("t1", 1), line],
    );
    assert!(all[0].finished);
    assert!(all[0].is_error);
    assert!(all[0].output.as_deref().unwrap().starts_with("boom xxx"));
}

#[test]
fn an_oversized_poll_attachment_still_finishes_the_script() {
    let env = event(1, "settled", &big("<result>start "));
    let line = poll(&[&env]);
    assert!(line.len() > 400_000);
    let all = run(
        "repl-big-poll",
        vec![repl_call("t1"), dispatched("t1", 1), line],
    );
    assert!(all[0].finished);
    assert!(all[0].output.as_deref().unwrap().starts_with("start xxx"));
}

#[test]
fn an_oversized_line_for_another_eval_changes_nothing_else() {
    let line = enqueue(&event(7, "settled", &big("<result>start ")));
    let all = run(
        "repl-big-other",
        vec![repl_call("t1"), dispatched("t1", 1), line],
    );
    assert!(!all[0].finished);
}

#[test]
fn an_inner_call_after_a_real_tool_does_not_nest_under_an_old_script() {
    let mut lines = vec![repl_call("t1")];
    lines.push(
        r#"{"type":"assistant","uuid":"ab","message":{"id":"mb","role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"ls"}}]}}"#
            .to_string(),
    );
    lines.extend(inner_call(
        "repl_aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "pwd",
    ));
    let all = run("repl-stale", lines);
    let inner = all
        .iter()
        .find(|c| c.id.starts_with("repl_"))
        .expect("inner");
    assert!(inner.parent.is_none());
}

#[test]
fn inner_calls_after_a_poll_nest_under_the_polled_script_even_after_other_tools() {
    let mut lines = vec![repl_call("t1"), dispatched("t1", 1), repl_call("t2")];
    lines.push(poll(&[&event(1, "settled", "<result>ok</result>")]));
    lines.extend(inner_call(
        "repl_bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        "ls",
    ));
    let all = run("repl-polled", lines);
    let inner = all
        .iter()
        .find(|c| c.id.starts_with("repl_"))
        .expect("inner");
    assert_eq!(inner.parent.as_deref(), Some("t1"));
    assert!(all.iter().find(|c| c.id == "t1").unwrap().finished);
}

#[test]
fn an_oversized_poll_settles_every_envelope_visible_in_its_head() {
    let status = "<event kind=\\\"repl-status\\\">still running</event>";
    let first = event(1, "settled", "<result>first</result>");
    let second = event(2, "settled", &big("<result>start "));
    let line = poll(&[status, &first, &second]);
    assert!(line.len() > 400_000);
    let all = run(
        "repl-big-multi",
        vec![
            repl_call("t1"),
            repl_call("t2"),
            dispatched("t1", 1),
            dispatched("t2", 2),
            line,
        ],
    );
    assert_eq!(all[0].output.as_deref(), Some("first"));
    assert!(all[1].finished);
    assert!(all[1].output.as_deref().unwrap().starts_with("start xxx"));
}

#[test]
fn an_oversized_json_body_finishes_the_script_with_the_status_deciding_failure() {
    let body = format!(
        "{{\\\"eval\\\":1,\\\"error\\\":\\\"boom {}",
        "x".repeat(450_000)
    );
    let event = "<event kind=\\\"repl-eval\\\" status=\\\"settled\\\">\\n".to_string() + &body;
    let all = run(
        "repl-big-json",
        vec![repl_call("t1"), dispatched("t1", 1), enqueue(&event)],
    );
    assert!(all[0].finished);
    assert!(!all[0].is_error);
    assert_eq!(all[0].output.as_deref(), Some("(too large to show)"));
}

#[test]
fn an_oversized_prompt_quoting_an_event_leaves_the_script_running() {
    let quoted = event(1, "failed", &big("<error>"));
    let line = enqueue(&format!("look at this {quoted}"));
    assert!(line.len() > 400_000);
    let all = run(
        "repl-big-quote",
        vec![repl_call("t1"), dispatched("t1", 1), line],
    );
    assert!(!all[0].finished);
    assert!(!all[0].is_error);
}
