use crate::acting_ops::Room;
use crate::embed_fetch::ModelNet;
use crate::embed_ops::{EmbedOps, EmbedParts, EmbedSettings, PluggedIn};
use crate::embed_paths;
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use notchling_engine::search::chunk::{ChunkKind, ChunkText};
use notchling_engine::search::corpus::{Corpus, Doc, DocStamp, Tier as CorpusTier};
use notchling_engine::search::embed::helper_fake::FakeSpawn;
use notchling_engine::search::embed::{Embedder, HashEmbedder, ModelId};
use notchling_engine::search::hybrid::Semantic;
use notchling_engine::search::query::filter::Filter;
use notchling_engine::search::range::IndexRange;
use notchling_engine::search::store::VectorDb;
use notchling_local_api::{ops, Caller, ClientKind, Tier};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const GRANITE: &str = "granite-fake@1/p1";

#[derive(Default)]
struct Events(Mutex<Vec<(String, String)>>, Mutex<Vec<String>>);

impl Room for Events {
    fn publish(&self, topic: &str, event: &str, data: &Value) {
        self.0
            .lock()
            .unwrap()
            .push((topic.to_string(), event.to_string()));
        if let Some(state) = data["fetch"]["state"].as_str() {
            self.1.lock().unwrap().push(state.to_string());
        }
    }

    fn can_answer(&self) -> bool {
        true
    }

    fn can_follow(&self, _topic: &str) -> bool {
        true
    }
}

struct Notes {
    docs: Vec<(String, String)>,
}

impl Corpus for Notes {
    fn id(&self) -> &'static str {
        "sessions"
    }

    fn tier(&self) -> CorpusTier {
        CorpusTier::Agent
    }

    fn scan(&self, _window: Option<f64>, _now: f64) -> Vec<DocStamp> {
        self.docs
            .iter()
            .map(|(k, _)| DocStamp {
                key: k.clone(),
                stamp: "1".to_string(),
                at: 100.0,
            })
            .collect()
    }

    fn read(&self, doc: &DocStamp, _now: f64) -> Option<Doc> {
        let text = &self.docs.iter().find(|(k, _)| *k == doc.key)?.1;
        Some(Doc {
            key: doc.key.clone(),
            title: doc.key.clone(),
            at: doc.at,
            facets: vec![("tool".to_string(), "claude".to_string())],
            chunks: vec![ChunkText {
                kind: ChunkKind::Head,
                text: text.clone(),
            }],
        })
    }
}

pub(crate) struct Rig {
    pub(crate) ops: Arc<EmbedOps>,
    room: Arc<Events>,
    spawn: FakeSpawn,
    pub(crate) on: Arc<Mutex<EmbedSettings>>,
    statics_opened: Arc<AtomicUsize>,
    clock: Arc<Mutex<f64>>,
    opening: Arc<Mutex<Option<Hook>>>,
}

type Hook = Arc<dyn Fn() + Send + Sync>;

impl Rig {
    pub(crate) fn fetch_states(&self) -> Vec<String> {
        self.room.1.lock().unwrap().clone()
    }

    fn at_request_one(&self, act: impl Fn() + Send + Sync + 'static) {
        self.spawn.on_request(Arc::new(move |n| {
            if n == 1 {
                act();
            }
        }));
    }

    fn granite_vectors(&self, dir: &TempDir) -> usize {
        let db = VectorDb::open(&embed_paths::vectors_db(dir.root())).unwrap();
        db.coverage(&ModelId(GRANITE.to_string()))
            .iter()
            .map(|c| c.embedded)
            .sum()
    }
}

fn settings(on: bool) -> EmbedSettings {
    EmbedSettings {
        on,
        paused: false,
        models: Vec::new(),
        range: IndexRange::All,
        categories: vec![notchling_engine::search::query::category::Category::Sessions],
    }
}

fn rig(docs: usize, fixture: bool, on: bool) -> (Rig, TempDir) {
    rig_net(docs, fixture, on, ModelNet::offline())
}

