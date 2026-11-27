use super::workspace::{
    back_branch, facts_calls, node_slug, origin_ok, parse_facts, plan_cleanup, plan_fetch_back,
    plan_prepare, plan_return, plan_send, repo_dir, scrubbed, valid_branch, FactsOut, GitCall,
    RepoFacts, HARDENING, NO_REMOTE,
};

const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const UP: &str = "89abcdef0123456789abcdef0123456789abcdef";

fn facts() -> RepoFacts {
    RepoFacts {
        root: "/Users/me/code/notchling".to_string(),
        origin: Some("git@github.com:me/notchling.git".to_string()),
        branch: "feature/x".to_string(),
        head: HEAD.to_string(),
        upstream: Some(UP.to_string()),
        unpushed: 2,
    }
}

fn joined(call: &GitCall) -> String {
    call.argv().join(" ")
}

fn all_calls() -> Vec<GitCall> {
    let f = facts();
    let mut calls =
        plan_prepare(&f, "j-0123456789ab", "/data/cluster/work", Some("/data/b")).unwrap();
    calls.extend(plan_cleanup(&f, "j-0123456789ab", "/data/cluster/work").unwrap());
    calls.push(plan_return("j-0123456789ab", "/data/cluster/work", "/data/out").unwrap());
    calls.extend(plan_fetch_back("/repo", "/data/b", "m1", "j-0123456789ab").unwrap());
    if let Some(bundle) = plan_send(&f, "/data/send").unwrap().bundle {
        calls.push(bundle);
    }
    let reads = facts_calls("/repo").unwrap();
    calls.extend([
        reads.root,
        reads.head,
        reads.branch,
        reads.origin,
        reads.upstream,
        reads.unpushed,
    ]);
    calls
}

#[cfg(unix)]
#[test]
fn every_call_is_hardened() {
    for call in all_calls() {
        let line = joined(&call);
        for part in HARDENING {
            assert!(line.starts_with(&HARDENING.join(" ")), "{line}");
            assert!(line.contains(part), "{line}");
        }
        assert!(line.contains("core.hooksPath=/dev/null"), "{line}");
        assert!(line.contains("core.fsmonitor=false"), "{line}");
        let removal = call
            .args
            .starts_with(&["worktree".to_string(), "remove".to_string()]);
        assert!(
            removal || !call.args.iter().any(|a| a == "--force" || a == "-f"),
            "{line}"
        );
        assert!(
            !call
                .args
                .iter()
                .any(|a| a.starts_with('+') && a.contains("refs/heads/notchling/")),
            "{line}"
        );
        let verb = call.args.iter().find(|a| !a.starts_with('-')).unwrap();
        assert!(
            !["push", "merge", "pull", "rebase", "reset", "checkout"].contains(&verb.as_str()),
            "{line}"
        );
    }
}

#[test]
fn scrubbed_names_are_every_git_variable() {
    let names = [
        "PATH",
        "GIT_DIR",
        "GIT_SSH_COMMAND",
        "HOME",
        "GIT_CONFIG_GLOBAL",
    ]
    .map(String::from);
    assert_eq!(
        scrubbed(names),
        vec!["GIT_DIR", "GIT_SSH_COMMAND", "GIT_CONFIG_GLOBAL"]
    );
}

#[cfg(unix)]
#[test]
fn send_plan_bundles_only_unpushed_commits() {
    let f = facts();
    let plan = plan_send(&f, "/data/send.bundle").unwrap();
    let call = plan.bundle.expect("two commits the remote lacks");
    assert_eq!(call.cwd, "/Users/me/code/notchling");
    let line = joined(&call);
    assert!(
        line.ends_with(
            "bundle create /data/send.bundle refs/heads/feature/x --not --remotes=origin"
        ),
        "{line}"
    );
    for upstream in [None, Some(HEAD.to_string()), Some(UP.to_string())] {
        let nothing = RepoFacts {
            upstream,
            unpushed: 0,
            ..facts()
        };
        assert!(
            plan_send(&nothing, "/data/send.bundle")
                .unwrap()
                .bundle
                .is_none(),
            "nothing unpushed means no bundle, whatever the upstream says"
        );
    }
    let ahead = RepoFacts {
        upstream: None,
        unpushed: 1,
        ..facts()
    };
    assert!(plan_send(&ahead, "/data/send.bundle")
        .unwrap()
        .bundle
        .is_some());
}

