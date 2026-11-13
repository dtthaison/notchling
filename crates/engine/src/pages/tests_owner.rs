use super::owner::*;
use super::tests::{ctx, publish_event};
use super::*;
use crate::sessions::testutil::TempDir;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

const ID: &str = "2vQiSN564g8wL9dUb2VwFy";
const SLUG: &str = "0f8e2c4a-6b1d-4e3f-9a7c-5d2b8e1f4a6c";
const OWNER: &str = "owner@example.com";

struct Tokens {
    asked: RefCell<Vec<String>>,
    answer: Result<OwnerToken, TokenError>,
}

impl Tokens {
    fn owner() -> Tokens {
        Tokens {
            asked: RefCell::new(Vec::new()),
            answer: Ok(OwnerToken {
                email: OWNER.into(),
                token: "tok-owner".into(),
                org: "org-1".into(),
            }),
        }
    }
}

impl TokenSource for Tokens {
    fn token_for(&self, email: &str) -> Result<OwnerToken, TokenError> {
        self.asked.borrow_mut().push(email.to_string());
        self.answer.clone()
    }
}

struct Http {
    sent: RefCell<Vec<HttpRequest>>,
    answers: RefCell<VecDeque<HttpResponse>>,
}

impl Http {
    fn new(answers: Vec<HttpResponse>) -> Http {
        Http {
            sent: RefCell::new(Vec::new()),
            answers: RefCell::new(answers.into()),
        }
    }
}

impl HttpClient for Http {
    fn send(&self, request: &HttpRequest) -> HttpResponse {
        self.sent.borrow_mut().push(request.clone());
        self.answers
            .borrow_mut()
            .pop_front()
            .unwrap_or(HttpResponse {
                status: 0,
                body: "no answer".into(),
            })
    }
}

struct Ticking {
    now: Cell<f64>,
    step: f64,
}

impl Clock for Ticking {
    fn now(&self) -> f64 {
        let t = self.now.get();
        self.now.set(t + self.step);
        t
    }
}

fn ok(body: Value) -> HttpResponse {
    HttpResponse {
        status: 200,
        body: body.to_string(),
    }
}

struct Setup {
    dir: TempDir,
    lib: PageLibrary,
    event: Value,
}

fn setup(recorded_version: &str) -> Setup {
    let dir = TempDir::new("owner");
    dir.write("work/p.html", "<title>Old</title><p>old</p>");
    let lib = PageLibrary::writable(dir.path("pages"));
    let url = format!("https://claude.ai/artifact/{ID}");
    let response = if recorded_version.is_empty() {
        url.clone()
    } else {
        format!("{url} (version {recorded_version})")
    };
    let cwd = dir.path("work").to_string_lossy().into_owned();
    lib.record(
        &publish_event(&cwd, "p.html", &response),
        &ctx(OWNER, dir.root()),
    )
    .unwrap()
    .unwrap();
    dir.write("work/p.html", "<title>New</title><p>new</p>");
    let event = json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Artifact",
        "cwd": cwd,
        "tool_input": {"file_path": "p.html", "url": url},
    });
    Setup { dir, lib, event }
}

fn run_with(s: &Setup, tokens: &Tokens, http: &Http, step: f64) -> Option<Outcome> {
    let clock = Ticking {
        now: Cell::new(0.0),
        step,
    };
    let env = OwnerEnv {
        tokens,
        http,
        clock: &clock,
        active_email: "me@example.com",
        record: ctx("me@example.com", s.dir.root()),
    };
    run(&s.lib, &s.event, &env)
}

fn live(version: &str) -> HttpResponse {
    ok(json!({"live": version}))
}

#[test]
fn a_link_id_is_the_pages_uuid_in_base58() {
    assert_eq!(uuid_from_short_id(ID), SLUG);
    assert_eq!(uuid_from_short_id("not-base58!"), "not-base58!");
    assert_eq!(short_id_from_uuid(SLUG).as_deref(), Some(ID));
    for uuid in [
        "00000000-0000-0000-0000-000000000000",
        "00000000-0000-0000-0000-000000000001",
        "ffffffff-ffff-ffff-ffff-ffffffffffff",
        "00ab12cd-0000-4e3f-8a7c-5d2b8e1f4a6c",
    ] {
        let short = short_id_from_uuid(uuid).unwrap();
        assert_eq!(short.len(), 22, "{uuid} -> {short}");
        assert_eq!(uuid_from_short_id(&short), uuid);
    }
    assert_eq!(short_id_from_uuid("not a uuid"), None);
    assert_eq!(
        short_id_from_uuid("0f8e2c4a-6b1d-4e3f-9a7c-5d2b8e1f4a6"),
        None
    );
}