pub(crate) fn rig_net(docs: usize, fixture: bool, on: bool, net: ModelNet) -> (Rig, TempDir) {
    let dir = TempDir::new("embed");
    let room = Arc::new(Events::default());
    let spawn = FakeSpawn::new(&ModelId(GRANITE.to_string()));
    let current = Arc::new(Mutex::new(settings(on)));
    let read = current.clone();
    let statics_opened = Arc::new(AtomicUsize::new(0));
    let opened = statics_opened.clone();
    let notes: Vec<(String, String)> = (0..docs)
        .map(|i| (format!("s{i}"), format!("note number {i} about indexing")))
        .collect();
    let clock = Arc::new(Mutex::new(1000.0));
    let ticking = clock.clone();
    let opening: Arc<Mutex<Option<Hook>>> = Arc::new(Mutex::new(None));
    let hooked = opening.clone();
    let ops = EmbedOps::with_parts(EmbedParts {
        data_dir: dir.root().to_path_buf(),
        room: room.clone(),
        settings: Arc::new(move || read.lock().unwrap().clone()),
        corpora: Arc::new(move |_| {
            vec![Arc::new(Notes {
                docs: notes.clone(),
            }) as Arc<dyn Corpus>]
        }),
        spawn: Arc::new(spawn.clone()),
        granite_id: ModelId(GRANITE.to_string()),
        granite_present: !fixture,
        statics: Arc::new(move || {
            opened.fetch_add(1, Ordering::SeqCst);
            let hook = hooked.lock().unwrap().clone();
            if let Some(hook) = hook {
                hook();
            }
            let embedder = HashEmbedder::new("static-fake@1/p1");
            Ok(Some(Arc::new(embedder) as Arc<dyn Embedder>))
        }),
        fixture,
        power: Arc::new(PluggedIn),
        net,
        now: Arc::new(move || *ticking.lock().unwrap()),
    });
    (
        Rig {
            ops,
            room,
            spawn,
            on: current,
            statics_opened,
            clock,
            opening,
        },
        dir,
    )
}

fn app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::Cli,
        peer: None,
        pid: None,
    }
}

#[test]
fn off_maps_nothing_and_spawns_nothing() {
    let (rig, _dir) = rig(20, false, false);
    rig.ops.tick();
    assert_eq!(rig.statics_opened.load(Ordering::SeqCst), 0);
    assert_eq!(rig.spawn.spawns(), 0);
    assert!(rig.ops.loaded_weak().upgrade().is_none());
    let (state, lists) = rig
        .ops
        .query_lists("anything", &Filter::default(), &["sessions"]);
    assert_eq!(state, Semantic::Off);
    assert!(lists.is_empty());
}

#[test]
fn turning_off_frees_everything() {
    let (rig, _dir) = rig(20, false, true);
    rig.ops.tick();
    let weak = rig.ops.loaded_weak();
    assert!(weak.upgrade().is_some());
    rig.on.lock().unwrap().on = false;
    rig.ops.tick();
    assert!(weak.upgrade().is_none());
    let (state, _) = rig
        .ops
        .query_lists("note about indexing", &Filter::default(), &["sessions"]);
    assert_eq!(state, Semantic::Off);
}

#[test]
fn a_fixture_root_never_spawns() {
    let (rig, _dir) = rig(20, true, true);
    for _ in 0..3 {
        rig.ops.tick();
    }
    assert_eq!(rig.spawn.spawns(), 0);
    let (state, lists) =
        rig.ops
            .query_lists("note about indexing", &Filter::default(), &["sessions"]);
    assert_ne!(state, Semantic::Off);
    let list = lists.get("sessions").expect("a list for sessions");
    assert!(list.granite.as_ref().is_some_and(|g| !g.is_empty()));
    assert!(list.statics.as_ref().is_some_and(|g| !g.is_empty()));
}

#[test]
fn the_queue_wakes_the_helper_at_sixteen() {
    let (rig, _dir) = rig(15, false, true);
    rig.ops.tick();
    assert_eq!(rig.spawn.spawns(), 0);
    let (rig16, _d16) = rig_with(16);
    rig16.ops.tick();
    assert!(rig16.spawn.spawns() >= 1);
    let status = rig16
        .ops
        .call(&app(), ops::EMBED_STATUS, &json!({}))
        .unwrap();
    let granite = status["corpora"][0]["embedded"][GRANITE].as_u64().unwrap();
    assert_eq!(granite, 16);
}

