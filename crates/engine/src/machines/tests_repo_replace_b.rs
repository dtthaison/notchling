//! Tests of Replace key, the second half: deletes, ambiguous swaps, the offer and the scripts in a
//! real shell (Swift `MachinesRepoStepsTests`).

use super::fakes_b::*;
use super::repo_common::*;
use super::step::{CheckResult, StepId, SubRow};
use super::steps_repo::{self, offers_replace};
use super::tests_repo_replace::*;

// MARK: a delete only removes a key of this run or of this machine (I1)

pub(super) const OTHER_PUB: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOtherKeyThatIsNotGitHubs0123456789abcdef agents-m2";

#[test]
fn a_hostile_keygen_that_returns_a_listed_key_is_refused_and_nothing_is_deleted() {
    // the "new" key is m2's, already on the repo under m2's title
    let m2 = key_json("5", OTHER_BODY, "agents-m2", true);
    let rig = replace_rig(Opts {
        new_read_only: None,
        keygen_out: Some(OTHER_PUB.into()),
        extra_listed: vec![m2],
        ..Opts::default()
    });
    let outcome = go(&rig);
    assert!(!outcome.ok);
    assert!(
        outcome
            .message
            .contains("the new key is already on GitHub; nothing changed"),
        "{}",
        outcome.message
    );
    assert_eq!(
        kinds(&rig),
        ["read old", "keygen", "gh list", "rm"],
        "no add, no GitHub delete, the .new files removed"
    );
}

#[test]
fn an_old_pub_that_holds_another_machines_key_is_never_deleted() {
    let m2 = key_json("5", OTHER_BODY, "agents-m2", true);
    let rig = replace_rig(Opts {
        old_id: None,
        old_pub_text: Some(OTHER_PUB.into()),
        extra_listed: vec![m2],
        ..Opts::default()
    });
    let outcome = go(&rig);
    let k = kinds(&rig);
    assert!(!has_prefix(&k, "gh delete"), "{k:?}");
    assert!(has(&k, "swap"), "the new key is in use");
    assert!(
        !outcome.ok,
        "the old key is left on GitHub: not a silent pass"
    );
    // it names the title found, says nothing was deleted, and stays red
    assert!(
        outcome.message.contains("replaced the key on m1; the old key on GitHub is titled 'agents-m2', not agents-m1, so nothing was deleted; remove it in Settings \u{2192} Deploy keys if it's m1's"),
        "{}",
        outcome.message
    );
}

#[test]
fn the_title_found_on_github_is_masked_and_cut_before_it_is_shown() {
    let token = long_token();
    let title = format!("agents-x{}{token}", "f".repeat(170));
    let mut rig = replace_rig(Opts {
        old_title: title,
        ..Opts::default()
    });
    rig = rig.hiding("m1-github-issues", &token);
    let outcome = go(&rig);
    assert!(!outcome.ok);
    assert_no_fragment(&token, &outcome.message);
    assert!(
        outcome.message.contains("titled 'agents-x"),
        "{}",
        outcome.message
    );
    assert!(!has_prefix(&kinds(&rig), "gh delete"));
}

#[test]
fn the_old_key_is_deleted_only_under_this_machines_own_titles() {
    let cases: [(&str, bool); 17] = [
        ("agents-m1", true),
        ("agents-m1-20261001", true),
        ("agents-m2", false),
        ("agents-m1-2026", false),
        ("agents-m10", false),
        ("agents-m1-20261001x", false),
        ("agents-m1-", false),
        ("", false),
        ("Agents-m1", false),
        ("deploy", false),
        ("xagents-m1", false),
        ("agents-m1\n", false),
        ("agents-m1-20261001\n", false),
        ("agents-m1\n-20261001", false),
        ("agents-m1-2026100\u{0661}", false),
        ("agents-m1-20261001-20261001", false),
        ("agents-m1 ", false),
    ];
    for (title, deleted) in cases {
        let rig = replace_rig(Opts {
            old_title: title.into(),
            ..Opts::default()
        });
        let outcome = go(&rig);
        let k = kinds(&rig);
        assert_eq!(has(&k, "gh delete 11"), deleted, "title {title:?}: {k:?}");
        assert_eq!(outcome.ok, deleted, "title {title:?}: {}", outcome.message);
        assert!(has(&k, "swap"), "{title:?}");
    }
}