#[cfg(unix)]
#[test]
fn prepare_clones_once_and_adds_a_worktree() {
    let f = facts();
    let calls = plan_prepare(&f, "j-0123456789ab", "/data/work", Some("/data/in/t1")).unwrap();
    let dir = repo_dir(&f, "/data/work").unwrap();
    assert!(dir.starts_with("/data/work/notchling-"), "{dir}");
    assert_eq!(dir.len(), "/data/work/notchling-".len() + 8);
    let clone = calls
        .iter()
        .find(|c| c.args.contains(&"clone".to_string()))
        .unwrap();
    assert_eq!(clone.skip_if_exists.as_deref(), Some(dir.as_str()));
    assert!(clone.args.ends_with(&[
        "--".to_string(),
        "git@github.com:me/notchling.git".to_string(),
        dir.clone()
    ]));
    let lines: Vec<String> = calls.iter().map(joined).collect();
    let fetch_origin = lines
        .iter()
        .position(|l| l.ends_with("fetch --no-tags -- origin +refs/heads/*:refs/remotes/origin/*"))
        .expect("origin fetch of every branch");
    let start = lines
        .iter()
        .position(|l| {
            l.ends_with(&format!(
                "update-ref refs/notchling/start/j-0123456789ab {HEAD}"
            ))
        })
        .expect("start ref");
    let fetch_bundle = lines
        .iter()
        .position(|l| l.ends_with("fetch --no-tags -- /data/in/t1 +refs/heads/feature/x:refs/notchling/in/j-0123456789ab"))
        .expect("bundle fetch");
    let verify = lines
        .iter()
        .position(|l| l.ends_with(&format!("cat-file -e {HEAD}^{{commit}}")))
        .expect("verify");
    let worktree = lines
        .iter()
        .position(|l| {
            l.ends_with(&format!(
                "worktree add -B notchling/j-0123456789ab /data/work/jobs/j-0123456789ab {HEAD}"
            ))
        })
        .expect("worktree");
    assert!(fetch_origin < fetch_bundle && fetch_bundle < verify && verify < worktree);
    assert!(verify < start && start < worktree);
    assert_eq!(
        calls[start].skip_if_exists.as_deref(),
        Some("/data/work/jobs/j-0123456789ab"),
        "the start ref is written only when the worktree is created"
    );
    assert!(
        !calls[fetch_origin].allow_failure,
        "a cached clone must really be refreshed"
    );
    assert_eq!(
        calls[worktree].skip_if_exists.as_deref(),
        Some("/data/work/jobs/j-0123456789ab")
    );
    assert!(!calls[verify].allow_failure);
    assert_eq!(calls[worktree].cwd, dir);
    let without = plan_prepare(&f, "j-0123456789ab", "/data/work", None).unwrap();
    assert!(!without
        .iter()
        .any(|c| joined(c).contains("refs/notchling/in/")));
}

#[cfg(unix)]
#[test]
fn repo_without_origin_is_refused() {
    let f = RepoFacts {
        origin: None,
        ..facts()
    };
    assert_eq!(
        plan_prepare(&f, "j-0123456789ab", "/data/work", None).unwrap_err(),
        NO_REMOTE
    );
    assert!(NO_REMOTE.starts_with("This repository has no remote; send its folder to"));
}

#[cfg(unix)]
#[test]
fn prepare_refuses_hostile_values() {
    let ok = |f: &RepoFacts| plan_prepare(f, "j-1", "/data/work", None);
    assert!(ok(&facts()).is_ok());
    for job in ["", "../x", "-j", "a/b", "j 1", "a\nb"] {
        assert!(
            plan_prepare(&facts(), job, "/data/work", None).is_err(),
            "{job:?}"
        );
        assert!(plan_return(job, "/data/work", "/o").is_err(), "{job:?}");
        assert!(plan_fetch_back("/r", "/b", "m1", job).is_err(), "{job:?}");
        assert!(
            plan_cleanup(&facts(), job, "/data/work").is_err(),
            "{job:?}"
        );
    }
    for branch in [
        "-x",
        "--upload-pack=sh",
        "a b",
        "a..b",
        "x.lock",
        ".x",
        "a//b",
        "a@{b",
        "",
        "HEAD",
        "a:b",
    ] {
        let f = RepoFacts {
            branch: branch.to_string(),
            ..facts()
        };
        assert!(ok(&f).is_err(), "{branch:?}");
        assert!(plan_send(&f, "/o").is_err(), "{branch:?}");
    }
    for head in ["", "main", "-p", &"g".repeat(40), &"a".repeat(41)] {
        let f = RepoFacts {
            head: head.to_string(),
            ..facts()
        };
        assert!(ok(&f).is_err(), "{head:?}");
    }
    for origin in [
        "-u=sh",
        "ext::sh -c touch% x",
        "https://user:secret@host/r.git",
        "http://host/r.git",
        "ftp://host/r",
        "a b",
        "host:",
        "",
    ] {
        let f = RepoFacts {
            origin: Some(origin.to_string()),
            ..facts()
        };
        assert!(ok(&f).is_err(), "{origin:?}");
    }
    assert!(plan_prepare(&facts(), "j-1", "relative", None).is_err());
    assert!(plan_prepare(&facts(), "j-1", "/data/work", Some("rel")).is_err());
}

