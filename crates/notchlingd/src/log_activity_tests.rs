use crate::log_activity::{ActivitySink, Publish, PRUNE_EVERY_MS};
use crate::log_bus::{EnvMaker, Sink};
use crate::log_queue::Stamped;
use notchling_engine::activity::{ActivityRow, DbQuery, EventsDb};
use notchling_events::kinds::choices::{CallerTier, ItemKind};
use notchling_events::kinds::inbox::InboxRaised;
use notchling_events::kinds::security::{ApiRefused, DaemonStopped, RefusedWhy, StopCause};
use notchling_events::spec::Outcome;
use notchling_events::{Actor, ActorTier, Event, Exe, Hex, Millis, Op, Ref, Salt, Via};
use serde_json::Value;
use std::sync::{Arc, Mutex};

const T0: i64 = 1_800_000_000_000;

struct Rig {
    db: Arc<Mutex<EventsDb>>,
    env: EnvMaker,
    published: Arc<Mutex<Vec<(String, String, Value)>>>,
}

fn rig() -> (Rig, ActivitySink) {
    let db = Arc::new(Mutex::new(EventsDb::memory().unwrap()));
    let published = Arc::new(Mutex::new(Vec::new()));
    let seen = published.clone();
    let publish: Publish = Arc::new(move |topic, event, data| {
        seen.lock()
            .unwrap()
            .push((topic.to_owned(), event.to_owned(), data));
    });
    let sink = ActivitySink::new(db.clone(), Salt([5; 32]), publish);
    let env = EnvMaker::new(
        Hex::new("abcdef0123456789").unwrap(),
        Hex::new("b00710ad").unwrap(),
        7,
        1,
    );
    (Rig { db, env, published }, sink)
}

impl Rig {
    fn stamp(&self, event: Event) -> Stamped {
        Stamped {
            env: self.env.make(Hex::new("00aa11bb"), Some(agent())),
            event,
            commit: None,
        }
    }

    fn rows(&self) -> Vec<ActivityRow> {
        self.db
            .lock()
            .unwrap()
            .list(&DbQuery {
                limit: 100,
                ..DbQuery::default()
            })
            .unwrap()
    }
}

fn agent() -> Actor {
    Actor {
        tier: ActorTier::Agent,
        via: Some(Via::Cli),
        exe: Some(Exe::named("codex")),
        device: None,
        session: Some(Ref::new("sess-1")),
    }
}

fn refused(ts: i64) -> Event {
    Event::new(
        &ApiRefused {
            op: Op::named("session.reply"),
            exe: Some(Exe::named("codex")),
            tier: CallerTier::Agent,
            why: RefusedWhy::Forbidden,
            switch: None,
        },
        Outcome::Refused,
        ts,
    )
}

fn stopped(ts: i64) -> Event {
    Event::new(
        &DaemonStopped {
            cause: StopCause::Quit,
            uptime_ms: Millis(5),
        },
        Outcome::Info,
        ts,
    )
}

#[test]
fn writes_activity_kinds_only() {
    let (rig, mut sink) = rig();
    let batch = vec![rig.stamp(stopped(T0)), rig.stamp(refused(T0 + 1))];
    let result = sink.write(&batch, &[None, None]);
    assert!(!result.failed);
    let rows = rig.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "api.refused");
    assert_eq!(rows[0].outcome, "refused");
    assert_eq!(rows[0].actor_tier.as_deref(), Some("agent"));
    assert_eq!(rows[0].actor_exe.as_deref(), Some("codex"));
    assert_eq!(rows[0].actor_session.as_deref(), Some("sess-1"));
    assert_eq!(rows[0].op_id.as_deref(), Some("00aa11bb"));
    assert_eq!(rows[0].host, "abcdef0123456789");
    assert_eq!(rows[0].fields()["op"], "session.reply");
    assert!(rows[0].expires > T0);
}

#[test]
fn audit_n_follows_the_audit_sink() {
    let (rig, mut sink) = rig();
    let batch = vec![rig.stamp(refused(T0)), rig.stamp(refused(T0 + 1))];
    sink.write(&batch, &[Some(41), None]);
    let mut rows = rig.rows();
    rows.sort_by_key(|r| r.seq);
    assert_eq!(rows[0].audit_n, Some(41));
    assert_eq!(rows[1].audit_n, None);
}

#[test]
fn item_refs_are_hashed() {
    let (rig, mut sink) = rig();
    let event = Event::new(
        &InboxRaised {
            item_kind: ItemKind::SecretApproval,
            item: Ref::new("vault-approval:TICKET-SECRET"),
            session: None,
        },
        Outcome::Waiting,
        T0,
    );
    sink.write(&[rig.stamp(event)], &[None]);
    let rows = rig.rows();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].f.contains("TICKET-SECRET"), "{}", rows[0].f);
    assert!(rows[0].f.contains("h:"), "{}", rows[0].f);
}

#[test]
fn publishes_log_event() {
    let (rig, mut sink) = rig();
    sink.write(&[rig.stamp(refused(T0))], &[Some(3)]);
    let published = rig.published.lock().unwrap();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].0, "log");
    assert_eq!(published[0].1, "log.event");
    assert_eq!(published[0].2["event"], "api.refused");
    assert_eq!(published[0].2["kind"], "refused");
    assert_eq!(published[0].2["audit_n"], 3);
}

#[test]
fn hourly_prune_writes_log_pruned() {
    let (rig, mut sink) = rig();
    let mut old = ActivityRow::blank(1, T0, "op.called");
    old.expires = T0 + 10;
    rig.db.lock().unwrap().insert(&[old]).unwrap();
    let now = T0 + 1_000;
    let ticked = sink.tick(now);
    assert_eq!(ticked.events.len(), 1);
    let event = &ticked.events[0];
    assert_eq!(event.spec.name, "log.pruned");
    assert!(format!("{:?}", event.fields).contains("retention"));
    assert!(rig.rows().is_empty());
    let again = sink.tick(now + PRUNE_EVERY_MS - 1);
    assert!(again.events.is_empty());
}

#[test]
fn a_small_cap_trims_oldest_rows_and_says_so() {
    let (rig, sink) = rig();
    let mut sink = sink.with_cap(1);
    let rows: Vec<ActivityRow> = (0..2_000)
        .map(|i| {
            let mut row = ActivityRow::blank(i + 1, T0 + i as i64, "op.called");
            row.f = format!("{{\"op\":\"{}\"}}", "x".repeat(200));
            row.expires = i64::MAX;
            row
        })
        .collect();
    rig.db.lock().unwrap().insert(&rows).unwrap();
    let ticked = sink.tick(T0 + 5_000);
    let names: Vec<&str> = ticked.events.iter().map(|e| e.spec.name).collect();
    assert_eq!(names, vec!["log.pruned"]);
    assert!(format!("{:?}", ticked.events[0].fields).contains("size_cap"));
}
