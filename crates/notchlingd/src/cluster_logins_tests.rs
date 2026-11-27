use super::*;
use crate::acting_ops::now_ms;
use crate::cluster_redact::{mask_tail, MAX_STANDING};
use crate::cluster_testkit::{Dir, FakeCalls};
use crate::grant_use::tests::{cluster, id, Fake};
use notchling_engine::cluster::login_files::LoginStore;
use std::sync::atomic::{AtomicI64, Ordering};

const MARKER: &str = "NL-MARKER-810";
const MINUTE: i64 = 60_000;

struct Rig {
    dir: Dir,
    fake: Arc<Fake>,
    masks: Arc<TailMasks>,
    hold: Arc<LoginHold>,
    skew: Arc<AtomicI64>,
}

fn rig_with(dir: Dir, home: &str, enabled: bool) -> Rig {
    let fake = Arc::new(Fake::default());
    *fake.state.lock().unwrap() = Some(cluster(home, &[]));
    *fake.me.lock().unwrap() = Some(id("worker"));
    let masks = Arc::new(TailMasks::new());
    let skew = Arc::new(AtomicI64::new(0));
    let clock_skew = skew.clone();
    let hold = LoginHold::new(
        (
            Arc::new(LoginStore::new(dir.path())),
            masks.clone(),
            fake.clone(),
        ),
        (dir.path(), enabled),
        Arc::new(move || now_ms() + clock_skew.load(Ordering::SeqCst)),
    );
    Rig {
        dir,
        fake,
        masks,
        hold,
        skew,
    }
}

fn rig_over(dir: Dir, home: &str) -> Rig {
    rig_with(dir, home, true)
}

fn rig(tag: &str) -> Rig {
    rig_over(Dir::new(tag), "vault")
}

fn from_vault() -> Origin {
    Origin {
        node: id("vault"),
        tier: OriginTier::Node,
    }
}

fn args(value: &str, expires_ms: i64) -> Value {
    json!({"tool": "claude", "value": value, "expiresMs": expires_ms})
}

fn good() -> Value {
    args(MARKER, now_ms() + 30 * MINUTE)
}

fn token_text(rig: &Rig) -> Option<String> {
    rig.hold.token().map(|t| t.expose().to_string())
}

#[cfg(unix)]
#[test]
fn the_vault_node_while_home_stores_the_login() {
    let rig = rig("login-store");
    assert!(!rig.hold.standing());
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    assert!(rig.hold.standing());
    assert_eq!(token_text(&rig).as_deref(), Some(MARKER));
}

#[cfg(unix)]
#[test]
fn the_login_file_is_private_and_where_snapshots_skip() {
    use std::os::unix::fs::PermissionsExt;
    let rig = rig("login-file");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    let file = rig.dir.path().join("logins/claude.json");
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&file), 0o600);
    assert_eq!(mode(&rig.dir.path().join("logins")), 0o700);
    assert!(notchling_engine::cluster::grant_files::EXCLUDED_DIRS.contains(&"cluster/logins"));
}

#[test]
fn nobody_else_delivers_a_login() {
    let rig = rig("login-senders");
    let other = |name: &str, tier| Origin {
        node: id(name),
        tier,
    };
    for origin in [
        other("worker", OriginTier::Node),
        other("other", OriginTier::Node),
        other("vault", OriginTier::Agent),
        other("vault", OriginTier::App),
    ] {
        let err = rig.hold.deliver(&origin, &good()).expect_err("refused");
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert!(
            !err.message.contains("NL-MARKER"),
            "an error held the value"
        );
    }
    assert!(!rig.hold.standing());
}

#[test]
fn the_vault_node_must_be_the_primary_to_deliver() {
    let rig = rig_over(Dir::new("login-not-home"), "other");
    let err = rig
        .hold
        .deliver(&from_vault(), &good())
        .expect_err("refused");
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(!rig.hold.standing());
    *rig.fake.state.lock().unwrap() = None;
    assert!(rig.hold.deliver(&from_vault(), &good()).is_err());
}

