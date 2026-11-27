use crate::cluster_fence::{fence_path, mark_moved, read_fence};
use crate::cluster_move_tests::{one, pair, run, Pair, Spec, S2};
use crate::snap_restore_testkit::S1;
use notchling_engine::cluster::fence::Gate;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::move_home::SwitchResult;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn two() -> Vec<Spec> {
    vec![
        Spec {
            id: S1,
            flags: &["--name", "one"],
            movable: true,
        },
        Spec {
            id: S2,
            flags: &["--name", "two"],
            movable: true,
        },
    ]
}

fn notice(p: &Pair, id: &str) -> Option<String> {
    p.from
        .inbox
        .items()
        .into_iter()
        .find(|item| item.id == id)
        .map(|item| format!("{} {}", item.title, item.detail))
}

fn mark_both(p: &Pair) {
    let at = Term {
        n: 2,
        node: p.b.clone(),
    };
    let keys = vec![S1.to_string(), S2.to_string()];
    mark_moved(&p.from.cluster, &p.a, at, &keys, "m1").unwrap();
}

type Seen = Arc<Mutex<Vec<(String, Vec<String>)>>>;

fn watch_starts(p: &Pair) -> Seen {
    let seen: Seen = Arc::default();
    let (cluster, log) = (p.from.cluster.clone(), seen.clone());
    *p.from.acts.on_start.lock().unwrap() = Some(Box::new(move |id: &str| {
        let gates = read_fence(&cluster)
            .map(|f| f.sessions.keys().cloned().collect())
            .unwrap_or_default();
        log.lock().unwrap().push((id.to_string(), gates));
    }));
    seen
}

fn each_lifted_before_it_starts(seen: &Seen) {
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert_eq!(seen[0].0, S1);
    assert!(!seen[0].1.contains(&S1.to_string()), "{seen:?}");
    assert!(seen[0].1.contains(&S2.to_string()), "{seen:?}");
    assert_eq!(seen[1].0, S2);
    assert!(!seen[1].1.contains(&S2.to_string()), "{seen:?}");
}

#[test]
fn a_fence_that_cannot_be_written_is_reported_by_the_move() {
    let p = pair(&one(), &|_| {});
    std::fs::create_dir_all(fence_path(&p.from.cluster)).unwrap();
    let runner = run(&p);
    let status = runner.status();
    assert_eq!(status["step"], "done");
    let why = status["unfenced"]
        .as_str()
        .expect("the move says the sessions were not paused");
    assert!(why.contains("fence.json"), "{why}");
    let text = notice(&p, "cluster-move-fence").expect("a notice about the fence");
    assert!(text.contains(S1), "{text}");
    assert!(text.contains("both machines"), "{text}");
    let announced = p
        .from
        .inbox
        .items()
        .into_iter()
        .find(|item| item.title.ends_with("is primary now"))
        .expect("the announcement");
    assert!(
        announced.detail.contains("both machines"),
        "{}",
        announced.detail
    );
}

#[test]
fn a_fence_write_that_works_on_a_later_try_is_a_clean_move() {
    let p = pair(&one(), &|_| {});
    let path = fence_path(&p.from.cluster);
    std::fs::create_dir_all(&path).unwrap();
    let blocker = path.clone();
    *p.to.link.on_take.lock().unwrap() = Some(Box::new(move |_: &str| {
        let path = blocker.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            let _ = std::fs::remove_dir(&path);
        });
    }));
    let runner = run(&p);
    let status = runner.status();
    assert_eq!(status["step"], "done");
    assert_eq!(status["unfenced"], Value::Null, "{status}");
    assert!(notice(&p, "cluster-move-fence").is_none());
    let held = read_fence(&p.from.cluster).expect("the gate was written");
    assert_eq!(held.sessions[S1], Gate::Moved { to: "m1".into() });
}

#[test]
fn a_move_with_no_own_node_id_is_not_reported_clean() {
    let p = pair(&one(), &|_| {});
    p.from.link.edit(|status| status["me"] = Value::Null);
    let result = p
        .from
        .mover
        .switch(&p.b, &[S1.to_string()], Some(p.manifest.set.as_str()), 2);
    assert!(
        !matches!(result, SwitchResult::Done),
        "a switch that fenced nothing was called Done"
    );
    let text = notice(&p, "cluster-move-fence").expect("a notice about the fence");
    assert!(text.contains(S1), "{text}");
    assert!(read_fence(&p.from.cluster).is_none());
}

#[test]
fn a_gate_that_cannot_be_lifted_is_reported() {
    let p = pair(&one(), &|_| {});
    std::fs::write(fence_path(&p.from.cluster), b"garbage").unwrap();
    let rows = p.from.mover.start_here(&[S1.to_string()]);
    assert!(rows[0].1.is_ok());
    let text = notice(&p, &format!("cluster-move-lift-{S1}")).expect("a notice about the gate");
    assert!(text.contains(S1), "{text}");
}

#[test]
fn restarting_lifts_each_gate_before_its_session_starts() {
    let p = pair(&two(), &|_| {});
    mark_both(&p);
    let seen = watch_starts(&p);
    let rows = p.from.mover.restart(&[S1.to_string(), S2.to_string()], &[]);
    assert!(rows.iter().all(|(_, result)| result.is_ok()));
    each_lifted_before_it_starts(&seen);
    assert!(read_fence(&p.from.cluster)
        .map(|f| f.sessions.is_empty())
        .unwrap_or(true));
}

#[test]
fn starting_here_lifts_each_gate_before_its_session_starts() {
    let p = pair(&two(), &|_| {});
    mark_both(&p);
    let seen = watch_starts(&p);
    let rows = p.from.mover.start_here(&[S1.to_string(), S2.to_string()]);
    assert!(rows.iter().all(|(_, result)| result.is_ok()));
    each_lifted_before_it_starts(&seen);
}