fn rig_with(docs: usize) -> (Rig, TempDir) {
    rig(docs, false, true)
}

#[test]
fn status_reports_coverage() {
    let (rig, _dir) = rig(4, true, true);
    rig.ops.tick();
    let status = rig.ops.call(&app(), ops::EMBED_STATUS, &json!({})).unwrap();
    assert_eq!(status["corpora"][0]["corpus"], "sessions");
    assert_eq!(status["corpora"][0]["docs"], 4);
    assert_eq!(status["corpora"][0]["chunks"], 4);
    assert_eq!(status["paused"], false);
    assert!(status["models"].is_array());
    assert!(status.get("state").is_some() && status.get("budget").is_some());
}

#[test]
fn embed_settings_is_app_tier() {
    let (rig, _dir) = rig(1, true, true);
    assert_eq!(rig.ops.spec(ops::EMBED_SETTINGS).unwrap().tier, Tier::App);
    assert_eq!(rig.ops.spec(ops::EMBED_STATUS).unwrap().tier, Tier::Agent);
    assert_eq!(rig.ops.spec(ops::EMBED_REMOVE).unwrap().tier, Tier::App);
    let stored = rig
        .ops
        .call(
            &app(),
            ops::EMBED_SETTINGS,
            &json!({"docs": [{"id": "x", "text": "words"}]}),
        )
        .unwrap();
    assert_eq!(stored["stored"], 1);
    let many: Vec<Value> = (0..201)
        .map(|i| json!({"id": i.to_string(), "text": "t"}))
        .collect();
    assert!(rig
        .ops
        .call(&app(), ops::EMBED_SETTINGS, &json!({"docs": many}))
        .is_err());
    let long = "x".repeat(4001);
    assert!(rig
        .ops
        .call(
            &app(),
            ops::EMBED_SETTINGS,
            &json!({"docs": [{"id": "x", "text": long}]})
        )
        .is_err());
}

#[test]
fn changed_is_at_most_once_a_second() {
    let (rig, _dir) = rig(1, true, true);
    for _ in 0..5 {
        rig.ops.publish_changed();
    }
    let count = |rig: &Rig| {
        rig.room
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, e)| t == "embed" && e == "embed.changed")
            .count()
    };
    assert_eq!(count(&rig), 1);
    *rig.clock.lock().unwrap() += 1.1;
    rig.ops.publish_changed();
    assert_eq!(count(&rig), 2);
}

#[test]
fn a_dead_helper_leaves_text_search_and_requeues() {
    let (rig, _dir) = rig(16, false, true);
    rig.spawn.die_after(0);
    rig.ops.tick();
    let (state, lists) =
        rig.ops
            .query_lists("note about indexing", &Filter::default(), &["sessions"]);
    assert_ne!(state, Semantic::Off);
    let list = lists.get("sessions").unwrap();
    assert!(list.granite.as_ref().is_none_or(Vec::is_empty));
    let status = rig.ops.call(&app(), ops::EMBED_STATUS, &json!({})).unwrap();
    assert_eq!(
        status["corpora"][0]["embedded"][GRANITE]
            .as_u64()
            .unwrap_or(0),
        0
    );
}

#[test]
fn embed_remove_drops_a_model() {
    let (rig, _dir) = rig(4, true, true);
    rig.ops.tick();
    let before = rig.ops.call(&app(), ops::EMBED_STATUS, &json!({})).unwrap();
    assert!(!before["corpora"][0]["embedded"]
        .as_object()
        .unwrap()
        .is_empty());
    let gone = rig
        .ops
        .call(
            &app(),
            ops::EMBED_REMOVE,
            &json!({"model": "static-fake@1/p1"}),
        )
        .unwrap();
    assert_eq!(gone["removed"], true);
    assert!(rig.ops.call(&app(), ops::EMBED_REMOVE, &json!({})).is_err());
}

fn count_changed(rig: &Rig) -> usize {
    rig.room
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|(t, e)| t == "embed" && e == "embed.changed")
        .count()
}