#[test]
fn a_newer_version_is_never_overwritten_without_force() {
    assert!(stale_refusal("u", "1-a", "2-b", false)
        .unwrap()
        .contains("force: true"));
    assert_eq!(stale_refusal("u", "1-a", "2-b", true), None);
    assert_eq!(stale_refusal("u", "1-a", "1-a", false), None);
    assert_eq!(
        stale_refusal("u", "", "2-b", false),
        None,
        "nothing on record to compare"
    );
}

#[test]
fn the_body_carries_the_slug_base_version_content_and_title() {
    let input = json!({"description": "d", "label": "l", "force": true, "ignored": 1});
    let body = publish_body(
        SLUG,
        "9-z",
        "<title>T</title>x",
        "fallback",
        input.as_object().unwrap(),
    );
    assert_eq!(
        body,
        json!({"slug": SLUG, "baseVersion": "9-z", "content": "<title>T</title>x", "title": "T",
               "description": "d", "label": "l", "force": true})
    );
    let plain = publish_body(SLUG, "9-z", "x", "fallback", json!({}).as_object().unwrap());
    assert_eq!(plain["title"], "fallback");
    assert!(plain.get("force").is_none() && plain.get("description").is_none());
}

#[test]
fn the_request_headers_are_claude_codes() {
    let get = HttpRequest {
        method: Method::Get,
        path: "/x".into(),
        token: "t".into(),
        body: None,
        timeout_secs: 1.0,
    };
    let headers = get.headers();
    assert!(headers.contains(&("Authorization", "Bearer t".to_string())));
    assert!(headers.contains(&("anthropic-beta", "oauth-2025-04-20".to_string())));
    assert!(!headers.iter().any(|(k, _)| *k == "Content-Type"));
    let post = HttpRequest {
        body: Some(json!({})),
        method: Method::Post,
        ..get
    };
    assert!(post
        .headers()
        .contains(&("Content-Type", "application/json".to_string())));
}

#[test]
fn a_publish_as_the_owner_goes_through_and_updates_the_copy() {
    let s = setup("100000-aa");
    let tokens = Tokens::owner();
    let http = Http::new(vec![live("100000-aa"), ok(json!({"version": "200000-bb"}))]);
    let outcome = run_with(&s, &tokens, &http, 1.0).unwrap();
    let Outcome::Done(text) = outcome else {
        panic!("{outcome:?}");
    };
    assert!(text.starts_with("Done, not an error: Notchling published p.html to "));
    assert!(text.contains(
        "as owner@example.com, the page's owner (Claude Code is signed in as me@example.com"
    ));
    assert!(text.contains(". New version 200000-bb, over 100000-aa."));
    assert!(text.ends_with("Don't publish it again. Notchling's local copy is updated."));

    let sent = http.sent.borrow();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].method, Method::Get);
    assert_eq!(sent[0].path, format!("/api/frame/perm/{SLUG}?org=org-1"));
    assert_eq!(sent[1].method, Method::Post);
    assert_eq!(sent[1].path, "/api/frame/deploy/direct");
    assert_eq!(sent[1].token, "tok-owner");
    let body = sent[1].body.as_ref().unwrap();
    assert_eq!(body["slug"], SLUG);
    assert_eq!(body["baseVersion"], "100000-aa");
    assert_eq!(body["title"], "New");
    assert!(body["content"].as_str().unwrap().contains("<p>new</p>"));
    assert!(body.get("force").is_none());
    assert_eq!(*tokens.asked.borrow(), vec![OWNER.to_string()]);
    let page = s.lib.entries().remove(0);
    assert_eq!(page.version, "200000-bb");
    assert_eq!(page.account, OWNER);
    assert_eq!(page.title, "New");
}

