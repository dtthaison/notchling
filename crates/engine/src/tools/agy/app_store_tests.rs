use super::*;
use crate::tools::agy::elsewhere::{self, Source};
use crate::transcript::model::ItemKind;

const NOW: f64 = 1_794_182_400.0;
const OURS: &str = "79ba6320-1b32-46fe-838d-9bb614eb992d";
const TEST: &str = "457369a9-35a1-40f8-92b9-26b6398b78a1";
const OTHER_BOT: &str = "11111111-1111-4111-8111-111111111111";
const NATIVE: &str = "22222222-2222-4222-8222-222222222222";
const CLI_ROW: &str = "33333333-3333-4333-8333-333333333333";
const OLD: &str = "44444444-4444-4444-8444-444444444444";

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-agy-app-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value & 0x7f) as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn put_bytes(out: &mut Vec<u8>, field: u64, body: &[u8]) {
    put_varint(out, field << 3 | 2);
    put_varint(out, body.len() as u64);
    out.extend_from_slice(body);
}

fn summary(title: &str, session: &str, sidecar: Option<&str>) -> Vec<u8> {
    let mut out = Vec::new();
    put_bytes(&mut out, 1, title.as_bytes());
    put_varint(&mut out, 2 << 3);
    put_varint(&mut out, 4);
    let mut time = Vec::new();
    put_varint(&mut time, 1 << 3);
    put_varint(&mut time, 1_795_015_409);
    put_bytes(&mut out, 3, &time);
    let mut meta = Vec::new();
    put_bytes(&mut meta, 3, b"b61a3fe0-33bc-4b79-a9e6-dca73ba8f7df");
    put_bytes(&mut meta, 6, session.as_bytes());
    if let Some(sidecar) = sidecar {
        let mut config = Vec::new();
        put_bytes(&mut config, 2, b"always");
        put_bytes(
            &mut config,
            1,
            b"/Applications/Notchling.app/Contents/MacOS/notchling",
        );
        let mut entry = Vec::new();
        put_bytes(&mut entry, 1, sidecar.as_bytes());
        put_bytes(&mut entry, 2, &config);
        let mut record = Vec::new();
        put_bytes(&mut record, 2, &entry);
        put_bytes(&mut meta, 12, &record);
    }
    put_bytes(&mut meta, 18, b"6b1f6382-9f5e-4083-bbc5-d7eae38318b7");
    put_bytes(&mut out, 17, &meta);
    out
}

#[allow(clippy::too_many_arguments)]
fn insert(
    db: &Connection,
    id: &str,
    title: &str,
    status: &str,
    at: &str,
    source: &str,
    depth: i64,
    sidecar: Option<&str>,
) {
    db.execute(
        "INSERT INTO conversation_summaries VALUES (?1, ?2, ?3, '', ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            id,
            title,
            title,
            status,
            at,
            source,
            depth,
            summary(title, id, sidecar)
        ],
    )
    .unwrap();
}