#[test]
fn a_bad_delivery_is_refused_without_echoing_the_value() {
    let rig = rig("login-bad");
    let soon = now_ms() + 30 * MINUTE;
    let cases = [
        args("two words", soon),
        args("", soon),
        args("line\nbreak", soon),
        args(MARKER, now_ms() - MINUTE),
        json!({"tool": "codex", "value": MARKER, "expiresMs": soon}),
        json!({"value": MARKER, "expiresMs": soon}),
        json!({"tool": "claude", "expiresMs": soon}),
        json!({"tool": "claude", "value": MARKER}),
    ];
    for case in cases {
        let err = rig.hold.deliver(&from_vault(), &case).expect_err("refused");
        assert!(
            !err.message.contains("NL-MARKER"),
            "an error held the value"
        );
    }
    assert!(!rig.hold.standing());
}

#[cfg(unix)]
#[test]
fn the_login_ends_on_the_day_it_was_sent_with_no_twelve_hour_cap() {
    let rig = rig("login-long");
    let ten_days = now_ms() + 10 * 86_400_000;
    rig.hold
        .deliver(&from_vault(), &args(MARKER, ten_days))
        .expect("stored");
    rig.skew.store(2 * 86_400_000, Ordering::SeqCst);
    assert_eq!(token_text(&rig).as_deref(), Some(MARKER));
    rig.skew.store(11 * 86_400_000, Ordering::SeqCst);
    assert!(rig.hold.token().is_none());
    assert!(!rig.hold.standing());
}

#[cfg(unix)]
#[test]
fn a_new_delivery_replaces_the_login() {
    let rig = rig("login-replace");
    rig.hold.deliver(&from_vault(), &good()).expect("first");
    rig.hold
        .deliver(
            &from_vault(),
            &args("NL-MARKER-811", now_ms() + MINUTE * 40),
        )
        .expect("second");
    assert_eq!(token_text(&rig).as_deref(), Some("NL-MARKER-811"));
}

fn masked(masks: &TailMasks, job: &str, text: &str) -> String {
    let (out, _) = mask_tail(masks, job, 0, &[], text.as_bytes(), true).expect("masked");
    String::from_utf8(out).unwrap()
}

#[cfg(unix)]
#[test]
fn the_token_is_masked_in_every_served_tail_even_for_jobs_with_no_grants() {
    let rig = rig("login-masks");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    let text = format!("the token is {MARKER} and again {MARKER}");
    let out = masked(&rig.masks, "j-0123456789ab", &text);
    assert!(!out.contains("NL-MARKER"), "a tail served the token");
    assert!(out.contains("[secret claude-worker]"));
    let union = rig.masks.union().expect("a union");
    let (out, _) =
        crate::cluster_redact::mask_with(union, 0, &[], text.as_bytes(), true).expect("masked");
    assert!(!String::from_utf8(out).unwrap().contains("NL-MARKER"));
}

#[cfg(unix)]
#[test]
fn the_token_stays_masked_after_it_is_replaced() {
    let rig = rig("login-masks-old");
    rig.hold.deliver(&from_vault(), &good()).expect("first");
    rig.hold
        .deliver(
            &from_vault(),
            &args("NL-MARKER-812", now_ms() + MINUTE * 40),
        )
        .expect("second");
    let out = masked(
        &rig.masks,
        "j-0123456789ab",
        &format!("{MARKER} NL-MARKER-812"),
    );
    assert!(!out.contains("NL-MARKER"));
}

#[cfg(unix)]
#[test]
fn a_restart_masks_the_stored_login_again() {
    let rig = rig("login-restart");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    let again = rig_over(rig.dir, "vault");
    let out = masked(&again.masks, "j-0123456789ab", MARKER);
    assert!(!out.contains("NL-MARKER"));
    assert!(again.hold.standing());
}