#[test]
fn origins_that_are_fine() {
    for url in [
        "https://github.com/me/r.git",
        "https://me@github.com/me/r.git",
        "ssh://git@host:22/me/r.git",
        "git@github.com:me/r.git",
        "host.example:path/r",
        "/srv/git/r.git",
        "file:///srv/git/r.git",
        "ssh://git@[::1]/r.git",
    ] {
        assert!(origin_ok(url), "{url}");
    }
}

#[test]
fn origins_that_start_an_ssh_option() {
    for url in [
        "ssh://-oProxyCommand=x/r.git",
        "ssh://-oProxyCommand=x@host/r.git",
        "ssh://git@-oProxyCommand=x/r.git",
        "https://-x/r.git",
        "-oProxyCommand=x:r",
        "git@-oProxyCommand=x:r",
        "user@-host:r",
    ] {
        assert!(!origin_ok(url), "{url}");
    }
}

#[test]
fn branch_names_follow_git_rules() {
    for good in ["main", "feature/x", "a-b_c.d", "release/1.2", "é"] {
        assert!(valid_branch(good), "{good}");
    }
    for bad in [
        "", "-a", "a/", "/a", "a.", "a/.b", "a/b.lock", "a b", "a~b", "a^b", "a?b", "a*b", "a[b",
        "a\\b", "a\tb", "@", "a@{u}",
    ] {
        assert!(!valid_branch(bad), "{bad:?}");
    }
}

#[cfg(unix)]
#[test]
fn return_and_fetch_back_never_force_or_merge() {
    let back = plan_return("j-1", "/data/work", "/data/out.bundle").unwrap();
    assert_eq!(back.cwd, "/data/work/jobs/j-1");
    assert!(joined(&back).ends_with(
        "bundle create /data/out.bundle refs/notchling/start/j-1..refs/heads/notchling/j-1"
    ));
    assert!(!back.refusals.is_empty());
    let calls = plan_fetch_back("/repo", "/data/b", "M1 Laptop", "j-1").unwrap();
    assert_eq!(calls.len(), 1);
    let line = joined(&calls[0]);
    assert!(
        line.ends_with("fetch --no-tags -- /data/b refs/heads/notchling/j-1:refs/heads/notchling/m1-laptop/j-1"),
        "{line}"
    );
    assert_eq!(calls[0].cwd, "/repo");
    assert!(calls[0]
        .refusals
        .iter()
        .any(|(when, text)| when == "rejected"
            && text == "notchling/m1-laptop/j-1 already has other commits"));
}

#[test]
fn node_names_become_branch_parts() {
    assert_eq!(node_slug("vps-sing-1"), "vps-sing-1");
    assert_eq!(
        node_slug("Son's MacBook Pro.local"),
        "son-s-macbook-pro-local"
    );
    assert_eq!(node_slug("---"), "node");
    assert_eq!(node_slug(""), "node");
    assert!(node_slug(&"x".repeat(100)).len() <= 40);
}

#[cfg(unix)]
#[test]
fn facts_are_read_and_checked() {
    let out = |branch: Option<&str>, origin: Option<&str>, upstream: Option<&str>| FactsOut {
        root: "/repo\n".to_string(),
        head: format!("{HEAD}\n"),
        branch: branch.map(|b| format!("{b}\n")),
        origin: origin.map(|o| format!("{o}\n")),
        upstream: upstream.map(|u| format!("{u}\n")),
        unpushed: Some("3\n".to_string()),
    };
    let got = parse_facts(out(Some("main"), Some("git@h:o/r.git"), Some(UP))).unwrap();
    assert_eq!(got.root, "/repo");
    assert_eq!(got.head, HEAD);
    assert_eq!(got.branch, "main");
    assert_eq!(got.origin.as_deref(), Some("git@h:o/r.git"));
    assert_eq!(got.upstream.as_deref(), Some(UP));
    assert_eq!(got.unpushed, 3);
    let bare = parse_facts(out(Some("main"), None, None)).unwrap();
    assert_eq!(bare.origin, None);
    assert_eq!(bare.upstream, None);
    assert!(parse_facts(out(None, None, None))
        .unwrap_err()
        .contains("branch"));
    assert!(parse_facts(out(Some("a b"), None, None)).is_err());
    assert!(parse_facts(out(Some("main"), Some("ext::x"), None)).is_err());
    assert!(parse_facts(out(Some("main"), None, Some("zzz"))).is_err());
    for unpushed in [None, Some("x\n"), Some("-1\n"), Some("")] {
        let mut broken = out(Some("main"), None, None);
        broken.unpushed = unpushed.map(str::to_string);
        assert!(parse_facts(broken).is_err(), "{unpushed:?}");
    }
    assert!(facts_calls("relative").is_err());
    let calls = facts_calls("/repo").unwrap();
    assert!(joined(&calls.unpushed).ends_with("rev-list --count HEAD --not --remotes=origin"));
}

#[test]
fn the_branch_back_is_named_by_node_and_job() {
    assert_eq!(back_branch("m1", "j-1").unwrap(), "notchling/m1/j-1");
    assert!(back_branch("m1", "../x").is_err());
}
