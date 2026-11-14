use super::*;
use crate::acting_ops_tests::{app, FakeRoom};
use crate::event_assert::{of, results, text};
use crate::limits_ops_tests::TempDir;
use notchling_engine::limits::mode::CLAUDE;
use notchling_events::{MemorySink, Salt};
use notchling_local_api::ClientKind;
use std::sync::atomic::Ordering;

struct Rig {
    tmp: TempDir,
    room: Arc<FakeRoom>,
    inbox: Arc<InboxOps>,
    limits: Arc<LimitConfig>,
    sink: Arc<MemorySink>,
    ops: SettingsOps,
}

fn agent() -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: Some(PathBuf::from("/agents/notchling")),
        pid: None,
    }
}

fn rig(tag: &str, wait: Duration) -> Rig {
    let tmp = TempDir::new(&format!("settings-{tag}"));
    let room = Arc::new(FakeRoom::default());
    let inbox = InboxOps::new(tmp.path("inbox"), room.clone());
    let limits = Arc::new(LimitConfig::in_memory());
    let states: SwitchStates = Arc::new(|name: &str| match name {
        "acting" => Some(true),
        "vault" => Some(false),
        _ => None,
    });
    let sink = Arc::new(MemorySink::new());
    let ops = SettingsOps::new(tmp.path("data"), room.clone(), states)
        .with_events(Events::fixed(sink.clone(), Salt([1; 32])))
        .with_limits(limits.clone())
        .with_inbox(inbox.clone())
        .with_ask_wait(wait);
    Rig {
        tmp,
        room,
        inbox,
        limits,
        sink,
        ops,
    }
}

impl Rig {
    fn call(&self, caller: &Caller, op: &str, args: Value) -> Result<Value, ApiError> {
        self.ops.call(caller, op, &args)
    }

    fn stored(&self, key: &str) -> Option<Value> {
        Settings::open(&self.tmp.path("data/settings.json"))
            .unwrap()
            .raw(key)
            .cloned()
    }

    fn audit(&self) -> Vec<notchling_events::Event> {
        of(&self.sink, "settings.agent")
    }

    fn results(&self) -> Vec<String> {
        results(&self.sink, "settings.agent")
    }

    fn app_follows_the_inbox(&self) {
        self.room.app.store(true, Ordering::SeqCst);
        self.room
            .following
            .lock()
            .unwrap()
            .push(topics::INBOX.into());
    }

    fn item(&self, id: &str) -> Value {
        self.inbox.flush();
        let list = self
            .inbox
            .call(&app(), ops::INBOX_LIST, &json!({}))
            .unwrap();
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == item_id(id))
            .cloned()
            .unwrap_or(Value::Null)
    }
}

fn entry<'a>(list: &'a Value, key: &str) -> &'a Value {
    list["settings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["key"] == key)
        .unwrap_or_else(|| panic!("{key} is not listed"))
}

#[test]
fn every_op_is_agent_tier_on_the_settings_topic() {
    let r = rig("spec", ASK_WAIT);
    for op in SETTINGS_OPS {
        assert_eq!(r.ops.spec(op), Some(OpSpec::AGENT), "{op}");
    }
    assert_eq!(r.ops.spec(ops::SETTINGS_SET_SWITCH), None);
    assert_eq!(r.ops.topics(), &[topics::SETTINGS]);
}