#[cfg(unix)]
#[test]
fn too_many_different_tokens_since_the_start_stop_serving_a_new_one() {
    let rig = rig("login-many");
    for n in 0..MAX_STANDING {
        let value = format!("NL-MARKER-9{n:02}");
        rig.hold
            .deliver(&from_vault(), &args(&value, now_ms() + MINUTE * 40))
            .expect("stored");
    }
    let err = rig
        .hold
        .deliver(
            &from_vault(),
            &args("NL-MARKER-999", now_ms() + MINUTE * 40),
        )
        .expect_err("refused");
    assert!(!err.message.contains("NL-MARKER"));
}

#[cfg(unix)]
#[test]
fn leaving_the_cluster_removes_the_login() {
    let rig = rig("login-wipe");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    rig.hold.wipe();
    assert!(!rig.hold.standing());
    assert!(rig.hold.token().is_none());
    rig.hold.wipe();
}

#[cfg(unix)]
#[test]
fn the_calls_answer_only_login_deliver() {
    let rig = rig("login-calls");
    let inner = Arc::new(FakeCalls::default());
    let calls = LoginCalls::new(inner.clone(), rig.hold.clone());
    calls
        .call(LOGIN_DELIVER, &good(), &from_vault())
        .expect("stored");
    assert!(rig.hold.standing());
    assert!(inner.calls.lock().unwrap().is_empty());
    let _ = calls.call("grant.revoke", &json!({}), &from_vault());
    assert_eq!(inner.calls.lock().unwrap().len(), 1);
    assert_eq!(NODE_OPS, [LOGIN_DELIVER, LOGIN_REVOKE]);
    assert_eq!(LOGIN_REVOKE, "login.revoke");
    assert_eq!(LOGIN_DELIVER, "login.deliver");
}

#[cfg(unix)]
#[test]
fn the_login_is_hidden_when_printed() {
    let rig = rig("login-print");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    let token = rig.hold.token().expect("token");
    assert!(!format!("{token:?}{token}").contains("NL-MARKER"));
}

#[cfg(unix)]
#[test]
fn the_versions_a_node_reports_follow_the_login_while_it_stands() {
    use crate::cluster_versions::{own, with_logins};
    use notchling_engine::versions::Tool;
    let rig = rig("login-versions");
    let report = |rig: &Rig| {
        with_logins(own(), rig.hold.standing())
            .logins
            .get(&Tool::Claude)
            .copied()
    };
    assert_eq!(report(&rig), None);
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    assert_eq!(report(&rig), Some(true));
    rig.skew.store(2 * 3_600_000, Ordering::SeqCst);
    assert_eq!(report(&rig), None);
}

fn session_row(rig: &Rig, session: &str, text: &str) -> Result<String, String> {
    let name = crate::cluster_logins_jobs::lost_name(session);
    if rig.masks.job_for_key(session).as_deref() != Some(name.as_str()) {
        return Ok(masked(&rig.masks, "j-0123456789ab", text));
    }
    mask_tail(&rig.masks, &name, 0, &[], text.as_bytes(), true)
        .map(|(out, _)| String::from_utf8(out).unwrap())
}

#[cfg(unix)]
#[test]
fn a_job_that_ran_with_the_current_token_stays_readable_after_a_restart() {
    let rig = rig("login-job-same");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    let token = rig.hold.token().expect("token");
    rig.hold.launched("6ce4c19c", &token).expect("recorded");
    let again = rig_over(rig.dir, "vault");
    let out = session_row(&again, "6ce4c19c", &format!("x {MARKER} y")).expect("served");
    assert!(!out.contains("NL-MARKER"));
    assert!(again.masks.job_for_key("6ce4c19c").is_none());
}