fn app_root(base: &Path) -> PathBuf {
    let root = base.join("antigravity");
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn app_db(root: &Path) -> Connection {
    let db = Connection::open(root.join(DB_NAME)).unwrap();
    db.execute_batch(
        "CREATE TABLE conversation_summaries (conversation_id TEXT, title TEXT, preview TEXT, \
         workspace_uris TEXT, status TEXT, last_modified_time TEXT, source TEXT, \
         nesting_depth INTEGER, raw_summary BLOB);",
    )
    .unwrap();
    db
}

fn world(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = scratch(tag);
    let root = app_root(&base);
    let db = app_db(&root);
    insert(
        &db,
        OURS,
        "pong",
        "CASCADE_RUN_STATUS_IDLE",
        "2026-11-07 15:23:29.996725+00:00",
        "",
        0,
        Some("notchling/sidecar"),
    );
    insert(
        &db,
        TEST,
        "notchling-test",
        "CASCADE_RUN_STATUS_RUNNING",
        "2026-11-07 06:54:29.119418+00:00",
        "",
        0,
        Some("notchling-test"),
    );
    insert(
        &db,
        OTHER_BOT,
        "from another sidecar",
        "CASCADE_RUN_STATUS_IDLE",
        "2026-11-07 06:54:29.119418+00:00",
        "",
        0,
        Some("slack-bot"),
    );
    insert(
        &db,
        NATIVE,
        "typed in the app",
        "CASCADE_RUN_STATUS_IDLE",
        "2026-11-07 06:54:29.119418+00:00",
        "",
        0,
        None,
    );
    insert(
        &db,
        CLI_ROW,
        "imported from the CLI",
        "CASCADE_RUN_STATUS_IDLE",
        "2026-11-07 06:54:29.119418+00:00",
        "CORTEX_TRAJECTORY_SOURCE_CLI",
        0,
        Some("notchling/sidecar"),
    );
    insert(
        &db,
        OLD,
        "months ago",
        "CASCADE_RUN_STATUS_IDLE",
        "2026-08-01 06:54:29.119418+00:00",
        "",
        0,
        Some("notchling/sidecar"),
    );
    let cli = base.join("antigravity-cli");
    std::fs::create_dir_all(&cli).unwrap();
    (base, cli, root)
}

#[test]
fn the_sidecar_is_read_from_the_summary_and_nothing_else_names_one() {
    let with = summary("pong", OURS, Some("notchling/sidecar"));
    assert_eq!(sidecar_of(&with).as_deref(), Some("notchling/sidecar"));
    assert_eq!(sidecar_of(&summary("pong", OURS, None)), None);
    assert_eq!(sidecar_of(b""), None);
    assert_eq!(sidecar_of(&with[..with.len() / 2]), None);
    assert_eq!(
        sidecar_of(b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff"),
        None
    );
    let mut text_only = Vec::new();
    put_bytes(&mut text_only, 1, b"talks about notchling/sidecar");
    assert_eq!(sidecar_of(&text_only), None);
}

#[test]
fn only_notchlings_own_sidecars_count() {
    assert!(ours("notchling/sidecar"));
    assert!(ours("notchling-test"));
    assert!(ours("notchling"));
    assert!(!ours("slack-bot"));
    assert!(!ours("notchlingx"));
    assert!(!ours(""));
}

#[test]
fn the_app_lists_only_conversations_a_notchling_sidecar_made() {
    let (base, cli, _root) = world("list");
    let copies = base.join("copy");
    let rows = rows_cached(&cli, &copies, NOW, Duration::ZERO);
    let mut ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, [TEST, OURS]);
    let ours = rows.iter().find(|r| r.id == OURS).unwrap();
    assert_eq!(ours.title, "pong");
    assert_eq!(ours.row_id(), format!("agy:{OURS}"));
    assert!(!ours.running);
    assert!(rows.iter().find(|r| r.id == TEST).unwrap().running);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_missing_or_unreadable_app_store_lists_nothing() {
    let base = scratch("none");
    let cli = base.join("antigravity-cli");
    assert!(rows_cached(&cli, &base.join("copy"), NOW, Duration::ZERO).is_empty());
    let root = app_root(&base);
    std::fs::write(root.join(DB_NAME), "not a database").unwrap();
    assert!(rows_cached(&cli, &base.join("copy"), NOW, Duration::ZERO).is_empty());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn cli_rows_are_listed_as_before_and_app_rows_follow_them_once() {
    let (base, cli, _root) = world("merge");
    let cli_db = Connection::open(cli.join(DB_NAME)).unwrap();
    cli_db
        .execute_batch(
            "CREATE TABLE conversation_summaries (conversation_id TEXT, title TEXT, preview TEXT, \
             workspace_uris TEXT, status TEXT, last_modified_time TEXT, nesting_depth INTEGER);
             INSERT INTO conversation_summaries VALUES \
             ('cli-one', 'From the CLI', '', '[\"file:///work/app\"]', 'IDLE', \
              '2026-11-07 10:00:00.000000+00:00', 0);
             INSERT INTO conversation_summaries VALUES \
             ('79ba6320-1b32-46fe-838d-9bb614eb992d', 'Also in the CLI list', '', '', 'IDLE', \
              '2026-11-07 10:00:00.000000+00:00', 0);",
        )
        .unwrap();
    drop(cli_db);
    let copies = base.join("copy");
    let cli_only = summaries::rows_cached(&cli, &copies, NOW, Duration::ZERO);
    assert_eq!(cli_only.len(), 2);
    let all = all_rows_cached(&cli, &copies, NOW, Duration::ZERO);
    let ids: Vec<&str> = all.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        &ids[..2],
        ["cli-one", OURS],
        "CLI rows first, as listed alone"
    );
    assert_eq!(all[1].title, "Also in the CLI list", "the CLI's row wins");
    assert_eq!(ids.iter().filter(|id| **id == OURS).count(), 1);
    assert_eq!(ids.len(), 3);
    assert_eq!(all[0], cli_only[0]);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn an_app_conversation_is_the_one_with_a_database_in_the_app_store() {
    let base = scratch("exists");
    let root = app_root(&base);
    std::fs::create_dir_all(root.join("conversations")).unwrap();
    std::fs::write(root.join("conversations").join(format!("{OURS}.db")), "x").unwrap();
    assert!(exists(&root, OURS));
    assert!(!exists(&root, TEST));
    assert!(!exists(&root, "../conversations/x"));
    assert_eq!(
        log_path(&root, OURS),
        root.join("brain")
            .join(OURS)
            .join(".system_generated/logs/transcript.jsonl")
    );
    assert_eq!(root_beside(&base.join("antigravity-cli")), root);
    let _ = std::fs::remove_dir_all(&base);
}

fn lines(text: &[&str]) -> Vec<String> {
    text.iter().map(|l| (*l).to_owned()).collect()
}

const PONG: [&str; 8] = [
    r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-11-18T06:53:22Z","content":"<USER_REQUEST>\nReply with the single word PONG and nothing else.\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nThe current local time is: 2026-11-18T13:53:22+07:00.\n</ADDITIONAL_METADATA>"}"#,
    r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-18T06:53:22Z","input_tokens":14178,"cache_read_tokens":0,"output_tokens":2,"content":"PONG"}"#,
    r#"{"step_index":2,"source":"SYSTEM","type":"SYSTEM_MESSAGE","status":"DONE","created_at":"2026-11-18T06:53:58Z","content":"The following is a <SYSTEM_MESSAGE> not actually sent by the user. It is provided by the system as important information to pay attention to.\n\n<SYSTEM_MESSAGE>\n[Message] timestamp=2026-11-18T06:53:58Z sender=system priority=MESSAGE_PRIORITY_HIGH content=Now reply with the single word PING.\n</SYSTEM_MESSAGE>"}"#,
    r#"{"step_index":3,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-18T06:53:58Z","input_tokens":2090,"cache_read_tokens":12182,"output_tokens":1,"content":"PING"}"#,
    r#"{"step_index":4,"source":"SYSTEM","type":"SYSTEM_MESSAGE","status":"DONE","created_at":"2026-11-18T06:54:26Z","content":"The following is a <SYSTEM_MESSAGE> not actually sent by the user. It is provided by the system as important information to pay attention to.\n\n<SYSTEM_MESSAGE>\n[Message] timestamp=2026-11-18T06:54:26Z sender=system priority=MESSAGE_PRIORITY_HIGH content=Count slowly from 1 to 30, one number per line.\n</SYSTEM_MESSAGE>"}"#,
    r#"{"step_index":5,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-18T06:54:26Z","input_tokens":2199,"cache_read_tokens":12172,"output_tokens":80,"content":"1\n2\n3\n4\n5\n6\n7\n8\n9\n10"}"#,
    r#"{"step_index":6,"source":"SYSTEM","type":"SYSTEM_MESSAGE","status":"DONE","created_at":"2026-11-18T06:54:27Z","content":"The following is a <SYSTEM_MESSAGE> not actually sent by the user. It is provided by the system as important information to pay attention to.\n\n<SYSTEM_MESSAGE>\n[Message] timestamp=2026-11-18T06:54:26Z sender=system priority=MESSAGE_PRIORITY_HIGH content=Reply with the single word DONE.\n</SYSTEM_MESSAGE>"}"#,
    r#"{"step_index":7,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-18T06:54:27Z","input_tokens":2377,"cache_read_tokens":12164,"output_tokens":1,"content":"DONE"}"#,
];

fn words(conversation: &crate::transcript::model::Conversation) -> Vec<(&'static str, String)> {
    conversation
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::User { text, .. } => Some(("you", text.clone())),
            ItemKind::Assistant { text } => Some(("agy", text.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn the_app_log_reads_as_your_messages_and_the_agents_replies() {
    let (conversation, working) = elsewhere::from_lines_in(&lines(&PONG), Source::App);
    assert!(!working);
    let said = words(&conversation);
    assert_eq!(
        said.iter().map(|(who, _)| *who).collect::<Vec<_>>(),
        ["you", "agy", "you", "agy", "you", "agy", "you", "agy"]
    );
    assert_eq!(
        said[0].1,
        "Reply with the single word PONG and nothing else."
    );
    assert_eq!(said[1].1, "PONG");
    assert_eq!(said[2].1, "Now reply with the single word PING.");
    assert_eq!(said[6].1, "Reply with the single word DONE.");
    assert_eq!(said[7].1, "DONE");
}

#[test]
fn a_message_the_agent_has_not_answered_yet_is_work_in_progress() {
    let open = lines(&PONG[..7]);
    let (conversation, working) = elsewhere::from_lines_in(&open, Source::App);
    assert!(working);
    assert_eq!(
        words(&conversation).last().unwrap().1,
        "Reply with the single word DONE."
    );
    let first = lines(&PONG[..1]);
    assert!(elsewhere::from_lines_in(&first, Source::App).1);
}

#[test]
fn only_the_systems_own_messages_are_yours_and_the_cli_reader_is_unchanged() {
    let (cli, cli_working) = elsewhere::from_lines(&lines(&PONG));
    assert!(!cli_working);
    assert_eq!(
        words(&cli).len(),
        5,
        "the CLI reader still skips system messages"
    );
    let from_a_subagent = r#"{"step_index":2,"type":"SYSTEM_MESSAGE","content":"x <SYSTEM_MESSAGE>\n[Message] timestamp=2026-11-18T06:53:58Z sender=5d1f priority=MESSAGE_PRIORITY_HIGH content=done\n</SYSTEM_MESSAGE>"}"#;
    let (conversation, working) = elsewhere::from_lines_in(&lines(&[from_a_subagent]), Source::App);
    assert!(words(&conversation).is_empty());
    assert!(!working);
    let plain = r#"{"step_index":2,"type":"SYSTEM_MESSAGE","content":"nothing to see"}"#;
    assert!(words(&elsewhere::from_lines_in(&lines(&[plain]), Source::App).0).is_empty());
}

#[test]
fn a_log_file_is_read_from_the_app_brain() {
    let base = scratch("log");
    let root = app_root(&base);
    let path = log_path(&root, TEST);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, PONG.join("\n") + "\n").unwrap();
    let (conversation, working) = elsewhere::from_log_file_in(&path, None, Source::App);
    assert!(!working);
    assert_eq!(words(&conversation).len(), 8);
    let _ = std::fs::remove_dir_all(&base);
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agy-app/antigravity-cli")
}

const FIXTURE_NOW: f64 = 4_098_729_600.0;

#[test]
fn the_two_real_shaped_app_conversations_are_listed_and_the_apps_own_are_not() {
    let base = scratch("fixture");
    let rows = rows_cached(&fixture(), &base, FIXTURE_NOW, Duration::ZERO);
    let mut ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, [TEST, OURS]);
    let all = all_rows_cached(&fixture(), &base, FIXTURE_NOW, Duration::ZERO);
    let listed: Vec<&str> = all.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        listed[0], "9ddb7ec8-ff66-4511-b25a-0ad5a8ed8c8e",
        "the CLI conversation is first and listed as it always was"
    );
    assert_eq!(listed.len(), 3);
    assert_eq!(all[0].title, "Check one illustration");
    assert_eq!(all[0].folder, "/Users/me/work/shop");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn the_real_shaped_logs_show_messages_replies_and_tool_calls() {
    let root = root_beside(&fixture());
    let (test, working) = elsewhere::from_log_file_in(&log_path(&root, TEST), None, Source::App);
    assert!(!working);
    let said = words(&test);
    assert_eq!(said.len(), 8);
    assert_eq!(said[7].1, "DONE");
    let (ours, working) = elsewhere::from_log_file_in(&log_path(&root, OURS), None, Source::App);
    assert!(!working);
    assert_eq!(
        words(&ours),
        [
            ("you", "pong".to_owned()),
            ("agy", "Ping! Ready when you are.".to_owned())
        ]
    );
    let tools: Vec<&str> = ours
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::Tools(calls) => calls.first().map(|c| c.title.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(tools, ["Read SKILL.md"]);
    assert!(exists(&root, OURS) && exists(&root, TEST));
    assert!(!exists(&root, "bf68ea82-bc31-4c24-b8bf-8aa150aa05d4"));
}
