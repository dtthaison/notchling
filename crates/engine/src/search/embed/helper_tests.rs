use super::helper::{HelperEmbedder, HelperState, CACHE};
use super::helper_fake::{FakeSpawn, FAKE_DIMS};
use super::{EmbedError, Embedder, ModelId};
use std::sync::Arc;
use std::time::Duration;

fn model() -> ModelId {
    ModelId("granite-fake@1/p1".to_string())
}

fn pair() -> (Arc<HelperEmbedder>, FakeSpawn) {
    let spawn = FakeSpawn::new(&model());
    (
        Arc::new(HelperEmbedder::new(Arc::new(spawn.clone()), model())),
        spawn,
    )
}

fn wait_for(embedder: &HelperEmbedder, state: HelperState) {
    for _ in 0..200 {
        if embedder.state() == state {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("state is {:?}, not {state:?}", embedder.state());
}

fn warmed() -> (Arc<HelperEmbedder>, FakeSpawn) {
    let (helper, spawn) = pair();
    helper.warm();
    wait_for(&helper, HelperState::Ready);
    (helper, spawn)
}

#[test]
fn cold_query_starts_the_helper_and_answers_not_ready() {
    let (helper, spawn) = pair();
    assert_eq!(helper.state(), HelperState::Cold);
    assert_eq!(helper.embed_query("hello"), Err(EmbedError::NotReady));
    assert_eq!(helper.embed_query("hello again"), Err(EmbedError::NotReady));
    wait_for(&helper, HelperState::Ready);
    assert_eq!(spawn.spawns(), 1);
    assert_eq!(helper.dims(), FAKE_DIMS);
    assert_eq!(
        helper.embed_query("hello"),
        Ok(FakeSpawn::vector_of("hello"))
    );
}

#[test]
fn a_dead_helper_requeues_its_batch() {
    let (helper, spawn) = pair();
    spawn.die_after(1);
    helper.warm();
    wait_for(&helper, HelperState::Ready);
    assert!(helper.embed_batch(&["one", "two"]).is_ok());
    let failed = helper.embed_batch(&["three", "four"]);
    assert!(matches!(failed, Err(EmbedError::Failed(_))), "{failed:?}");
    assert_eq!(helper.state(), HelperState::Cold);
    spawn.die_after(usize::MAX);
    assert_eq!(helper.embed_query("next"), Err(EmbedError::NotReady));
    wait_for(&helper, HelperState::Ready);
    assert_eq!(spawn.spawns(), 2);
    assert_eq!(
        helper.embed_batch(&["three", "four"]),
        Ok(vec![
            FakeSpawn::vector_of("three"),
            FakeSpawn::vector_of("four")
        ])
    );
}

#[test]
fn the_cache_holds_256() {
    let (helper, spawn) = warmed();
    for i in 0..=CACHE {
        helper.embed_query(&format!("query {i}")).unwrap();
    }
    let before = spawn.requests();
    helper.embed_query(&format!("query {CACHE}")).unwrap();
    assert_eq!(spawn.requests(), before);
    helper.embed_query("query 0").unwrap();
    assert_eq!(spawn.requests(), before + 1);
}

#[test]
fn at_most_two_queries_at_once() {
    let (helper, spawn) = warmed();
    spawn.delay(Duration::from_millis(80));
    let threads: Vec<_> = (0..6)
        .map(|i| {
            let helper = helper.clone();
            std::thread::spawn(move || helper.embed_query(&format!("parallel {i}")))
        })
        .collect();
    for t in threads {
        assert!(t.join().unwrap().is_ok());
    }
    assert_eq!(spawn.peak_in_flight(), 2);
}

#[test]
fn a_refused_spawn_is_failed_and_says_why() {
    let (helper, spawn) = pair();
    spawn.refuse(true);
    helper.warm();
    wait_for(&helper, HelperState::Failed("no helper here".to_string()));
    assert_eq!(
        helper.embed_query("x"),
        Err(EmbedError::Failed("no helper here".to_string()))
    );
}

#[test]
fn a_helper_with_another_model_is_failed() {
    let spawn = FakeSpawn::new(&ModelId("other@1/p1".to_string()));
    let helper = HelperEmbedder::new(Arc::new(spawn), model());
    helper.warm();
    wait_for(
        &helper,
        HelperState::Failed("the helper runs another model".to_string()),
    );
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    for _ in 0..400 {
        if done() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{what}");
}

#[test]
fn a_helper_that_exits_while_idle_goes_cold_and_starts_again() {
    let (helper, spawn) = warmed();
    spawn.exit_all();
    wait_for(&helper, HelperState::Cold);
    helper.warm();
    wait_for(&helper, HelperState::Ready);
    assert_eq!(spawn.spawns(), 2);
    assert_eq!(
        helper.embed_batch(&["after"]),
        Ok(vec![FakeSpawn::vector_of("after")])
    );
}

#[test]
fn stopping_while_warming_leaves_no_helper_running() {
    let (helper, spawn) = pair();
    let release = spawn.hold_spawn();
    helper.warm();
    assert_eq!(helper.state(), HelperState::Warming);
    helper.stop();
    assert_eq!(helper.state(), HelperState::Cold);
    release.send(()).unwrap();
    wait_until("the late helper was kept", || {
        spawn.spawns() == 1 && spawn.live() == 0
    });
    assert_eq!(helper.state(), HelperState::Cold);
    helper.warm();
    wait_for(&helper, HelperState::Ready);
    assert_eq!(spawn.spawns(), 2);
    assert_eq!(spawn.live(), 1);
}