#[cfg(unix)]
#[test]
fn a_job_that_ran_with_an_earlier_token_is_refused_after_a_restart() {
    let rig = rig("login-job-old");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    let old = rig.hold.token().expect("token");
    rig.hold.launched("6ce4c19c", &old).expect("recorded");
    rig.hold
        .deliver(
            &from_vault(),
            &args("NL-MARKER-840", now_ms() + MINUTE * 40),
        )
        .expect("replaced");
    let new = rig.hold.token().expect("token");
    rig.hold.launched("6ce4c19d", &new).expect("recorded");
    let again = rig_over(rig.dir, "vault");
    let lost = session_row(&again, "6ce4c19c", &format!("x {MARKER} y"));
    assert!(lost.is_err(), "an old-token job was served");
    assert!(!lost.unwrap_err().contains("NL-MARKER"));
    let kept = session_row(&again, "6ce4c19d", "x NL-MARKER-840 y").expect("served");
    assert!(!kept.contains("NL-MARKER"));
}

#[cfg(unix)]
#[test]
fn after_the_login_is_gone_every_recorded_job_is_refused_after_a_restart() {
    let rig = rig("login-job-gone");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    let token = rig.hold.token().expect("token");
    rig.hold.launched("6ce4c19c", &token).expect("recorded");
    rig.hold.wipe();
    let again = rig_over(rig.dir, "vault");
    assert!(session_row(&again, "6ce4c19c", "x").is_err());
}

#[cfg(unix)]
#[test]
fn the_record_holds_a_hash_and_never_the_value() {
    let rig = rig("login-job-file");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    let token = rig.hold.token().expect("token");
    rig.hold.launched("6ce4c19c", &token).expect("recorded");
    let text = std::fs::read_to_string(rig.dir.path().join("logins/jobs.json")).expect("record");
    assert!(!text.contains("NL-MARKER"), "the record held the value");
    assert!(text.contains(&crate::cluster_logins_jobs::hash_of(&token)));
}

#[cfg(unix)]
#[test]
fn a_record_that_cannot_be_read_stops_new_launches() {
    let rig = rig("login-job-bad");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    std::fs::write(rig.dir.path().join("logins/jobs.json"), "{not json").unwrap();
    let again = rig_over(rig.dir, "vault");
    assert!(again.hold.token().is_none());
    assert!(!again.hold.standing());
}

#[cfg(unix)]
#[test]
fn an_unreadable_record_refuses_every_unmapped_row() {
    let rig = rig("login-job-blanket");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    assert!(!rig.masks.unidentified_lost());
    std::fs::write(rig.dir.path().join("logins/jobs.json"), "{not json").unwrap();
    let again = rig_over(rig.dir, "vault");
    assert!(again.masks.unidentified_lost());
}

#[cfg(unix)]
#[test]
fn the_stored_token_is_masked_at_start_even_when_the_record_is_unreadable() {
    let rig = rig("login-job-stored");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    std::fs::write(rig.dir.path().join("logins/jobs.json"), "{not json").unwrap();
    let again = rig_over(rig.dir, "vault");
    assert!(again.hold.token().is_none());
    let out = masked(&again.masks, "j-0123456789ab", &format!("x {MARKER} y"));
    assert!(!out.contains("NL-MARKER"), "{out}");
    assert!(again.masks.login_lost());
}

#[cfg(unix)]
#[test]
fn recorded_jobs_are_lost_after_a_restart_even_while_the_login_is_off() {
    let rig = rig("login-job-off");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    let token = rig.hold.token().expect("token");
    rig.hold.launched("6ce4c19c", &token).expect("recorded");
    let off = rig_with(rig.dir, "vault", false);
    assert!(session_row(&off, "6ce4c19c", &format!("x {MARKER} y")).is_err());
    assert!(!off.masks.unidentified_lost());
}

#[test]
fn a_full_record_keeps_taking_launches_and_folds_the_older_hashes_into_an_overflow() {
    let dir = Dir::new("login-job-cap");
    let logins = dir.path().join("logins");
    let jobs = crate::cluster_logins_jobs::LoginJobs::load_capped(&logins, 4);
    for n in 0..4 {
        jobs.record(&format!("old{n}"), "hash-old")
            .expect("recorded");
    }
    for n in 0..4 {
        jobs.record(&format!("new{n}"), "hash-new")
            .expect("recorded");
        assert!(jobs.usable());
    }
    assert!(jobs.all().len() <= 4);
    let kept = jobs.all();
    let overflow = jobs.overflow();
    for n in 0..4 {
        let old = format!("old{n}");
        let new = format!("new{n}");
        assert!(kept.iter().any(|(s, _)| *s == old) || overflow.contains(&"hash-old".to_string()));
        assert!(kept.iter().any(|(s, _)| *s == new) || overflow.contains(&"hash-new".to_string()));
    }
    let again = crate::cluster_logins_jobs::LoginJobs::load_capped(&logins, 4);
    assert_eq!(again.overflow(), jobs.overflow());
    assert_eq!(again.all(), jobs.all());
}

