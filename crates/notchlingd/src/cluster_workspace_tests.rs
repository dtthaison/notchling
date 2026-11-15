use super::command_for;
use crate::cluster_workspace_testkit::{node_id, Fixture};
use notchling_engine::cluster::jobs::job_id;
use notchling_engine::cluster::workspace::{GitCall, NO_NEW_COMMITS};
use serde_json::json;
use std::path::Path;

fn tree_of(cwd: &str) -> &Path {
    Path::new(cwd)
}

#[test]
fn worker_gets_unpushed_commit() {
    let fx = Fixture::new("ws-unpushed");
    let head = fx.commit("a.txt", "two\n", "two");
    let node = node_id('a');
    let cwd = fx
        .sender
        .send_code(&node, "worker", "k1", fx.repo_str())
        .unwrap();
    let tree = tree_of(&cwd);
    assert!(
        tree.starts_with(fx.worker_cluster.join("work/jobs")),
        "{cwd}"
    );
    assert_eq!(
        std::fs::read_to_string(tree.join("a.txt")).unwrap(),
        "two\n"
    );
    assert_eq!(fx.git(tree, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        fx.git(tree, &["rev-parse", "--abbrev-ref", "HEAD"]),
        format!("notchling/{}", job_id("k1"))
    );
    assert_eq!(
        fx.ops(),
        vec!["node.put", "node.put.end", "workspace.prepare"]
    );
    assert!(fx.git(&fx.origin, &["branch", "--list"]).contains("main"));
    assert_eq!(fx.git(&fx.origin, &["rev-list", "--count", "main"]), "1");
    let incoming = fx.worker_cluster.join("incoming");
    assert_eq!(std::fs::read_dir(incoming).unwrap().count(), 0);
}

#[test]
fn a_pushed_branch_sends_no_bundle_and_a_subfolder_is_kept() {
    let fx = Fixture::new("ws-pushed");
    std::fs::create_dir_all(fx.repo.join("sub/dir")).unwrap();
    std::fs::write(fx.repo.join("sub/dir/f.txt"), "x").unwrap();
    fx.git(&fx.repo, &["add", "sub"]);
    fx.git(&fx.repo, &["commit", "-q", "-m", "sub"]);
    fx.git(&fx.repo, &["push", "-q"]);
    let cwd = fx
        .sender
        .send_code(
            &node_id('a'),
            "worker",
            "k2",
            fx.repo.join("sub/dir").to_str().unwrap(),
        )
        .unwrap();
    assert!(cwd.ends_with("sub/dir"), "{cwd}");
    assert!(Path::new(&cwd).join("f.txt").is_file());
    assert_eq!(fx.ops(), vec!["workspace.prepare"]);
}

#[test]
fn a_big_bundle_goes_in_several_chunks() {
    let fx = Fixture::new("ws-big");
    let mut seed = 12345u64;
    let noise: Vec<u8> = (0..700_000)
        .map(|_| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as u8
        })
        .collect();
    std::fs::write(fx.repo.join("noise.bin"), &noise).unwrap();
    fx.git(&fx.repo, &["add", "noise.bin"]);
    fx.git(&fx.repo, &["commit", "-q", "-m", "noise"]);
    let cwd = fx
        .sender
        .send_code(&node_id('a'), "worker", "k3", fx.repo_str())
        .unwrap();
    assert_eq!(
        std::fs::read(Path::new(&cwd).join("noise.bin")).unwrap(),
        noise
    );
    let puts = fx.ops().iter().filter(|o| *o == "node.put").count();
    assert!(puts >= 3, "{:?}", fx.ops());
}

#[test]
fn the_same_key_again_reuses_the_worktree() {
    let fx = Fixture::new("ws-again");
    fx.commit("a.txt", "two\n", "two");
    let first = fx
        .sender
        .send_code(&node_id('a'), "worker", "k4", fx.repo_str())
        .unwrap();
    std::fs::write(Path::new(&first).join("scratch.txt"), "mine").unwrap();
    let second = fx
        .sender
        .send_code(&node_id('a'), "worker", "k4", fx.repo_str())
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(
        std::fs::read_to_string(Path::new(&second).join("scratch.txt")).unwrap(),
        "mine"
    );
}

#[test]
fn a_failed_prepare_leaves_no_worktree_but_keeps_the_clone() {
    let fx = Fixture::new("ws-fail");
    let mut facts = fx.sender.read_facts(fx.repo_str()).unwrap();
    facts.head = "f".repeat(40);
    let err = fx
        .worker
        .prepare(&json!({"job": "j-fail", "facts": facts}))
        .unwrap_err();
    assert!(
        err.message.contains("isn't on the remote"),
        "{}",
        err.message
    );
    let work = fx.worker_cluster.join("work");
    assert!(!work.join("jobs/j-fail").exists());
    let entries: Vec<String> = std::fs::read_dir(&work)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        entries.iter().any(|e| e.starts_with("repo-")),
        "{entries:?}"
    );
    let repo = work.join(entries.iter().find(|e| e.starts_with("repo-")).unwrap());
    let refs = fx.git(
        &repo,
        &["for-each-ref", "refs/notchling", "refs/heads/notchling"],
    );
    assert_eq!(refs, "");
}

