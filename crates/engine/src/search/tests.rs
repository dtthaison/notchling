use super::*;

const NOW: f64 = 1_800_000_000.0;

fn claude(json: &str) -> Option<TranscriptLine> {
    claude_line(json.as_bytes())
}

fn codex(json: &str) -> Option<TranscriptLine> {
    codex_line(json.as_bytes())
}

fn msg(role: SessionRole, text: &str, cwd: Option<&str>) -> Option<TranscriptLine> {
    Some(TranscriptLine::Message {
        role,
        text: text.to_string(),
        cwd: cwd.map(str::to_string),
    })
}

#[test]
fn claude_user_and_assistant_text() {
    let user = r#"{"type":"user","isSidechain":false,"cwd":"/Users/me/app","message":{"role":"user","content":"fix the login bug"}}"#;
    assert_eq!(
        claude(user),
        msg(
            SessionRole::User,
            "fix the login bug",
            Some("/Users/me/app")
        )
    );
    let assistant = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"Found it."},{"type":"tool_use","name":"Bash","input":{}},{"type":"text","text":"Fixed."}]}}"#;
    assert_eq!(
        claude(assistant),
        msg(SessionRole::Assistant, "Found it.\nFixed.", None)
    );
}

#[test]
fn claude_skips_noise() {
    assert_eq!(
        claude(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#
        ),
        None
    );
    assert_eq!(
        claude(
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/clear</command-name>"}}"#
        ),
        None
    );
    assert_eq!(
        claude(r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"hello"}}"#),
        None
    );
    assert_eq!(
        claude(r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"hello"}}"#),
        None
    );
    assert_eq!(
        claude(r#"{"type":"attachment","attachment":{"type":"hook_success"}}"#),
        None
    );
    assert_eq!(
        claude(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"x"}]}}"#
        ),
        None
    );
    let huge = format!(
        r#"{{"type":"user","message":{{"role":"user","content":"{}"}}}}"#,
        "a".repeat(100_100)
    );
    assert_eq!(claude(&huge), None);
}

#[test]
fn claude_titles() {
    assert_eq!(
        claude(r#"{"type":"custom-title","customTitle":"notching","sessionId":"s"}"#),
        Some(TranscriptLine::CustomTitle("notching".into()))
    );
    assert_eq!(
        claude(r#"{"type":"ai-title","aiTitle":"Piaz Switch","sessionId":"s"}"#),
        Some(TranscriptLine::AiTitle("Piaz Switch".into()))
    );
    assert_eq!(claude(r#"{"type":"ai-title","aiTitle":""}"#), None);
}

#[test]
fn codex_messages() {
    let user = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"add a retry"}]}}"#;
    assert_eq!(codex(user), msg(SessionRole::User, "add a retry", None));
    let env = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"}]}}"#;
    assert_eq!(codex(env), None);
    let agents = r##"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions"}]}}"##;
    assert_eq!(codex(agents), None);
    let developer = r#"{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"rules"}]}}"#;
    assert_eq!(codex(developer), None);
    let reply = r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done."}]}}"#;
    assert_eq!(codex(reply), msg(SessionRole::Assistant, "Done.", None));
    let not_message = r#"{"type":"response_item","payload":{"type":"reasoning","message":"x"}}"#;
    assert_eq!(codex(not_message), None);
}