#[test]
fn a_state_change_inside_the_second_is_still_published() {
    let (rig, _dir) = rig(1, true, true);
    rig.ops.tick();
    rig.ops.publish_changed();
    let base = count_changed(&rig);
    rig.ops.publish_changed();
    assert_eq!(count_changed(&rig), base);
    rig.on.lock().unwrap().paused = true;
    rig.ops.publish_changed();
    assert_eq!(count_changed(&rig), base + 1);
    rig.ops.publish_changed();
    assert_eq!(count_changed(&rig), base + 1);
    rig.on.lock().unwrap().on = false;
    rig.ops.publish_changed();
    assert_eq!(count_changed(&rig), base + 2);
}

#[test]
fn turning_off_stops_a_feed_that_is_running() {
    let (rig, _dir) = rig(40, false, true);
    let on = rig.on.clone();
    rig.at_request_one(move || on.lock().unwrap().on = false);
    rig.ops.tick();
    assert_eq!(rig.spawn.requests(), 1);
}

#[test]
fn pausing_stops_a_feed_that_is_running() {
    let (rig, _dir) = rig(40, false, true);
    let on = rig.on.clone();
    rig.at_request_one(move || on.lock().unwrap().paused = true);
    rig.ops.tick();
    assert_eq!(rig.spawn.requests(), 1);
}

#[test]
fn a_removed_model_stops_the_feed_and_gets_no_more_vectors() {
    let (rig, dir) = rig(40, false, true);
    let embed = Arc::downgrade(&rig.ops);
    rig.at_request_one(move || {
        if let Some(embed) = embed.upgrade() {
            let gone = json!({"model": embed_paths::GRANITE_ID});
            let _ = embed.call(&app(), ops::EMBED_REMOVE, &gone);
        }
    });
    rig.ops.tick();
    assert_eq!(rig.spawn.requests(), 1);
    assert_eq!(rig.granite_vectors(&dir), 0);
}

#[test]
fn a_model_that_arrives_is_loaded_without_a_gap() {
    let (rig, _dir) = rig(4, true, true);
    rig.ops.tick();
    let before = rig.ops.loaded_weak();
    rig.ops.model_arrived();
    assert!(before.upgrade().is_some());
    assert_ne!(rig.ops.state(), Semantic::NoModel);
    rig.ops.tick();
    assert!(before.upgrade().is_none());
    assert!(rig.ops.loaded_weak().upgrade().is_some());
}

#[test]
fn a_model_that_arrives_stops_the_feed_and_the_next_pass_goes_on() {
    let (rig, dir) = rig(40, false, true);
    let embed = Arc::downgrade(&rig.ops);
    rig.at_request_one(move || {
        if let Some(embed) = embed.upgrade() {
            embed.model_arrived();
        }
    });
    rig.ops.tick();
    assert_eq!(rig.spawn.requests(), 1);
    assert_eq!(rig.granite_vectors(&dir), 8);
    rig.ops.tick();
    assert_eq!(rig.spawn.requests(), 5);
    assert_eq!(rig.granite_vectors(&dir), 40);
}

#[test]
fn nothing_loads_after_close() {
    let (rig, _dir) = rig(4, true, true);
    rig.ops.tick();
    let weak = rig.ops.loaded_weak();
    assert!(weak.upgrade().is_some());
    rig.ops.close();
    assert!(weak.upgrade().is_none());
    rig.ops.tick();
    assert!(rig.ops.loaded_weak().upgrade().is_none());
}

#[test]
fn a_close_while_the_set_is_opening_leaves_nothing_loaded() {
    let (rig, _dir) = rig(4, false, true);
    let embed = Arc::downgrade(&rig.ops);
    *rig.opening.lock().unwrap() = Some(Arc::new(move || {
        if let Some(embed) = embed.upgrade() {
            embed.close();
        }
    }));
    rig.ops.tick();
    assert_eq!(rig.statics_opened.load(Ordering::SeqCst), 1);
    assert!(rig.ops.loaded_weak().upgrade().is_none());
}

#[test]
fn a_wake_ends_the_rest_between_batches() {
    let (rig, _dir) = rig(1, true, true);
    let since = rig.ops.stirs();
    rig.ops.refresh();
    assert!(rig.ops.rest(since, Duration::from_secs(30)));
}

#[path = "embed_ops_race_tests.rs"]
mod race_tests;