#[test]
fn identical_content_says_so() {
    let s = setup("100000-aa");
    let http = Http::new(vec![live("100000-aa"), ok(json!({"version": "100000-aa"}))]);
    let Some(Outcome::Done(text)) = run_with(&s, &Tokens::owner(), &http, 1.0) else {
        panic!()
    };
    assert!(text.contains("claude.ai already had exactly this content (version 100000-aa)"));
}

#[test]
fn a_page_changed_elsewhere_is_refused_until_force() {
    let s = setup("100000-aa");
    let http = Http::new(vec![live("300000-cc")]);
    let Some(Outcome::Refused(text)) = run_with(&s, &Tokens::owner(), &http, 1.0) else {
        panic!()
    };
    assert!(
        text.contains("300000-cc") && text.contains("100000-aa") && text.contains("force: true")
    );
    assert_eq!(http.sent.borrow().len(), 1, "no publish was sent");

    let mut forced = setup("100000-aa");
    forced.event["tool_input"]["force"] = json!(true);
    let http = Http::new(vec![live("300000-cc"), ok(json!({"version": "400000-dd"}))]);
    assert!(matches!(
        run_with(&forced, &Tokens::owner(), &http, 1.0),
        Some(Outcome::Done(_))
    ));
    let sent = http.sent.borrow();
    let body = sent[1].body.as_ref().unwrap();
    assert_eq!(body["force"], true);
    assert_eq!(
        body["baseVersion"], "300000-cc",
        "built on the live version"
    );
}

#[test]
fn a_page_with_no_recorded_version_is_not_blocked() {
    let s = setup("");
    let http = Http::new(vec![live("300000-cc"), ok(json!({"version": "400000-dd"}))]);
    assert!(matches!(
        run_with(&s, &Tokens::owner(), &http, 1.0),
        Some(Outcome::Done(_))
    ));
}

#[test]
fn another_accounts_token_is_never_sent() {
    let s = setup("100000-aa");
    let tokens = Tokens {
        asked: RefCell::new(Vec::new()),
        answer: Ok(OwnerToken {
            email: "someone-else@example.com".into(),
            token: "tok-other".into(),
            org: "org-2".into(),
        }),
    };
    let http = Http::new(vec![live("100000-aa")]);
    let Some(Outcome::Failed(text)) = run_with(&s, &tokens, &http, 1.0) else {
        panic!()
    };
    assert!(text.contains("belongs to another account"));
    assert!(http.sent.borrow().is_empty(), "nothing was sent at all");
    assert_eq!(*tokens.asked.borrow(), vec![OWNER.to_string()]);
}

#[test]
fn without_a_login_the_normal_publish_runs() {
    let s = setup("100000-aa");
    let http = Http::new(vec![]);
    for answer in [
        Err(TokenError::Unavailable("accounts arrive in M6".into())),
        Err(TokenError::NoAccount),
    ] {
        let tokens = Tokens {
            asked: RefCell::new(Vec::new()),
            answer,
        };
        assert_eq!(run_with(&s, &tokens, &http, 1.0), None);
    }
    assert!(http.sent.borrow().is_empty());
    assert_eq!(
        UnavailableTokens.token_for(OWNER),
        Err(TokenError::Unavailable("accounts arrive in M6".into()))
    );
}

#[test]
fn publishes_it_does_not_handle_are_left_to_claude_code() {
    let http = Http::new(vec![]);
    let tokens = Tokens::owner();
    type Change = Box<dyn Fn(&mut Value)>;
    let mut cases: Vec<Change> = vec![
        Box::new(|e| e["tool_input"]["action"] = json!("read")),
        Box::new(|e| {
            e["tool_input"]["url"] = json!("https://claude.ai/artifact/ZZZZZZZZZZZZZZZZZZZZZZ")
        }),
        Box::new(|e| e["tool_input"]["url"] = json!("https://claude.ai/artifact/short")),
        Box::new(|e| e["tool_input"]["file_path"] = json!("p.md")),
        Box::new(|e| e["tool_input"]["file_path"] = json!("gone.html")),
        Box::new(|e| e["tool_input"]["files"] = json!({"a.css": "a.css"})),
        Box::new(|e| e["tool_input"]["capabilities"] = json!({})),
        Box::new(|e| e["tool_input"]["root"] = json!(null)),
        Box::new(|e| e["tool_input"]["asset"] = json!(true)),
        Box::new(|e| e["tool_input"]["edits"] = json!([{"old_str": "absent", "new_str": "x"}])),
        Box::new(|e| {
            e["tool_input"]["edits"] =
                json!([{"old_str": "<p>", "new_str": "x"}, {"old_str": "<p>", "new_str": "y"}])
        }),
        Box::new(|e| e["tool_input"]["edits"] = json!([{"old_str": "", "new_str": "x"}])),
    ];
    for change in cases.drain(..) {
        let mut s = setup("100000-aa");
        change(&mut s.event);
        assert_eq!(run_with(&s, &tokens, &http, 1.0), None, "{}", s.event);
    }
    assert!(http.sent.borrow().is_empty());
}