#[test]
fn malformed_lines_are_skipped() {
    assert_eq!(claude("not json at all \"role\":\"user\""), None);
    assert_eq!(claude(r#"["role","user"]"#), None);
    assert_eq!(
        codex(r#"{"type":"response_item","payload":"message"}"#),
        None
    );
    assert_eq!(
        claude(r#"{"type": "user", "message": {"role": "user", "content": "hi"}}"#),
        None
    );
    assert_eq!(
        claude(r#"{"type":"user","message":{"role":"user","content":["hi"]}}"#),
        None
    );
}

#[test]
fn builder_title_and_cap() {
    let mut b = SessionTextBuilder::new();
    b.add(TranscriptLine::Message {
        role: SessionRole::User,
        text: "first prompt here".into(),
        cwd: Some("/x".into()),
    });
    b.add(TranscriptLine::Message {
        role: SessionRole::Assistant,
        text: "an answer".into(),
        cwd: None,
    });
    assert_eq!(b.title(), "first prompt here");
    assert_eq!(b.cwd(), Some("/x"));
    b.add(TranscriptLine::AiTitle("Generated".into()));
    assert_eq!(b.title(), "Generated");
    b.add(TranscriptLine::CustomTitle("Mine".into()));
    assert_eq!(b.title(), "Mine");
    assert_eq!(b.text(), "first prompt here\nan answer");

    let mut big = SessionTextBuilder::new();
    let chunk = "x".repeat(6_000);
    for i in 0..10 {
        big.add(TranscriptLine::Message {
            role: SessionRole::User,
            text: format!("{i}{chunk}"),
            cwd: None,
        });
    }
    assert!(big.text().chars().count() <= MAX_CHARS + 10);
    assert!(big.text().contains(&format!("9{chunk}")));
    assert!(!big.text().starts_with('0'));
}

#[test]
fn builder_defaults_and_first_prompt() {
    let mut b = SessionTextBuilder::new();
    assert_eq!(b.title(), "Session");
    b.add(TranscriptLine::Message {
        role: SessionRole::Assistant,
        text: "hi".into(),
        cwd: Some("/a".into()),
    });
    b.add(TranscriptLine::Message {
        role: SessionRole::User,
        text: "ask\nsecond line".into(),
        cwd: Some("/b".into()),
    });
    assert_eq!(b.first_prompt(), Some("ask\nsecond line"));
    assert_eq!(b.cwd(), Some("/a"));
    assert_eq!(b.title(), "ask");
}

#[test]
fn builder_one_huge_message_keeps_its_tail() {
    let mut b = SessionTextBuilder::new();
    let long = format!("{}END", "y".repeat(50_000));
    b.add(TranscriptLine::Message {
        role: SessionRole::User,
        text: long,
        cwd: None,
    });
    let text = b.text();
    assert_eq!(text.chars().count(), MAX_CHARS);
    assert!(text.ends_with("END"));
}

#[test]
fn title_from_first_line_and_cap() {
    assert_eq!(first_line_title("one\ntwo"), "one");
    assert_eq!(first_line_title("\n\nlater"), "later");
    assert_eq!(first_line_title(&"a".repeat(80)), "a".repeat(80));
    assert_eq!(
        first_line_title(&"a".repeat(81)),
        format!("{}…", "a".repeat(79))
    );
}

#[test]
fn clean_trims_and_caps() {
    assert_eq!(clean("  hi \n", SessionRole::User).as_deref(), Some("hi"));
    assert_eq!(clean("   ", SessionRole::User), None);
    assert_eq!(clean("Caveat: x", SessionRole::User), None);
    assert_eq!(
        clean("[Request interrupted by user]", SessionRole::User),
        None
    );
    assert_eq!(
        clean("<b>bold</b>", SessionRole::Assistant).as_deref(),
        Some("<b>bold</b>")
    );
    let long = "z".repeat(6_500);
    assert_eq!(
        clean(&long, SessionRole::Assistant).map(|s| s.chars().count()),
        Some(MAX_MESSAGE_CHARS)
    );
}

#[test]
fn codex_id_from_file_name() {
    assert_eq!(
        codex_id("rollout-2026-11-05T23-05-22-01a1ac75-2a19-7750-a0d3-2df380733486.jsonl"),
        "01a1ac75-2a19-7750-a0d3-2df380733486"
    );
    assert_eq!(codex_id("short.jsonl"), "short");
}

fn session(id: &str, title: &str, folder: &str, text: &str, age: f64) -> IndexedSession {
    session_tool(id, title, folder, text, age, SessionTool::Claude)
}

fn session_tool(
    id: &str,
    title: &str,
    folder: &str,
    text: &str,
    age: f64,
    tool: SessionTool,
) -> IndexedSession {
    IndexedSession::new(
        SessionMeta {
            key: format!("{}:{id}", tool.key()),
            tool_key: tool.key().to_string(),
            session_id: id.to_string(),
            title: title.to_string(),
            folder: folder.to_string(),
            path: String::new(),
            config_dir: None,
            modified: NOW - age,
            size: 0,
        },
        text.to_string(),
    )
}

fn s(id: &str, title: &str, text: &str, age: f64) -> IndexedSession {
    session(id, title, "/Users/me/app", text, age)
}

fn w(words: &[&str]) -> Vec<String> {
    words.iter().map(|x| x.to_string()).collect()
}

fn ids(hits: &[SessionHit]) -> Vec<&str> {
    hits.iter().map(|h| h.meta.session_id.as_str()).collect()
}

#[test]
fn every_word_must_match() {
    let a = s("a", "Login flow", "we rewrote the token refresh", 3600.0);
    let b = s("b", "Billing", "invoices and refresh of the plan", 3600.0);
    let all = [a, b];
    assert_eq!(ids(&rank(&w(&["token", "refresh"]), &all, NOW, 30)), ["a"]);
    assert!(rank(&w(&["nothing"]), &all, NOW, 30).is_empty());
    assert!(rank(&Vec::<String>::new(), &all, NOW, 30).is_empty());
}

#[test]
fn case_insensitive_and_folder_match() {
    let x = session("a", "T", "/Users/me/Notchling", "The Keychain item", 3600.0);
    let all = [x];
    assert_eq!(rank(&w(&["keychain"]), &all, NOW, 30).len(), 1);
    assert_eq!(rank(&w(&["notchling"]), &all, NOW, 30).len(), 1);
    assert_eq!(rank(&w(&["notchling"]), &all, NOW, 30)[0].snippet, None);
}

#[test]
fn title_outranks_body_and_newer_breaks_ties() {
    let body = s("body", "Other", "something about caching here", 60.0);
    let title = s("title", "Caching layer", "unrelated", 86400.0 * 5.0);
    assert_eq!(
        ids(&rank(&w(&["caching"]), &[body, title], NOW, 30)),
        ["title", "body"]
    );

    let old = s("old", "Same", "same words", 86400.0 * 6.0);
    let new = s("new", "Same", "same words", 60.0);
    assert_eq!(
        ids(&rank(&w(&["words"]), &[old, new], NOW, 30)),
        ["new", "old"]
    );
}

#[test]
fn equal_scores_fall_back_to_modified_then_input_order() {
    let a = s("a", "T", "match", 86400.0 * 40.0);
    let b = s("b", "T", "match", 86400.0 * 40.0);
    let c = s("c", "T", "match", 86400.0 * 39.0);
    assert_eq!(
        ids(&rank(&w(&["match"]), &[a, b, c], NOW, 30)),
        ["c", "a", "b"]
    );
}

#[test]
fn phrase_outranks_scattered_words() {
    let scattered = s(
        "s",
        "A",
        "the cache is slow; later we added a token",
        3600.0,
    );
    let phrase = s("p", "B", "we added a token cache to the app", 3600.0);
    let hits = rank(&w(&["token", "cache"]), &[scattered, phrase], NOW, 30);
    assert_eq!(hits[0].meta.session_id, "p");
}

#[test]
fn score_values_are_pinned() {
    let x = session("a", "token", "/x", "token", 0.0);
    let hits = rank(&w(&["token"]), &[x], NOW, 30);
    assert_eq!(hits[0].score, 30.0 + 20.0 + 5.0 + 10.0);
    let y = session("b", "t", "/x", "aaaaaaaaaa", 86400.0 * 30.0);
    assert_eq!(rank(&w(&["aa"]), &[y], NOW, 30)[0].score, 3.0 + 5.0 * 2.0);
}

#[test]
fn limit() {
    let many: Vec<_> = (0..10)
        .map(|i| s(&format!("s{i}"), "t", "match", 3600.0))
        .collect();
    assert_eq!(rank(&w(&["match"]), &many, NOW, 3).len(), 3);
}

#[test]
fn latest_has_no_snippet() {
    let hits = latest(&[s("a", "a", "x", 500.0), s("b", "b", "x", 10.0)], 5);
    assert_eq!(ids(&hits), ["b", "a"]);
    assert_eq!(hits[0].snippet, None);
    assert_eq!(
        latest(&[s("a", "a", "x", 500.0), s("b", "b", "x", 10.0)], 1).len(),
        1
    );
}

#[test]
fn snippet_around_match() {
    let filler = "lorem ipsum dolor ".repeat(20);
    let x = s(
        "a",
        "T",
        &format!("{filler}the Keychain prompt appeared twice {filler}"),
        3600.0,
    );
    let snip = snippet(&x, &w(&["keychain"]), 56).unwrap_or_default();
    assert!(snip.contains("Keychain prompt"), "{snip}");
    assert!(snip.starts_with('…') && snip.ends_with('…'));
    assert!(snip.chars().count() < 200);
}

#[test]
fn snippet_collapses_newlines_and_keeps_short_text() {
    let x = s("a", "T", "line one\n\nline   two with needle\nend", 3600.0);
    assert_eq!(
        snippet(&x, &w(&["needle"]), 56).as_deref(),
        Some("line one line two with needle end")
    );
}

#[test]
fn snippet_with_multibyte_text() {
    let text = format!(
        "{}Xác thực token mới {}",
        "đã sửa lỗi ".repeat(30),
        "xong rồi ".repeat(30)
    );
    let x = s("a", "T", &text, 3600.0);
    let snip = snippet(&x, &w(&["token"]), 56).unwrap_or_default();
    assert!(snip.contains("token"));
    assert!(!snip.contains('\u{FFFD}'));
}

#[test]
fn snippet_prefers_the_phrase_and_is_none_without_a_text_match() {
    let x = s(
        "a",
        "token",
        "cache first, later token cache together",
        3600.0,
    );
    let snip = snippet(&x, &w(&["token", "cache"]), 8).unwrap_or_default();
    assert!(snip.contains("token cache"), "{snip}");
    assert_eq!(snippet(&x, &w(&["absent"]), 56), None);
}

#[test]
fn snippet_when_lowercasing_changes_the_length_shows_lowercased_text() {
    let x = s("a", "T", "İstanbul needle", 3600.0);
    assert_eq!(
        snippet(&x, &w(&["needle"]), 56).as_deref(),
        Some("i\u{307}stanbul needle")
    );
}

#[test]
fn collapse_whitespace() {
    assert_eq!(collapse("  a \n\t b  "), "a b");
    assert_eq!(collapse(""), "");
    assert_eq!(collapse("a\u{a0}b"), "a b");
}

#[test]
fn highlights_mark_every_occurrence() {
    let text = "Fix the Token refresh and token cache";
    let ranges = highlights(text, &w(&["token"]));
    let found: Vec<&str> = ranges.iter().map(|r| &text[r.clone()]).collect();
    assert_eq!(found, ["Token", "token"]);
    assert!(highlights(text, &w(&["absent"])).is_empty());
}

#[test]
fn highlights_are_byte_ranges_on_char_boundaries() {
    let text = "Đã sửa Token và token";
    let ranges = highlights(text, &w(&["token", "đã", "token"]));
    let found: Vec<&str> = ranges.iter().map(|r| &text[r.clone()]).collect();
    assert_eq!(found, ["Đã", "Token", "token"]);
}

#[test]
#[allow(clippy::single_range_in_vec_init)]
fn highlights_are_disjoint_when_words_overlap() {
    let text = "the bridge\u{2019}s state";
    let ranges = highlights(text, &w(&["bridge", "b"]));
    assert_eq!(ranges, [4..10]);
    let text = "cache\u{2014}stale";
    let ranges = highlights(text, &w(&["cache", "c"]));
    assert_eq!(ranges, [0..5]);
    assert_eq!(highlights("abc", &w(&["ab", "c"])), [0..3]);
    assert_eq!(highlights("a b a", &w(&["a"])), [0..1, 4..5]);
}

#[test]
fn highlights_skip_text_whose_lowercase_changes_length() {
    assert!(highlights("İ token", &w(&["token"])).is_empty());
}

#[test]
fn words_lowercase_and_split_on_whitespace() {
    assert_eq!(
        words("  Token\tRefresh\n\u{a0}Cache "),
        ["token", "refresh", "cache"]
    );
    assert!(words("   ").is_empty());
}

#[test]
fn age() {
    assert_eq!(age_text(NOW - 10.0, NOW), "now");
    assert_eq!(age_text(NOW - 300.0, NOW), "5m");
    assert_eq!(age_text(NOW - 3.0 * 3600.0, NOW), "3h");
    assert_eq!(age_text(NOW - 2.0 * 86400.0, NOW), "2d");
    assert_eq!(age_text(NOW - 15.0 * 86400.0, NOW), "2w");
    assert_eq!(age_text(NOW + 100.0, NOW), "now");
}

#[test]
fn tool_keys_round_trip() {
    for tool in [
        SessionTool::Claude,
        SessionTool::Codex,
        SessionTool::Antigravity,
    ] {
        assert_eq!(SessionTool::from_key(tool.key()), Some(tool));
    }
    assert_eq!(SessionTool::Antigravity.key(), "agy");
    assert_eq!(SessionTool::from_key("gemini"), None);
}

#[test]
fn meta_json_uses_the_swift_cache_keys() {
    let m = session_tool("abc", "T", "/f", "x", 0.0, SessionTool::Codex).meta;
    let json = serde_json::to_value(&m).unwrap_or_default();
    assert_eq!(json["toolKey"], "codex");
    assert_eq!(json["sessionID"], "abc");
    let back: Result<SessionMeta, _> = serde_json::from_value(json);
    assert_eq!(back.ok(), Some(m.clone()));
    assert_eq!(m.tool(), SessionTool::Codex);
    let old = r#"{"key":"k","toolKey":"zzz","sessionID":"s","title":"t","folder":"f","path":"","modified":5.0}"#;
    let m: Option<SessionMeta> = serde_json::from_str(old).ok();
    assert_eq!(m.as_ref().map(|m| m.size), Some(0));
    assert_eq!(m.map(|m| m.tool()), Some(SessionTool::Claude));
}

#[test]
fn ruling_case_folding_covers_vietnamese() {
    let x = s("a", "SỬA LỖI", "ĐÃ SỬA LỖI ĐĂNG NHẬP", 3600.0);
    let hits = rank(&words("Đăng Nhập"), &[x], NOW, 30);
    assert_eq!(hits.len(), 1);
    assert!(hits[0]
        .snippet
        .as_deref()
        .unwrap_or("")
        .contains("ĐĂNG NHẬP"));
}

#[test]
fn ruling_no_diacritic_folding() {
    let x = s("a", "sửa lỗi", "đã sửa lỗi đăng nhập", 3600.0);
    assert!(rank(&words("sua loi"), std::slice::from_ref(&x), NOW, 30).is_empty());
    assert_eq!(rank(&words("sửa lỗi"), &[x], NOW, 30).len(), 1);
}

#[test]
fn ruling_no_normalization_between_nfc_and_nfd() {
    let x = s("a", "t", "caf\u{e9}", 3600.0);
    assert!(rank(&words("cafe\u{301}"), std::slice::from_ref(&x), NOW, 30).is_empty());
    assert_eq!(rank(&words("caf\u{e9}"), &[x], NOW, 30).len(), 1);
}

#[test]
fn ruling_lowercasing_is_per_scalar_without_final_sigma() {
    assert_eq!(lowercase("ΟΔΟΣ"), "οδοσ");
    assert_eq!(lowercase("ĐÀ"), "đà");
}

#[test]
fn ruling_empty_words_are_ignored() {
    let x = s("a", "T", "alpha", 3600.0);
    assert_eq!(
        rank(&w(&["alpha", ""]), std::slice::from_ref(&x), NOW, 30).len(),
        1
    );
    assert!(rank(&w(&[""]), &[x], NOW, 30).is_empty());
}

#[test]
fn ruling_caps_count_scalars_not_grapheme_clusters() {
    let text = "e\u{301}".repeat(3_001);
    assert_eq!(
        clean(&text, SessionRole::Assistant).map(|t| t.chars().count()),
        Some(MAX_MESSAGE_CHARS)
    );
}