#[test]
fn an_agent_lists_and_gets_every_setting_with_its_class() {
    let r = rig("list", ASK_WAIT);
    let list = r.call(&agent(), ops::SETTINGS_LIST, json!({})).unwrap();
    let acting = entry(&list, "switch.acting");
    assert_eq!(acting["value"], true);
    assert_eq!(acting["class"], "never");
    assert_eq!(acting["switch"], true);
    assert_eq!(entry(&list, "switch.vault")["value"], false);
    assert_eq!(entry(&list, "switch.bridge")["value"], Value::Null);
    let history = entry(&list, "chatHistory");
    assert_eq!(history["class"], "free");
    assert_eq!(history["value"], "4mb");
    assert_eq!(history["title"], "Conversation history to load");
    assert_eq!(history["allowed"], json!(["2mb", "4mb", "8mb", "16mb"]));
    assert_eq!(entry(&list, "limitSwitchAt")["range"], json!([80, 99]));
    assert_eq!(entry(&list, "limitMode")["class"], "ask");
    let accounts = entry(&list, "accounts");
    assert_eq!(accounts["class"], "never");
    assert!(accounts.get("value").is_none(), "an agent never sees it");
    assert_eq!(list["settings"].as_array().unwrap().len(), KEYS.len() + 5);
    let full = r.call(&app(), ops::SETTINGS_LIST, json!({})).unwrap();
    assert!(entry(&full, "accounts").get("value").is_some());

    let got = r
        .call(&agent(), ops::SETTINGS_GET, json!({"key": "showPet"}))
        .unwrap();
    assert_eq!(got["value"], true);
    assert_eq!(got["kind"], "bool");
    let vault = r
        .call(&agent(), ops::SETTINGS_GET, json!({"key": "vault"}))
        .unwrap();
    assert_eq!(vault["key"], "switch.vault");
    let e = r
        .call(&agent(), ops::SETTINGS_GET, json!({"key": "history"}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    assert!(
        e.message.contains("No setting named history"),
        "{}",
        e.message
    );
    assert!(e.message.contains("chatHistory"), "{}", e.message);
    assert!(r.audit().is_empty(), "reads are not audited");
}

#[test]
fn a_free_key_is_set_at_once_published_and_audited() {
    let r = rig("free", ASK_WAIT);
    let reply = r
        .call(
            &agent(),
            ops::SETTINGS_SET,
            json!({"key": "chatHistory", "value": "8MB"}),
        )
        .unwrap();
    assert_eq!(
        reply,
        json!({"key": "chatHistory", "value": "8mb", "state": "applied", "changed": true})
    );
    assert_eq!(r.stored("chatHistory"), Some(json!("8mb")));
    assert_eq!(
        r.room.named(topics::SETTINGS_CHANGED),
        vec![json!({"key": "chatHistory", "value": "8mb", "by": "agent"})]
    );
    let again = r
        .call(
            &agent(),
            ops::SETTINGS_SET,
            json!({"key": "showPet", "value": false}),
        )
        .unwrap();
    assert_eq!(again["value"], false);
    assert_eq!(r.stored("showPet"), Some(json!(false)));
    let audit = r.audit();
    assert_eq!(audit.len(), 2);
    assert_eq!(text(&audit[0], "key").as_deref(), Some("chatHistory"));
    assert_eq!(text(&audit[0], "class").as_deref(), Some("free"));
    assert_eq!(text(&audit[0], "result").as_deref(), Some("set"));
    assert_eq!(text(&audit[0], "value_choice").as_deref(), Some("8mb"));
    assert_eq!(text(&audit[1], "value_flag").as_deref(), Some("false"));
    assert!(r.inbox.items().is_empty(), "nothing to ask");
}

#[test]
fn switches_and_never_keys_are_refused_and_audited() {
    let r = rig("never", ASK_WAIT);
    for (key, value) in [
        ("switch.acting", json!(true)),
        ("acting", json!("on")),
        ("switch.accounts", json!(true)),
        ("install", json!(true)),
        ("bridgeEnabled", json!(true)),
        ("codexNetwork", json!(true)),
        ("accounts", json!("[]")),
    ] {
        let e = r
            .call(
                &agent(),
                ops::SETTINGS_SET,
                json!({"key": key, "value": value}),
            )
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Forbidden, "{key}");
        assert!(r.stored(key).is_none(), "{key}");
    }
    let e = r
        .call(
            &app(),
            ops::SETTINGS_SET,
            json!({"key": "switch.vault", "value": true}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden, "not even the app, here");
    let e = r
        .call(
            &agent(),
            ops::SETTINGS_SET,
            json!({"key": "codexNetwork", "value": true}),
        )
        .unwrap_err();
    assert!(e.message.contains("sandbox permission"), "{}", e.message);
    assert!(r.room.named(topics::SETTINGS_CHANGED).is_empty());
    assert!(r.inbox.items().is_empty());
    let results = r.results();
    assert_eq!(results.len(), 8, "the app's refusal is the app's own");
    assert!(results.iter().all(|r| r == "refused"), "{results:?}");
}

#[test]
fn bad_values_and_unknown_keys_are_refused_with_what_they_take() {
    let r = rig("bad", ASK_WAIT);
    r.app_follows_the_inbox();
    for (key, value, says) in [
        (
            "chatHistory",
            json!("1gb"),
            "chatHistory takes one of 2mb, 4mb, 8mb, 16mb.",
        ),
        ("showPet", json!("maybe"), "showPet takes true or false."),
        (
            "limitSwitchAt",
            json!("120"),
            "limitSwitchAt takes a whole number from 80 to 99.",
        ),
        (
            "notchWidth",
            json!(3),
            "notchWidth takes one of small, medium, large.",
        ),
    ] {
        let e = r
            .call(
                &agent(),
                ops::SETTINGS_SET,
                json!({"key": key, "value": value}),
            )
            .unwrap_err();
        assert_eq!((e.code, e.message.as_str()), (ErrorCode::BadRequest, says));
    }
    let e = r
        .call(
            &agent(),
            ops::SETTINGS_SET,
            json!({"key": "theme", "value": "light"}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    assert!(
        e.message.starts_with("No setting named theme."),
        "{}",
        e.message
    );
    let e = r
        .call(&agent(), ops::SETTINGS_SET, json!({"key": "showPet"}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    assert!(r.inbox.items().is_empty(), "a bad value is never asked");
    assert_eq!(
        r.results(),
        [
            "bad_value",
            "bad_value",
            "bad_value",
            "bad_value",
            "unknown_key"
        ]
    );
}

fn ask(r: &Rig, key: &str, value: Value) -> String {
    let reply = r
        .call(
            &agent(),
            ops::SETTINGS_SET,
            json!({"key": key, "value": value}),
        )
        .unwrap();
    assert_eq!(reply["state"], "waiting", "{reply}");
    reply["id"].as_str().unwrap().to_string()
}

fn asked(r: &Rig, id: &str) -> Value {
    r.call(&agent(), ops::SETTINGS_ASKED, json!({"id": id, "wait": 5}))
        .unwrap()
}

#[test]
fn an_ask_key_raises_an_inbox_item_and_allow_applies_it() {
    let r = rig("allow", ASK_WAIT);
    r.app_follows_the_inbox();
    let id = ask(&r, "limitMode", json!("notify"));
    let item = r.item(&id);
    assert_eq!(item["kind"], "setting_change");
    assert_eq!(item["title"], "Set Claude Code limit to notify");
    assert!(
        item["detail"]
            .as_str()
            .unwrap()
            .ends_with("Now notifyAndSwitch."),
        "{item}"
    );
    let actions: Vec<&str> = item["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(actions, ["allow", "deny"]);
    assert!(
        r.stored("limitMode").is_none(),
        "nothing changes before the user allows"
    );
    let got = r
        .call(&agent(), ops::SETTINGS_GET, json!({"key": "limitMode"}))
        .unwrap();
    assert_eq!(got["pending"], json!([{"id": id, "value": "notify"}]));

    let decided = r
        .inbox
        .decide(&item_id(&id), ActionId::Allow, "phone")
        .unwrap();
    assert!(decided.answered);
    let done = asked(&r, &id);
    assert_eq!(done["state"], "applied", "{done}");
    assert_eq!(r.stored("limitMode"), Some(json!("notify")));
    assert_eq!(r.limits.mode(&CLAUDE), LimitMode::Notify);
    assert_eq!(
        r.room.named(topics::SETTINGS_CHANGED),
        vec![json!({"key": "limitMode", "value": "notify", "by": "agent", "ask": id})]
    );
    assert_eq!(r.item(&id)["answered_by"], "phone");
    assert_eq!(r.results(), ["asked", "allowed"]);
    let again = r
        .inbox
        .decide(&item_id(&id), ActionId::Deny, "mac")
        .unwrap();
    assert!(!again.answered, "the first answer wins");
}

#[test]
fn deny_leaves_the_setting_as_it_was() {
    let r = rig("deny", ASK_WAIT);
    r.app_follows_the_inbox();
    let id = ask(&r, "keepPages", json!("false"));
    r.inbox
        .decide(&item_id(&id), ActionId::Deny, "mac")
        .unwrap();
    let done = asked(&r, &id);
    assert_eq!(done["state"], "denied");
    assert_eq!(done["message"], "The user denied it.");
    assert!(r.stored("keepPages").is_none());
    assert!(r.room.named(topics::SETTINGS_CHANGED).is_empty());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while r.results().len() < 2 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(r.results(), ["asked", "denied"]);
}

#[test]
fn an_unanswered_ask_expires_and_closes_its_item() {
    let r = rig("expire", Duration::from_millis(200));
    r.app_follows_the_inbox();
    let id = ask(&r, "continueAfterSleep", json!(false));
    let done = asked(&r, &id);
    assert_eq!(done["state"], "expired", "{done}");
    assert!(r.stored("continueAfterSleep").is_none());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while r.item(&id)["state"] != "expired" && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(r.item(&id)["state"], "expired");
    let late = r
        .inbox
        .decide(&item_id(&id), ActionId::Allow, "mac")
        .unwrap();
    assert!(!late.answered);
    assert!(r.stored("continueAfterSleep").is_none());
}

#[test]
fn an_ask_with_no_app_to_answer_fails_closed() {
    let r = rig("no-app", ASK_WAIT);
    let e = r
        .call(
            &agent(),
            ops::SETTINGS_SET,
            json!({"key": "limitMode", "value": "off"}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsApproval);
    assert!(e.message.contains("the app isn't open"), "{}", e.message);
    assert!(r.inbox.items().is_empty());
    assert_eq!(r.results(), ["no_app"]);
}

#[test]
fn the_app_sets_an_ask_key_without_asking() {
    let r = rig("app", ASK_WAIT);
    let reply = r
        .call(
            &app(),
            ops::SETTINGS_SET,
            json!({"key": "limitSwitchAt", "value": 90}),
        )
        .unwrap();
    assert_eq!(reply["state"], "applied");
    assert_eq!(r.limits.switch_at(), 90);
    assert_eq!(
        r.room.named(topics::SETTINGS_CHANGED),
        vec![json!({"key": "limitSwitchAt", "value": 90, "by": "app"})]
    );
    assert!(r.audit().is_empty(), "an app change is not an agent's");
}

#[test]
fn closing_withdraws_the_asks_still_waiting() {
    let r = rig("close", ASK_WAIT);
    r.app_follows_the_inbox();
    let id = ask(&r, "answerFromNotch", json!(false));
    r.ops.close();
    let ask = r
        .call(&agent(), ops::SETTINGS_ASKED, json!({"id": id}))
        .unwrap();
    assert_eq!(ask["state"], "withdrawn");
    assert_eq!(r.item(&id)["state"], "withdrawn");
    let e = r
        .call(&agent(), ops::SETTINGS_ASKED, json!({"id": "setting-nope"}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
}

#[test]
fn free_set_commits_before_writing() {
    let r = rig("commit-first", ASK_WAIT);
    r.call(
        &agent(),
        ops::SETTINGS_SET,
        json!({"key": "showPet", "value": false}),
    )
    .unwrap();
    assert_eq!(r.sink.commits(), 1);
    assert_eq!(r.results(), ["set"]);
}

#[test]
fn commit_failure_leaves_store_unchanged() {
    let r = rig("commit-fails", ASK_WAIT);
    r.sink.fail_commits(true);
    let e = r
        .call(
            &agent(),
            ops::SETTINGS_SET,
            json!({"key": "showPet", "value": false}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "the audit log can't be written");
    assert!(r.stored("showPet").is_none());
    assert!(r.room.named(topics::SETTINGS_CHANGED).is_empty());
}

#[test]
fn ask_allowed_commits_with_answered_by() {
    let r = rig("ask-by", ASK_WAIT);
    r.app_follows_the_inbox();
    let id = ask(&r, "limitMode", json!("notify"));
    r.inbox
        .decide(&item_id(&id), ActionId::Allow, "phone")
        .unwrap();
    let done = asked(&r, &id);
    assert_eq!(done["state"], "applied", "{done}");
    let events = r.audit();
    let allowed = events
        .iter()
        .find(|e| text(e, "result").as_deref() == Some("allowed"))
        .unwrap();
    assert_eq!(text(allowed, "answered_by").as_deref(), Some("phone"));
    assert_eq!(text(allowed, "class").as_deref(), Some("ask"));
    assert_eq!(r.sink.commits(), 1);
}

#[test]
fn an_allowed_ask_is_not_applied_when_the_commit_fails() {
    let r = rig("ask-commit-fails", ASK_WAIT);
    r.app_follows_the_inbox();
    let id = ask(&r, "limitMode", json!("notify"));
    r.sink.fail_commits(true);
    r.inbox
        .decide(&item_id(&id), ActionId::Allow, "mac")
        .unwrap();
    let done = asked(&r, &id);
    assert_eq!(done["state"], "failed", "{done}");
    assert!(r.stored("limitMode").is_none());
}

#[test]
fn text_value_is_chars_and_hash() {
    let r = rig("text", ASK_WAIT);
    r.app_follows_the_inbox();
    let marker = "ctrl+alt+marker7";
    ask(&r, "searchShortcut", json!(marker));
    let events = r.audit();
    assert_eq!(text(&events[0], "value_chars").as_deref(), Some("16"));
    assert_eq!(text(&events[0], "value_hash").unwrap().len(), 16);
    assert!(!format!("{:?}", r.sink.events()).contains(marker));
}

#[test]
fn unknown_key_is_hashed() {
    let r = rig("unknown-hash", ASK_WAIT);
    let e = r
        .call(
            &agent(),
            ops::SETTINGS_SET,
            json!({"key": "my-private-key", "value": "v"}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    let events = r.audit();
    assert_eq!(text(&events[0], "key").as_deref(), Some("unknown"));
    assert_eq!(text(&events[0], "key_hash").unwrap().len(), 16);
    assert!(!format!("{:?}", r.sink.events()).contains("my-private-key"));
}
