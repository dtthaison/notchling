//! Tests of Replace key (Swift `MachinesRepoStepsTests`, second half): the order of its checks, its
//! undo, what it deletes and what it never does. The machine and GitHub are scripted; the three
//! tests that run the scripts in a real `/bin/sh` use a throwaway HOME and stubs for `git` and
//! `ssh`.

use super::fakes_b::*;
use super::registry::Machine;
use super::repo_common::*;
use super::step::{FixOutcome, FixPlan, StepId};
use super::steps_repo;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub(super) const NEW_TITLE: &str = "agents-m1-20270115";

pub(super) fn new_pub() -> String {
    format!("ssh-ed25519 {NEW_BODY} {NEW_TITLE}")
}

pub(super) fn replace_plan(m: &Machine) -> FixPlan {
    let step = find(
        &steps_repo::all(m, &m.repos()[0].clone()),
        &StepId::deploy_key(REPO),
    );
    step.replace.expect("a Replace key plan")
}

pub(super) fn key_json(id: &str, body: &str, title: &str, read_only: bool) -> String {
    format!(
        "{{\"id\":{id},\"key\":\"ssh-ed25519 {body}\",\"title\":{},\"read_only\":{read_only}}}",
        json!(title)
    )
}

/// What a machine and GitHub do for Replace key: the old key is id 11 on GitHub, the new one
/// becomes id 22.
pub(super) struct Opts {
    pub(super) ssh_ok: bool,
    pub(super) push_accepted: bool,
    pub(super) new_read_only: Option<bool>,
    pub(super) old_id: Option<&'static str>,
    pub(super) delete_old_code: i32,
    pub(super) delete_new_code: i32,
    pub(super) add_code: i32,
    pub(super) keygen_out: Option<String>,
    pub(super) old_title: String,
    pub(super) old_pub_text: Option<String>,
    pub(super) extra_listed: Vec<String>,
    pub(super) new_title_on_github: Option<String>,
    pub(super) swap_code: i32,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            ssh_ok: true,
            push_accepted: false,
            new_read_only: Some(true),
            old_id: Some("11"),
            delete_old_code: 0,
            delete_new_code: 0,
            add_code: 0,
            keygen_out: None,
            old_title: "agents-m1".into(),
            old_pub_text: None,
            extra_listed: Vec::new(),
            new_title_on_github: None,
            swap_code: 0,
        }
    }
}

pub(super) fn replace_rig(o: Opts) -> Rig {
    let rig = Rig::new(mac("m1"));
    let mut before = o.extra_listed.clone();
    if let Some(id) = o.old_id {
        before.push(key_json(id, BODY, &o.old_title, true));
    }
    let mut after = before.clone();
    if let Some(ro) = o.new_read_only {
        let title = o.new_title_on_github.as_deref().unwrap_or(NEW_TITLE);
        after.push(key_json("22", NEW_BODY, title, ro));
    }
    let (before, after) = (
        format!("[{}]", before.join(",")),
        format!("[{}]", after.join(",")),
    );
    // the first listing is the one before the add; every later one is after it
    let listings = Arc::new(AtomicUsize::new(0));
    rig.runner.on_call(is_list, move |_| {
        if listings.fetch_add(1, Ordering::SeqCst) == 0 {
            ok(&before)
        } else {
            ok(&after)
        }
    });
    let r = &rig.runner;
    r.on(script_has("mv -f"), result(o.swap_code, "", ""));
    let gh_has = |verb: &'static str, id: &'static str| {
        move |a: &[String]| a[0] == gh() && a.iter().any(|s| s == verb) && a.iter().any(|s| s == id)
    };
    r.on(
        gh_has("delete", "11"),
        result(
            o.delete_old_code,
            "",
            if o.delete_old_code == 0 {
                ""
            } else {
                "delete boom"
            },
        ),
    );
    r.on(
        gh_has("delete", "22"),
        result(
            o.delete_new_code,
            "",
            if o.delete_new_code == 0 {
                ""
            } else {
                "delete new boom"
            },
        ),
    );
    r.on(
        |a| a[0] == gh() && a.iter().any(|s| s == "add"),
        result(
            o.add_code,
            "",
            if o.add_code == 0 { "" } else { "add boom" },
        ),
    );
    r.on(
        script_has("ssh-keygen"),
        ok(&format!("{}\n", o.keygen_out.unwrap_or_else(new_pub))),
    );
    r.on(
        script_has("head -c 1024"),
        ok(&format!("{}\n", o.old_pub_text.unwrap_or_else(pub_line))),
    );
    r.on(
        script_has("-T git@github.com"),
        ok(if o.ssh_ok {
            format!("{WELCOME}\n")
        } else {
            "git@github.com: Permission denied (publickey).\n".to_string()
        }
        .as_str()),
    );
    r.on(
        script_has("push --dry-run"),
        ok(&if o.push_accepted {
            format!(
                "To github.com:{REPO}.git\n * [new branch] HEAD -> agents-push-test\n@@push 0\n"
            )
        } else {
            format!("{READ_ONLY_MSG}\n@@push 128\n")
        }),
    );
    rig
}