#[test]
fn prepare_refuses_what_the_planner_refuses() {
    let fx = Fixture::new("ws-refuse");
    let facts = fx.sender.read_facts(fx.repo_str()).unwrap();
    for job in ["../x", "", "-j"] {
        assert!(fx
            .worker
            .prepare(&json!({"job": job, "facts": facts}))
            .is_err());
    }
    let mut hostile = facts.clone();
    hostile.branch = "--upload-pack=touch x".to_string();
    assert!(fx
        .worker
        .prepare(&json!({"job": "j-1", "facts": hostile}))
        .is_err());
    let mut hostile = facts.clone();
    hostile.origin = Some("ext::sh -c touch% /tmp/nlc-pwned".to_string());
    assert!(fx
        .worker
        .prepare(&json!({"job": "j-1", "facts": hostile}))
        .is_err());
    assert!(!Path::new("/tmp/nlc-pwned").exists());
    assert!(fx
        .worker
        .prepare(&json!({"job": "j-1", "facts": facts, "transfer": "../nope"}))
        .is_err());
    assert!(fx
        .worker
        .prepare(&json!({"job": "j-1", "facts": facts, "transfer": "never-arrived"}))
        .is_err());
}

#[test]
fn hooks_never_run() {
    let fx = Fixture::new("ws-hooks");
    fx.sender
        .send_code(&node_id('a'), "worker", "first", fx.repo_str())
        .unwrap();
    let work = fx.worker_cluster.join("work");
    let clone = std::fs::read_dir(&work)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("repo-")
        })
        .unwrap();
    let marker = fx.dir.path().join("hook-ran");
    let hook = clone.join(".git/hooks/post-checkout");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    fx.commit("a.txt", "two\n", "two");
    fx.sender
        .send_code(&node_id('a'), "worker", "second", fx.repo_str())
        .unwrap();
    assert!(!marker.exists(), "a hook from the repository ran");
    fx.git(
        &clone,
        &[
            "worktree",
            "add",
            "-q",
            "-B",
            "control",
            fx.dir.path().join("control").to_str().unwrap(),
            "HEAD",
        ],
    );
    assert!(
        marker.exists(),
        "the trap hook would have fired without the hardening"
    );
}