#[test]
fn a_page_with_no_known_owner_is_left_alone() {
    let s = setup("100000-aa");
    let mut list = s.lib.entries();
    list[0].account.clear();
    std::fs::write(
        s.lib.list_path(),
        serde_json::to_vec(&json!({"pages": list})).unwrap(),
    )
    .unwrap();
    let tokens = Tokens::owner();
    assert_eq!(run_with(&s, &tokens, &Http::new(vec![]), 1.0), None);
    assert!(
        tokens.asked.borrow().is_empty(),
        "no login was even asked for"
    );
}

#[test]
fn edits_are_applied_once_and_saved_to_the_file() {
    let mut s = setup("100000-aa");
    s.event["tool_input"]["edits"] = json!([{"old_str": "<p>new</p>", "new_str": "<p>edited</p>"}]);
    let http = Http::new(vec![live("100000-aa"), ok(json!({"version": "200000-bb"}))]);
    let Some(Outcome::Done(text)) = run_with(&s, &Tokens::owner(), &http, 1.0) else {
        panic!()
    };
    assert!(text.ends_with("; the edits were applied to the file too. Don't publish it again. Notchling's local copy is updated."), "{text}");
    let file = std::fs::read_to_string(s.dir.path("work/p.html")).unwrap();
    assert!(file.contains("<p>edited</p>"));
    let sent = http.sent.borrow();
    assert!(sent[1].body.as_ref().unwrap()["content"]
        .as_str()
        .unwrap()
        .contains("edited"));
}

#[test]
fn trouble_on_the_way_fails_open_with_a_reason() {
    let s = setup("100000-aa");
    let http = Http::new(vec![HttpResponse {
        status: 404,
        body: " not found ".into(),
    }]);
    let Some(Outcome::Failed(text)) = run_with(&s, &Tokens::owner(), &http, 1.0) else {
        panic!()
    };
    assert!(
        text.contains("reading the page failed (HTTP 404: not found)."),
        "{text}"
    );
    let http = Http::new(vec![ok(json!({"other": 1}))]);
    assert!(matches!(
        run_with(&s, &Tokens::owner(), &http, 1.0),
        Some(Outcome::Failed(t)) if t.contains("HTTP 200)")
    ));
    let http = Http::new(vec![live("100000-aa")]);
    let Some(Outcome::Failed(text)) = run_with(&s, &Tokens::owner(), &http, 1.0) else {
        panic!()
    };
    assert!(
        text.contains("couldn't confirm the publish")
            && text.contains("may still have gone through")
    );
    let http = Http::new(vec![
        live("100000-aa"),
        HttpResponse {
            status: 500,
            body: "boom".into(),
        },
    ]);
    let Some(Outcome::Failed(text)) = run_with(&s, &Tokens::owner(), &http, 1.0) else {
        panic!()
    };
    assert!(text.contains("(HTTP 500: boom)."), "{text}");
    assert_eq!(s.lib.entries()[0].version, "100000-aa");
}

#[test]
fn a_slow_read_leaves_no_time_to_publish() {
    let s = setup("100000-aa");
    let http = Http::new(vec![live("100000-aa"), ok(json!({"version": "200000-bb"}))]);
    let Some(Outcome::Failed(text)) = run_with(&s, &Tokens::owner(), &http, 25.0) else {
        panic!()
    };
    assert!(text.contains("answered too slowly"));
    assert_eq!(http.sent.borrow().len(), 1, "only the read was sent");
    assert!(http.sent.borrow()[0].timeout_secs <= PERM_TIMEOUT_SECS);
}
