use super::*;
use std::path::{Path, PathBuf};

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|a| a.to_string()).collect()
}

#[test]
fn exec_arguments() {
    assert_eq!(
        arguments("hi", None, "/p", &Pick::default(), WORKSPACE_WRITE, false),
        s(&[
            "exec",
            "--json",
            "--skip-git-repo-check",
            "-c",
            "sandbox_mode=\"workspace-write\"",
            "-C",
            "/p",
            "hi"
        ])
    );
    assert_eq!(
        arguments(
            "more",
            Some("abc"),
            "/p",
            &Pick::default(),
            WORKSPACE_WRITE,
            false
        ),
        s(&[
            "exec",
            "resume",
            "--json",
            "--skip-git-repo-check",
            "-c",
            "sandbox_mode=\"workspace-write\"",
            "abc",
            "more"
        ])
    );
}

#[test]
fn network_is_opened_only_in_the_workspace_write_sandbox() {
    let open = "sandbox_workspace_write.network_access=true";
    let with = |sandbox: &str, network: bool| {
        arguments("hi", None, "/p", &Pick::default(), sandbox, network)
    };
    assert!(with(WORKSPACE_WRITE, true).iter().any(|a| a == open));
    assert!(!with(WORKSPACE_WRITE, false).iter().any(|a| a == open));
    assert!(
        !with("read-only", true).iter().any(|a| a == open),
        "read-only stays as it is"
    );
    let args = arguments(
        "more",
        Some("abc"),
        "/p",
        &Pick::default(),
        WORKSPACE_WRITE,
        true,
    );
    assert_eq!(&args[args.len() - 2..], ["abc", "more"]);
}

#[test]
fn a_picked_model_and_effort_go_on_every_turn() {
    let pick = Pick {
        model: Some("gpt-6.1-sol".into()),
        effort: Some("high".into()),
    };
    let args = arguments("more", Some("abc"), "/p", &pick, WORKSPACE_WRITE, false);
    assert_eq!(&args[..2], ["exec", "resume"]);
    let at = args.iter().position(|a| a == "-m").expect("-m");
    assert_eq!(args[at + 1], "gpt-6.1-sol");
    assert!(args.iter().any(|a| a == "model_reasoning_effort=\"high\""));
    assert_eq!(
        &args[args.len() - 2..],
        ["abc", "more"],
        "options before the session id and prompt"
    );
}

#[test]
fn a_prompt_that_looks_like_a_flag_follows_a_double_dash() {
    let args = arguments("-h", None, "/p", &Pick::default(), WORKSPACE_WRITE, false);
    assert_eq!(&args[args.len() - 3..], ["/p", "--", "-h"]);
    let args = arguments(
        "--version",
        Some("abc"),
        "/p",
        &Pick::default(),
        WORKSPACE_WRITE,
        false,
    );
    assert_eq!(&args[args.len() - 3..], ["--", "abc", "--version"]);
}

#[test]
fn what_a_caller_may_name_is_checked() {
    assert!(valid_session_id("019a5c3e-12ab-7cde-8f01-23456789abcd"));
    for bad in ["", "-x", "a b", "a/b", "../x", "a\nb", &"a".repeat(65)] {
        assert!(!valid_session_id(bad), "{bad:?}");
    }
    assert!(valid_model("gpt-6.1-sol"));
    for bad in ["", "-m", "a b", "a\"b", "a=b", &"a".repeat(65)] {
        assert!(!valid_model(bad), "{bad:?}");
    }
    assert!(valid_effort("xhigh"));
    for bad in ["", "a\"b", "high\" -c x=\"", "a b"] {
        assert!(!valid_effort(bad), "{bad:?}");
    }
}