/// What each call was, in order: `gh list`, `gh add`, `gh delete <id>`, or the kind of script sent
/// to the machine.
pub(super) fn kinds(rig: &Rig) -> Vec<String> {
    rig.argvs()
        .iter()
        .map(|argv| {
            if argv[0] == gh() {
                if argv.iter().any(|s| s == "add") {
                    return "gh add".to_string();
                }
                if let Some(i) = argv.iter().position(|s| s == "delete") {
                    return format!("gh delete {}", argv[i + 1]);
                }
                return "gh list".into();
            }
            let script = unquoted(last(argv));
            let kind = if script.contains("mv -f") {
                "swap"
            } else if script.contains("ssh-keygen") {
                "keygen"
            } else if script.contains("head -c 1024") {
                "read old"
            } else if script.contains("-T git@github.com") {
                "ssh -T"
            } else if script.contains("push --dry-run") {
                "push"
            } else if script.contains("rm -f") {
                "rm"
            } else {
                "other"
            };
            kind.to_string()
        })
        .collect()
}

pub(super) fn go(rig: &Rig) -> FixOutcome {
    (replace_plan(&rig.machine).run)(&rig.fix())
}

pub(super) fn has(k: &[String], what: &str) -> bool {
    k.iter().any(|s| s == what)
}

pub(super) fn has_prefix(k: &[String], what: &str) -> bool {
    k.iter().any(|s| s.starts_with(what))
}

#[test]
fn happy_path_runs_in_order_and_deletes_only_the_old_key_last() {
    let rig = replace_rig(Opts::default());
    let outcome = go(&rig);
    assert!(outcome.ok, "{}", outcome.message);
    assert_eq!(
        kinds(&rig),
        [
            "read old",
            "keygen",
            "gh list",
            "gh add",
            "ssh -T",
            "push",
            "gh list",
            "swap",
            "gh delete 11"
        ]
    );
    let calls = rig.argvs();
    let keygen = unquoted(last(&calls[1]));
    assert!(
        keygen.contains("ssh-keygen -q -t ed25519 -N '' -C 'agents-m1-20270115' -f"),
        "{keygen}"
    );
    assert!(
        keygen.contains(".ssh/agents-dtthaison-lingeva-mono.new'"),
        "{keygen}"
    );
    assert!(
        keygen.contains("cat \"$HOME\"/'.ssh/agents-dtthaison-lingeva-mono.new.pub'"),
        "{keygen}"
    );
    assert_eq!(
        calls[2],
        list_argv(REPO),
        "the keys are listed before the add"
    );
    assert_eq!(
        &calls[3][..4],
        &[gh(), "repo".into(), "deploy-key".into(), "add".into()]
    );
    assert_eq!(
        &calls[3][calls[3].len() - 4..],
        &["-R", REPO, "--title", NEW_TITLE]
    );
    assert_eq!(calls[3].len(), 9, "no flag besides -R and --title");
    let ssh = unquoted(last(&calls[4]));
    assert!(
        ssh.contains(".ssh/agents-dtthaison-lingeva-mono.new'"),
        "{ssh}"
    );
    assert!(
        ssh.contains("IdentitiesOnly=yes")
            && ssh.contains("BatchMode=yes")
            && ssh.contains("-F /dev/null"),
        "{ssh}"
    );
    assert!(
        ssh.contains("StrictHostKeyChecking=yes") && ssh.contains("</dev/null"),
        "{ssh}"
    );
    let push = unquoted(last(&calls[5]));
    assert!(
        push.contains("core.hooksPath=/dev/null")
            && push.contains("push --dry-run")
            && push.contains("</dev/null"),
        "{push}"
    );
    assert!(
        push.contains("GIT_TERMINAL_PROMPT=0")
            && push.contains("IdentitiesOnly=yes")
            && push.contains(".new"),
        "{push}"
    );
    assert!(
        push.contains("StrictHostKeyChecking=yes") && push.contains("-i '$k'"),
        "{push}"
    );
    assert_eq!(calls[6], list_argv(REPO));
    let swap = unquoted(last(&calls[7]));
    assert!(
        swap.contains(".new.pub") && swap.contains("mv -f"),
        "{swap}"
    );
    assert!(
        !swap.contains(".ssh/config"),
        "the alias file name is the same, so ssh config is not touched"
    );
    assert_eq!(calls[8], delete_argv("11", REPO));
    assert!(rig.calls().iter().all(|c| c.stdin.is_none()));
    assert!(
        outcome.message.contains("old key was deleted"),
        "{}",
        outcome.message
    );
}