#[test]
fn cleanup_deletes_only_a_key_of_this_run_under_this_runs_title() {
    // ssh fails after the add; GitHub now lists a key with the new text but another title: not
    // ours to delete
    let rig = replace_rig(Opts {
        ssh_ok: false,
        new_title_on_github: Some("somebody-elses-title".into()),
        ..Opts::default()
    });
    let outcome = go(&rig);
    assert!(!outcome.ok);
    let k = kinds(&rig);
    assert!(!has_prefix(&k, "gh delete"), "{k:?}");
    assert!(has(&k, "rm"));
    assert!(
        outcome.message.contains("nothing was deleted"),
        "{}",
        outcome.message
    );
    assert!(
        outcome.message.contains("Settings \u{2192} Deploy keys"),
        "{}",
        outcome.message
    );
}

#[test]
fn cleanup_after_a_failed_add_deletes_nothing_that_was_already_listed() {
    // The add fails, and the listing after it holds a key with the new key's text that this run
    // did not add: it has another id and another title (someone else's, put there meanwhile). The
    // cleanup must delete nothing and say so. m2's key, listed all along, stays too.
    let m2 = key_json("5", OTHER_BODY, "agents-m2", true);
    let rig = replace_rig(Opts {
        new_read_only: Some(true),
        add_code: 1,
        extra_listed: vec![m2],
        new_title_on_github: Some("agents-m2-someone-elses".into()),
        ..Opts::default()
    });
    let outcome = go(&rig);
    assert!(!outcome.ok);
    assert!(outcome.message.contains("add boom"), "{}", outcome.message);
    assert!(
        outcome.message.contains("nothing was deleted"),
        "{}",
        outcome.message
    );
    let k = kinds(&rig);
    assert!(!has_prefix(&k, "gh delete"), "{k:?}");
    assert!(has(&k, "rm"));
}

#[test]
fn a_listing_that_fails_before_the_add_stops_before_the_add() {
    let rig = Rig::new(mac("m1"));
    rig.runner
        .on(script_has("head -c 1024"), ok(&format!("{}\n", pub_line())));
    rig.runner
        .on(script_has("ssh-keygen"), ok(&format!("{}\n", new_pub())));
    rig.runner.on(
        |a| a.iter().any(|s| s == "--paginate"),
        result(1, "", "list boom"),
    );
    let outcome = go(&rig);
    assert!(!outcome.ok);
    assert!(outcome.message.contains("list boom"), "{}", outcome.message);
    let k = kinds(&rig);
    assert!(!has(&k, "gh add"));
    assert!(!has_prefix(&k, "gh delete"));
}

#[test]
fn key_text_is_normalised_to_type_and_base64_on_a_one_line_input() {
    let want = Some(format!("ssh-ed25519 {BODY}"));
    assert_eq!(
        normalised_key(&format!("ssh-ed25519 {BODY} agents-m1")),
        want
    );
    assert_eq!(
        normalised_key(&format!("ssh-ed25519 {BODY} other comment here\n")),
        want
    );
    assert_eq!(normalised_key(&format!("ssh-ed25519 {BODY}")), want);
    for bad in [
        format!("ssh-ed25519 {BODY}\nssh-ed25519 {OTHER_BODY}"),
        format!("ssh-ed25519 {BODY} x\ny"),
        format!("ssh-ed25519 {BODY}\r\nz"),
        format!("ssh-rsa {BODY}"),
        "ssh-ed25519".to_string(),
        "ssh-ed25519 !!!notbase64!!!".to_string(),
        String::new(),
        "\n".to_string(),
    ] {
        assert_eq!(normalised_key(&bad), None, "{bad:?}");
    }
}

#[test]
fn a_multi_line_key_field_from_github_never_matches() {
    // a listed "key" that holds two lines, the first one the new key plus a comment: not a match
    let hostile = format!(
        "{{\"id\":7,\"key\":\"ssh-ed25519 {NEW_BODY} x\\nssh-ed25519 {OTHER_BODY}\",\"title\":\"x\",\"read_only\":true}}"
    );
    let rig = replace_rig(Opts {
        extra_listed: vec![hostile],
        ..Opts::default()
    });
    let outcome = go(&rig);
    assert!(outcome.ok, "{}", outcome.message);
}