#[cfg(unix)]
#[test]
fn an_overflow_of_an_earlier_token_refuses_unmapped_rows_after_a_restart_but_the_current_one_does_not(
) {
    let rig = rig("login-job-overflow");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    let current = crate::cluster_logins_jobs::hash_of(&rig.hold.token().expect("token"));
    let file = rig.dir.path().join("logins/jobs.json");
    std::fs::write(
        &file,
        format!(r#"{{"v":1,"jobs":{{}},"overflow":["{current}"]}}"#),
    )
    .unwrap();
    let same = rig_over(rig.dir, "vault");
    assert!(!same.masks.unidentified_lost());
    std::fs::write(&file, r#"{"v":1,"jobs":{},"overflow":["beef"]}"#).unwrap();
    let other = rig_over(same.dir, "vault");
    assert!(other.masks.unidentified_lost());
}

#[cfg(unix)]
#[test]
fn a_disabled_hold_takes_nothing_serves_nothing_and_removes_an_old_file() {
    let rig = rig("login-off");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    let off = rig_with(rig.dir, "vault", false);
    assert!(!off.dir.path().join("logins/claude.json").exists());
    let err = off.hold.deliver(&from_vault(), &good()).expect_err("off");
    assert!(err.message.contains("inherit"));
    assert!(off.hold.token().is_none());
    assert!(!off.hold.standing());
}

#[test]
fn the_cluster_login_is_off_in_this_build() {
    const { assert!(!CLUSTER_LOGIN_ON) };
}

#[cfg(unix)]
#[test]
fn standing_and_token_agree_once_the_masks_are_full() {
    let rig = rig("login-agree");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    assert!(rig.hold.standing());
    for n in 1..MAX_STANDING {
        let value = format!("NL-MARKER-8{n:02}");
        rig.masks
            .add_standing(vec![("v".to_string(), GrantValue::new(value))])
            .unwrap();
    }
    LoginStore::new(rig.dir.path())
        .put(&notchling_engine::cluster::login_files::LoginFile {
            tool: "claude".to_string(),
            expires_ms: now_ms() + 30 * MINUTE,
            value: GrantValue::new("NL-MARKER-899".to_string()),
        })
        .expect("a file the masks can't take");
    assert!(rig.hold.token().is_none());
    assert!(!rig.hold.standing());
}

#[cfg(unix)]
#[test]
fn the_home_or_the_vault_node_revokes_and_nobody_else() {
    let rig = rig("login-revoke");
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    let from = |name: &str, tier| Origin {
        node: id(name),
        tier,
    };
    for origin in [
        from("other", OriginTier::Node),
        from("worker", OriginTier::Node),
        from("vault", OriginTier::Agent),
    ] {
        assert!(rig.hold.revoke(&origin).is_err());
    }
    assert!(rig.hold.standing());
    rig.hold
        .revoke(&from("vault", OriginTier::Node))
        .expect("revoked");
    assert!(!rig.hold.standing());
    let calls = LoginCalls::new(Arc::new(FakeCalls::default()), rig.hold.clone());
    rig.hold.deliver(&from_vault(), &good()).expect("stored");
    *rig.fake.state.lock().unwrap() = Some(cluster("other", &[]));
    calls
        .call(LOGIN_REVOKE, &json!({}), &from("other", OriginTier::Node))
        .expect("the home revokes");
    assert!(!rig.hold.standing());
}