#[test]
fn the_codex_command_is_found_in_the_usual_places() {
    let home = Path::new("/home/me");
    let only = |found: &'static str| move |p: &Path| p == Path::new(found);
    assert_eq!(
        find_binary(home, "", only("/opt/homebrew/bin/codex")),
        Some(PathBuf::from("/opt/homebrew/bin/codex"))
    );
    assert_eq!(
        find_binary(home, "", |_| true),
        Some(PathBuf::from("/home/me/.local/bin/codex"))
    );
    assert_eq!(
        find_binary(home, "/x/bin:/y/bin", only("/y/bin/codex")),
        Some(PathBuf::from("/y/bin/codex"))
    );
    assert_eq!(find_binary(home, "/x/bin", |_| false), None);
    assert_eq!(find_binary(home, "bin:.", only("bin/codex")), None);
}

#[test]
fn the_path_gains_the_usual_tool_folders_once() {
    let home = Path::new("/home/me");
    let path = augmented_path("/usr/bin:/bin", home);
    assert!(path.starts_with("/usr/bin:/bin:"));
    assert!(path.contains("/opt/homebrew/bin"));
    assert!(path.contains("/home/me/.local/bin"));
    assert_eq!(augmented_path(&path, home), path, "nothing is added twice");
}

#[test]
fn a_turn_spec_drops_no_color_and_runs_in_the_folder() {
    let spec = turn_spec(
        Path::new("/usr/local/bin/codex"),
        &Turn {
            prompt: "hi",
            session: None,
            folder: "/work",
            pick: &Pick::default(),
            network: true,
            codex_home: None,
        },
        Some("/usr/bin"),
    );
    assert_eq!(spec.program, PathBuf::from("/usr/local/bin/codex"));
    assert_eq!(spec.cwd, "/work");
    assert!(spec
        .args
        .contains(&"sandbox_workspace_write.network_access=true".to_string()));
    assert!(spec.env.contains(&("NO_COLOR".to_string(), None)));
    assert!(spec
        .env
        .contains(&("PATH".to_string(), Some("/usr/bin".to_string()))));
}

#[test]
fn a_turn_runs_with_the_codex_home_the_daemon_reads() {
    let pick = Pick::default();
    let turn = |codex_home| Turn {
        prompt: "hi",
        session: None,
        folder: "/work",
        pick: &pick,
        network: true,
        codex_home,
    };
    let spec = turn_spec(
        Path::new("/bin/codex"),
        &turn(Some(Path::new("/r/codex"))),
        None,
    );
    assert!(spec
        .env
        .contains(&("CODEX_HOME".to_string(), Some("/r/codex".to_string()))));
    let spec = turn_spec(Path::new("/bin/codex"), &turn(None), None);
    assert!(spec.env.iter().all(|(key, _)| key != "CODEX_HOME"));
}

#[test]
fn over_a_fixture_root_only_the_roots_own_codex_is_found() {
    let root = Path::new("/r");
    let only = |wanted: &'static str| move |p: &Path| p == Path::new(wanted);
    assert_eq!(
        find_fixture_binary(root, only("/r/.local/bin/codex")),
        Some(PathBuf::from("/r/.local/bin/codex"))
    );
    for real in [
        "/opt/homebrew/bin/codex",
        "/usr/local/bin/codex",
        "/r/.npm-global/bin/codex",
        "/usr/bin/codex",
    ] {
        assert_eq!(find_fixture_binary(root, only(real)), None, "{real}");
    }
}

#[test]
fn a_turn_ends_as_the_swift_runner_judged_it() {
    let exit = |code: Option<i32>, signaled: bool| ExitInfo { code, signaled };
    assert_eq!(failure_of(&exit(Some(0), false), ""), None);
    assert_eq!(
        failure_of(&exit(Some(130), false), "x"),
        None,
        "stopped on purpose"
    );
    assert_eq!(failure_of(&exit(None, true), "x"), None, "a signal");
    assert_eq!(
        failure_of(&exit(Some(1), false), ""),
        Some("Codex exited with code 1.".to_string())
    );
    assert_eq!(
        failure_of(&exit(Some(1), false), "rate limited"),
        Some("rate limited".to_string())
    );
    assert_eq!(
        failure_of(&exit(None, false), ""),
        Some("Codex exited without a status.".to_string())
    );
}