#[test]
fn never_uses_the_write_flag_and_the_display_shows_the_commands() {
    let rig = replace_rig(Opts::default());
    go(&rig);
    for argv in rig.argvs() {
        assert!(!argv.iter().any(|a| a.contains("allow-write")), "{argv:?}");
        assert!(!argv.iter().any(|a| a == "-w"), "{argv:?}");
    }
    let plan = replace_plan(&rig.machine);
    let d = &plan.display;
    assert!(!d.contains("allow-write"));
    for needle in [
        "ssh-keygen",
        "deploy-key add",
        "deploy-key delete",
        "push --dry-run",
        ".new",
    ] {
        assert!(d.contains(needle), "{needle}");
    }
    assert!(!plan.needs_admin);
    // every GitHub call is shown, the cleanup's too, and the date is said to be UTC
    assert!(d.contains("deploy-key delete <id of the new key>"), "{d}");
    assert!(d.contains("deploy-key delete <id of the old key>"), "{d}");
    assert!(
        d.split(&format!("{} api --paginate repos/{REPO}/keys", gh()))
            .count()
            >= 4,
        "the list before the add, the check, the cleanup"
    );
    assert!(d.contains("today's date in UTC"), "{d}");
}

pub(super) fn assert_cleaned_up(rig: &Rig) {
    let k = kinds(rig);
    assert!(!has(&k, "swap"), "{k:?}");
    assert!(
        !has(&k, "gh delete 11"),
        "the old key stays on GitHub: {k:?}"
    );
    assert!(has(&k, "rm"), "the .new files are removed: {k:?}");
    assert!(
        has(&k, "gh delete 22"),
        "the new key is deleted from GitHub: {k:?}"
    );
    let rm = rig
        .argvs()
        .into_iter()
        .map(|a| unquoted(last(&a)))
        .find(|s| s.contains("rm -f") && !s.contains("ssh-keygen"))
        .unwrap_or_default();
    assert!(rm.contains(".new'") && rm.contains(".new.pub'"), "{rm}");
}

#[test]
fn a_new_key_that_can_push_is_undone_and_says_why() {
    let rig = replace_rig(Opts {
        push_accepted: true,
        ..Opts::default()
    });
    let outcome = go(&rig);
    assert!(!outcome.ok);
    assert!(outcome.message.contains("can write"), "{}", outcome.message);
    assert_cleaned_up(&rig);
    assert!(outcome.message.contains("old key"), "{}", outcome.message);
}

#[test]
fn a_new_key_that_cannot_authenticate_is_undone() {
    let rig = replace_rig(Opts {
        ssh_ok: false,
        ..Opts::default()
    });
    let outcome = go(&rig);
    assert!(!outcome.ok);
    assert!(outcome.message.contains("ssh"), "{}", outcome.message);
    assert!(
        !has(&kinds(&rig), "push"),
        "the push is not tried when ssh failed"
    );
    assert_cleaned_up(&rig);
}

#[test]
fn a_new_key_github_lists_as_writable_is_undone() {
    let rig = replace_rig(Opts {
        new_read_only: Some(false),
        ..Opts::default()
    });
    let outcome = go(&rig);
    assert!(!outcome.ok);
    assert!(outcome.message.contains("can write"), "{}", outcome.message);
    assert_cleaned_up(&rig);
}