// MARK: an ambiguous swap deletes nothing (I2)

#[test]
fn a_swap_that_returns_255_is_cant_tell_and_deletes_nothing() {
    // 1, 2 and 3 are what a signal looks like to the runner: they prove nothing either
    for code in [255, 124, 125, 137, 4, 15, 9, 1, 2, 3, 40, 44] {
        let rig = replace_rig(Opts {
            swap_code: code,
            ..Opts::default()
        });
        let outcome = go(&rig);
        assert!(!outcome.ok, "{code}");
        let m = &outcome.message;
        assert!(
            m.contains("can't tell whether the key was replaced on m1"),
            "{m}"
        );
        assert!(
            m.contains("the swap may have happened; Check again before anything else"),
            "{m}"
        );
        assert!(m.contains("Nothing was removed from GitHub"), "{m}");
        assert!(
            !m.contains("untouched"),
            "never claims what it didn't prove: {m}"
        );
        let k = kinds(&rig);
        assert_eq!(k.last().map(String::as_str), Some("swap"), "{code}: {k:?}");
        assert!(
            !has(&k, "rm"),
            "{code}: the machine's files are left as they are"
        );
    }
}

#[test]
fn a_swap_that_fails_with_its_own_codes_is_undone_as_before() {
    // 41 the private key would not move, 43 a file was missing
    for code in [41, 43] {
        let rig = replace_rig(Opts {
            swap_code: code,
            ..Opts::default()
        });
        let outcome = go(&rig);
        assert!(!outcome.ok, "{code}");
        let k = kinds(&rig);
        assert!(has(&k, "rm"), "{code}: {k:?}");
        assert!(has(&k, "gh delete 22"), "{code}: {k:?}");
        assert!(!has(&k, "gh delete 11"), "{code}: {k:?}");
        assert!(
            outcome
                .message
                .contains("the old key and the ssh alias are untouched"),
            "{}",
            outcome.message
        );
    }
}

#[test]
fn a_swap_whose_second_move_failed_says_the_key_is_in_use_and_the_pub_may_be_stale() {
    let rig = replace_rig(Opts {
        swap_code: 42,
        ..Opts::default()
    });
    let outcome = go(&rig);
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("new key in use"),
        "{}",
        outcome.message
    );
    assert!(
        outcome.message.contains("Check again"),
        "{}",
        outcome.message
    );
    assert!(!has_prefix(&kinds(&rig), "gh delete"));
    let swap = rig
        .argvs()
        .into_iter()
        .map(|a| unquoted(last(&a)))
        .find(|s| s.contains("mv -f"))
        .unwrap();
    assert!(
        swap.contains("ssh-keygen -y"),
        "the public key file is rewritten from the private key when its move fails: {swap}"
    );
}

#[test]
fn an_unreadable_old_key_changes_nothing() {
    let rig = Rig::new(mac("m1"));
    rig.runner
        .on(script_has("head -c 1024"), ok("ssh-rsa AAAA\nrm -rf x\n"));
    let outcome = go(&rig);
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("unexpected public key"),
        "{}",
        outcome.message
    );
    assert_eq!(
        rig.calls().len(),
        1,
        "nothing but the read of the old key ran"
    );
}

#[test]
fn an_unsafe_machine_name_runs_nothing() {
    let mut m = mac("m1");
    m.name = "m1; touch x".into();
    let rig = Rig {
        machine: m.clone(),
        all: vec![m],
        ..replace_rig(Opts::default())
    };
    let outcome = go(&rig);
    assert!(!outcome.ok);
    assert!(rig.calls().is_empty());
}

/// A machine named `m1-20261001` would own the title `agents-m1-20261001`, which is how `m1`'s
/// replaced key is titled: Replace key refuses such a name, before any command.
#[test]
fn a_machine_name_that_looks_like_a_replaced_key_title_is_refused() {
    for name in ["m1-20261001", "vps-99999999", "a-00000000"] {
        let mut m = mac("m1");
        m.name = name.into();
        let rig = Rig {
            machine: m.clone(),
            all: vec![m],
            ..replace_rig(Opts::default())
        };
        let outcome = go(&rig);
        assert!(!outcome.ok, "{name}");
        assert!(
            outcome
                .message
                .contains("a machine name can't end in -<8 digits>"),
            "{}",
            outcome.message
        );
        assert!(rig.calls().is_empty(), "{name}");
    }
    for name in [
        "m1-2026100",
        "m1-202610011",
        "20261001",
        "m1-20261001-x",
        "m1",
    ] {
        let mut m = mac("m1");
        m.name = name.into();
        let rig = Rig {
            machine: m.clone(),
            all: vec![m],
            ..replace_rig(Opts::default())
        };
        go(&rig);
        assert!(!rig.calls().is_empty(), "{name} is a fine name");
    }
}

