use super::{Turns, Workspace};
use crate::cluster_blobs::Blobs;
use crate::cluster_workspace_testkit::{node_id, noise, Fixture, Gated};
use notchling_engine::bridge::net::crypto::{base64_decode, sha256_hex};
use notchling_engine::cluster::jobs::job_id;
use serde_json::json;
use std::path::Path;

#[test]
fn turns_serialize_one_key_and_let_others_run() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let turns = std::sync::Arc::new(Turns::default());
    let held = turns.claim("repo-a");
    let flag = std::sync::Arc::new(AtomicBool::new(false));
    let waiter = {
        let (turns, flag) = (turns.clone(), flag.clone());
        std::thread::spawn(move || {
            let _turn = turns.claim("repo-a");
            flag.store(true, Ordering::SeqCst);
        })
    };
    drop(turns.claim("repo-b"));
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert!(!flag.load(Ordering::SeqCst), "the same key waits");
    drop(held);
    waiter.join().unwrap();
    assert!(flag.load(Ordering::SeqCst));
}

#[test]
fn prepares_for_different_repositories_overlap() {
    let fx = Fixture::new("ws-overlap");
    let (_origin_b, repo_b) = fx.second_repo("other");
    let facts_a = fx.sender.read_facts(fx.repo_str()).unwrap();
    let facts_b = fx.sender.read_facts(repo_b.to_str().unwrap()).unwrap();
    let (gated, gate) = Gated::new(fx.home.clone(), "origin.git");
    let cluster = fx.dir.path().join("gated/cluster");
    let worker = Workspace::new(
        cluster.clone(),
        std::sync::Arc::new(gated),
        Blobs::new(cluster),
    );
    let slow = {
        let worker = worker.clone();
        std::thread::spawn(move || worker.prepare(&json!({"job": "j-a", "facts": facts_a})))
    };
    assert!(gate.wait_entered(), "the first clone never started");
    let quick = worker.prepare(&json!({"job": "j-b", "facts": facts_b}));
    let first_still_waiting = !slow.is_finished();
    gate.open();
    let slow = slow.join().unwrap();
    quick.unwrap();
    slow.unwrap();
    assert!(
        first_still_waiting,
        "a slow clone held up an unrelated repository"
    );
}

fn noisy_worktree(fx: &Fixture, key: &str) -> String {
    let cwd = fx
        .sender
        .send_code(&node_id('a'), "w", key, fx.repo_str())
        .unwrap();
    let tree = Path::new(&cwd);
    std::fs::write(tree.join("noise.bin"), noise(700_000)).unwrap();
    fx.git(tree, &["add", "."]);
    fx.git(tree, &["commit", "-q", "-m", "noise"]);
    job_id(key)
}

#[test]
fn two_returns_of_one_job_cannot_corrupt_each_other() {
    let fx = Fixture::new("ws-race");
    let job = noisy_worktree(&fx, "kc");
    let first = |from: u64| fx.worker.give_back(&json!({"job": job, "from": from}));
    let mut runs = Vec::new();
    for _ in 0..2 {
        let reply = first(0).unwrap();
        assert_eq!(reply["done"], false);
        let data = base64_decode(reply["data"].as_str().unwrap()).unwrap();
        runs.push((
            reply["nonce"].clone(),
            reply["next"].as_u64().unwrap(),
            data,
            None,
        ));
    }
    assert_ne!(runs[0].0, runs[1].0);
    for _ in 0..8 {
        for (nonce, next, data, sha) in &mut runs {
            if sha.is_some() {
                continue;
            }
            let args = json!({"job": job, "from": *next, "nonce": nonce});
            let step = fx.worker.give_back(&args).unwrap();
            data.extend(base64_decode(step["data"].as_str().unwrap()).unwrap());
            *next = step["next"].as_u64().unwrap();
            if step["done"] == true {
                *sha = Some(step["sha256"].as_str().unwrap().to_string());
            }
        }
    }
    for (n, (_, _, data, sha)) in runs.iter().enumerate() {
        assert_eq!(
            sha.as_deref(),
            Some(sha256_hex(data).as_str()),
            "return {n}"
        );
    }
    let leftover = std::fs::read_dir(fx.worker_cluster.join("outgoing")).unwrap();
    assert_eq!(leftover.count(), 0);
}

#[test]
fn a_return_chunk_needs_its_own_nonce() {
    let fx = Fixture::new("ws-nonce");
    let job = noisy_worktree(&fx, "kq");
    let first = fx
        .worker
        .give_back(&json!({"job": job, "from": 0}))
        .unwrap();
    let next = first["next"].as_u64().unwrap();
    for nonce in [json!(null), json!("nope"), json!("../x"), json!("-a")] {
        let args = json!({"job": job, "from": next, "nonce": nonce});
        assert!(fx.worker.give_back(&args).is_err(), "{nonce}");
    }
    let args = json!({"job": job, "from": next, "nonce": first["nonce"]});
    fx.worker.give_back(&args).unwrap();
}