#[test]
fn a_failed_add_looks_for_the_new_key_and_stops_before_any_check() {
    let rig = replace_rig(Opts {
        new_read_only: None,
        add_code: 1,
        ..Opts::default()
    });
    let outcome = go(&rig);
    assert!(!outcome.ok);
    assert!(outcome.message.contains("add boom"), "{}", outcome.message);
    let k = kinds(&rig);
    assert!(
        !(has(&k, "ssh -T") || has(&k, "swap") || has(&k, "gh delete 11")),
        "{k:?}"
    );
    assert!(has(&k, "rm"), "{k:?}");
}

#[test]
fn a_cleanup_that_cannot_delete_the_new_key_tells_where_to_remove_it() {
    let rig = replace_rig(Opts {
        push_accepted: true,
        delete_new_code: 1,
        ..Opts::default()
    });
    let outcome = go(&rig);
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("Settings \u{2192} Deploy keys"),
        "{}",
        outcome.message
    );
    assert!(outcome.message.contains(NEW_TITLE), "{}", outcome.message);
    assert!(!has(&kinds(&rig), "gh delete 11"));
}

#[test]
fn a_keygen_that_gives_something_else_stops_and_cleans_up() {
    let rig = replace_rig(Opts {
        keygen_out: Some("ssh-rsa AAAA\nrm -rf /\n".into()),
        ..Opts::default()
    });
    let outcome = go(&rig);
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("unexpected public key"),
        "{}",
        outcome.message
    );
    assert_eq!(kinds(&rig), ["read old", "keygen", "rm"]);
}

#[test]
fn a_failed_delete_of_the_old_key_after_the_swap_is_not_a_silent_pass() {
    let rig = replace_rig(Opts {
        delete_old_code: 1,
        ..Opts::default()
    });
    let outcome = go(&rig);
    assert!(!outcome.ok);
    for needle in [
        "new key in use",
        "old key is still on GitHub",
        "Settings \u{2192} Deploy keys",
    ] {
        assert!(
            outcome.message.contains(needle),
            "{needle}: {}",
            outcome.message
        );
    }
    let k = kinds(&rig);
    assert_eq!(&k[k.len() - 2..], ["swap", "gh delete 11"]);
    assert!(!has(&k, "rm"), "the swap happened; nothing is rolled back");
}

#[test]
fn an_old_id_that_is_not_an_integer_deletes_nothing() {
    for bad in [
        "\"11; touch x\"",
        "\"abc\"",
        "\"11\"",
        "1.5",
        "true",
        "null",
        "-3",
        "0",
    ] {
        let rig = replace_rig(Opts {
            old_id: Some(bad),
            ..Opts::default()
        });
        let outcome = go(&rig);
        assert!(!outcome.ok, "{bad}");
        assert!(
            outcome.message.contains("not a number"),
            "{bad}: {}",
            outcome.message
        );
        let k = kinds(&rig);
        assert!(
            !k.iter()
                .any(|s| s.starts_with("gh delete") && s != "gh delete 22"),
            "{bad}: {k:?}"
        );
        assert!(!has(&k, "swap"), "{bad}: nothing was replaced: {k:?}");
        for argv in rig.argvs() {
            assert!(!argv.iter().any(|a| a.contains("touch")), "{argv:?}");
        }
    }
}

#[test]
fn when_github_does_not_list_the_old_key_nothing_is_deleted_and_it_says_so() {
    let rig = replace_rig(Opts {
        old_id: None,
        ..Opts::default()
    });
    let outcome = go(&rig);
    assert!(outcome.ok, "{}", outcome.message);
    assert!(
        outcome.message.contains("not listed"),
        "{}",
        outcome.message
    );
    let k = kinds(&rig);
    assert!(has(&k, "swap"));
    assert!(!has_prefix(&k, "gh delete"), "{k:?}");
}

#[test]
fn the_old_key_is_matched_by_its_key_text_not_by_title() {
    // another key with the old key's title and a lower id must not be deleted
    let decoy = key_json("5", OTHER_BODY, "agents-m1", true);
    let rig = replace_rig(Opts {
        extra_listed: vec![decoy],
        ..Opts::default()
    });
    let outcome = go(&rig);
    assert!(outcome.ok, "{}", outcome.message);
    assert_eq!(rig.argvs().last().unwrap(), &delete_argv("11", REPO));
    assert!(!rig
        .argvs()
        .iter()
        .any(|a| a.iter().any(|s| s == "delete") && a.iter().any(|s| s == "5")));
}