#[test]
fn masks_what_it_shows_before_cutting_it() {
    let token = long_token();
    let said = format!("{}{token}", "f".repeat(170));
    let rig = Rig::new(mac("m1")).hiding("m1-github-issues", &token);
    rig.runner
        .on(script_has("head -c 1024"), ok(&format!("{}\n", pub_line())));
    rig.runner
        .on(script_has("ssh-keygen"), result(1, "", &said));
    let outcome = go(&rig);
    assert!(!outcome.ok);
    assert_no_fragment(&token, &outcome.message);
    assert!(
        outcome.message.contains("[secret:m1-github-issues]"),
        "{}",
        outcome.message
    );
}

#[test]
fn replace_key_is_offered_when_the_check_passes_or_the_key_is_not_on_github() {
    assert!(offers_replace(Some(&CheckResult::pass("read-only key"))));
    let not_listed = CheckResult::fail(format!("the key is not on {REPO} yet"))
        .rows(vec![SubRow::new("Key on GitHub", false, "not listed")]);
    assert!(offers_replace(Some(&not_listed)));
    // no key on the machine, a writable key, a missing alias, can't tell, no result yet
    assert!(!offers_replace(Some(&CheckResult::fail(
        "no deploy key on m1 yet"
    ))));
    assert!(!offers_replace(Some(
        &CheckResult::fail("can write").rows(vec![SubRow::new(
            "Key on GitHub",
            false,
            "can write"
        )])
    )));
    assert!(!offers_replace(Some(
        &CheckResult::fail("no ssh alias").rows(vec![SubRow::new(
            "Key on GitHub",
            true,
            "read-only"
        )])
    )));
    assert!(!offers_replace(Some(&CheckResult::cant_tell(
        "waits for Reachable"
    ))));
    assert!(!offers_replace(None));
}

#[test]
fn the_probe_names_the_row_that_makes_replace_key_available() {
    let check = |keys: String| {
        let rig = Rig::new(mac("m1"));
        rig.runner.on(is_list, ok(&keys));
        let (_, parse) = remote(&find(
            &steps_repo::all(&rig.machine, &rig.machine.repos()[0].clone()),
            &StepId::deploy_key(REPO),
        ));
        parse(
            &super::step::RemoteOutput {
                text: format!("{}\n@@config\nalias=yes\n@@ssh\n{WELCOME}\n", pub_line()),
                code: 0,
            },
            &rig.step(),
        )
    };
    assert!(offers_replace(Some(&check(gh_keys(
        &format!("ssh-ed25519 {OTHER_BODY}"),
        Some(true)
    )))));
    assert!(offers_replace(Some(&check(gh_keys(
        &format!("ssh-ed25519 {BODY}"),
        Some(true)
    )))));
    assert!(!offers_replace(Some(&check(gh_keys(
        &format!("ssh-ed25519 {BODY}"),
        Some(false)
    )))));
}

#[test]
fn only_the_deploy_key_step_of_a_valid_repo_has_a_replace_plan() {
    let m = mac("m1");
    let all = steps_repo::all(&m, &m.repos()[0].clone());
    assert!(all[0].replace.is_some());
    assert!(all[1].replace.is_none());
    assert!(all[2].replace.is_none());
    let bad = super::registry::MachineRepo {
        github: "a/b; touch x".into(),
        local_path: "/x".into(),
        check_command: "true".into(),
        machine_path: None,
        verify_skill: None,
    };
    for step in steps_repo::all(&m, &bad) {
        assert!(step.replace.is_none(), "{}", step.id.raw());
    }
    // and the Deploy key's Fix itself is no Replace key
    assert!(!all[0].plan().unwrap().display.contains("deploy-key delete"));
}