#[test]
fn return_creates_branch_and_never_merges() {
    let fx = Fixture::new("ws-return");
    fx.commit("a.txt", "two\n", "two");
    let before = fx.git(&fx.repo, &["rev-parse", "HEAD"]);
    let node = node_id('a');
    let cwd = fx
        .sender
        .send_code(&node, "Worker One", "kr", fx.repo_str())
        .unwrap();
    let tree = Path::new(&cwd);
    std::fs::write(tree.join("b.txt"), "from the worker\n").unwrap();
    fx.git(tree, &["add", "b.txt"]);
    fx.git(tree, &["commit", "-q", "-m", "worker work"]);
    let theirs = fx.git(tree, &["rev-parse", "HEAD"]);
    let job = job_id("kr");
    let reply = fx.sender.bring_back(&job).unwrap();
    let branch = format!("notchling/worker-one/{job}");
    assert_eq!(reply["branch"], branch);
    assert_eq!(
        fx.git(&fx.repo, &["rev-parse", &format!("refs/heads/{branch}")]),
        theirs
    );
    assert_eq!(fx.git(&fx.repo, &["rev-parse", "HEAD"]), before);
    assert_eq!(
        fx.git(&fx.repo, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "main"
    );
    assert_eq!(fx.git(&fx.repo, &["status", "--porcelain"]), "");
    assert!(!fx.repo.join("b.txt").exists());
    assert_eq!(
        fx.git(&fx.repo, &["rev-list", "--merges", "--count", "HEAD"]),
        "0"
    );
    assert_eq!(fx.git(&fx.origin, &["rev-list", "--count", "main"]), "1");
    let outgoing = fx.dir.path().join("sender/cluster/outgoing");
    assert_eq!(std::fs::read_dir(outgoing).unwrap().count(), 0);
    assert_eq!(
        std::fs::read_dir(fx.worker_cluster.join("outgoing"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn a_second_return_fast_forwards() {
    let fx = Fixture::new("ws-return2");
    let cwd = fx
        .sender
        .send_code(&node_id('a'), "w", "k2r", fx.repo_str())
        .unwrap();
    let tree = Path::new(&cwd);
    let job = job_id("k2r");
    std::fs::write(tree.join("one.txt"), "1").unwrap();
    fx.git(tree, &["add", "."]);
    fx.git(tree, &["commit", "-q", "-m", "one"]);
    fx.sender.bring_back(&job).unwrap();
    std::fs::write(tree.join("two.txt"), "2").unwrap();
    fx.git(tree, &["add", "."]);
    fx.git(tree, &["commit", "-q", "-m", "two"]);
    let tip = fx.git(tree, &["rev-parse", "HEAD"]);
    fx.sender.bring_back(&job).unwrap();
    assert_eq!(
        fx.git(
            &fx.repo,
            &["rev-parse", &format!("refs/heads/notchling/w/{job}")]
        ),
        tip
    );
}

#[test]
fn fetch_back_refuses_a_diverged_branch() {
    let fx = Fixture::new("ws-diverged");
    let cwd = fx
        .sender
        .send_code(&node_id('a'), "w", "kd", fx.repo_str())
        .unwrap();
    let tree = Path::new(&cwd);
    let job = job_id("kd");
    std::fs::write(tree.join("w.txt"), "w").unwrap();
    fx.git(tree, &["add", "."]);
    fx.git(tree, &["commit", "-q", "-m", "worker"]);
    fx.git(&fx.repo, &["checkout", "-q", "-b", "mine"]);
    fx.commit("mine.txt", "m", "mine");
    let mine = fx.git(&fx.repo, &["rev-parse", "HEAD"]);
    fx.git(&fx.repo, &["branch", &format!("notchling/w/{job}")]);
    fx.git(&fx.repo, &["checkout", "-q", "main"]);
    let err = fx.sender.bring_back(&job).unwrap_err();
    assert_eq!(
        err.message,
        format!("notchling/w/{job} already has other commits")
    );
    assert_eq!(
        fx.git(
            &fx.repo,
            &["rev-parse", &format!("refs/heads/notchling/w/{job}")]
        ),
        mine
    );
}

#[test]
fn return_with_nothing_new_says_so() {
    let fx = Fixture::new("ws-nothing");
    fx.sender
        .send_code(&node_id('a'), "w", "kn", fx.repo_str())
        .unwrap();
    let err = fx.sender.bring_back(&job_id("kn")).unwrap_err();
    assert_eq!(err.message, NO_NEW_COMMITS);
    assert!(fx.sender.bring_back("j-never-sent").is_err());
    assert!(fx.sender.bring_back("../x").is_err());
}

#[test]
fn a_repository_without_a_remote_is_refused_before_anything_is_sent() {
    let fx = Fixture::new("ws-noremote");
    fx.git(&fx.repo, &["remote", "remove", "origin"]);
    let err = fx
        .sender
        .send_code(&node_id('a'), "m1", "kx", fx.repo_str())
        .unwrap_err();
    assert_eq!(
        err.message,
        "This repository has no remote; send its folder to m1 another way"
    );
    assert!(fx.ops().is_empty());
}

#[test]
fn a_detached_head_or_a_plain_folder_is_refused() {
    let fx = Fixture::new("ws-detached");
    fx.git(&fx.repo, &["checkout", "-q", "--detach"]);
    let err = fx
        .sender
        .send_code(&node_id('a'), "w", "ky", fx.repo_str())
        .unwrap_err();
    assert!(err.message.contains("branch"), "{}", err.message);
    let plain = fx.dir.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let err = fx
        .sender
        .send_code(&node_id('a'), "w", "kz", plain.to_str().unwrap())
        .unwrap_err();
    assert!(
        err.message.contains("isn't in a git repository"),
        "{}",
        err.message
    );
    assert!(fx.ops().is_empty());
}

#[test]
fn git_env_is_scrubbed() {
    let call = GitCall {
        cwd: "/repo".to_string(),
        args: vec!["status".to_string()],
        skip_if_exists: None,
        allow_failure: false,
        refusals: Vec::new(),
        timeout_secs: 5,
    };
    let inherited = [
        "PATH",
        "GIT_DIR",
        "GIT_SSH_COMMAND",
        "GIT_CONFIG_PARAMETERS",
        "HOME",
    ]
    .map(String::from);
    let command = command_for(&call, inherited);
    for name in ["GIT_DIR", "GIT_SSH_COMMAND", "GIT_CONFIG_PARAMETERS"] {
        assert_eq!(command.env.get(name), Some(&None), "{name}");
    }
    assert_eq!(
        command.env.get("GIT_TERMINAL_PROMPT"),
        Some(&Some("0".to_string()))
    );
    assert!(!command.env.contains_key("PATH"));
    assert!(!command.env.contains_key("HOME"));
    assert_eq!(command.argv[0], "git");
    assert!(command
        .argv
        .contains(&"core.hooksPath=/dev/null".to_string()));
}
