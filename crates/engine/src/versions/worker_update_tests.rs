use super::worker_update::{
    busy_script, busy_text, failure_text, parse_busy, plan, strict, unread_how_to, Move,
};
use super::{Method, Tool, ToolState, Version};

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn on(version: &str, method: Method) -> ToolState {
    ToolState::Installed {
        version: Some(v(version)),
        method,
        path: "/home/w/.local/bin/x".into(),
    }
}

fn run(tool: Tool, state: &ToolState, to: &str) -> String {
    match plan("m1", tool, state, &v(to)).unwrap() {
        Move::Run(script) => script,
        other => panic!("{other:?}"),
    }
}

fn how_to(tool: Tool, state: &ToolState, to: &str) -> String {
    match plan("m1", tool, state, &v(to)).unwrap() {
        Move::HowTo(text) => text,
        other => panic!("{other:?}"),
    }
}

#[test]
fn strict_takes_only_three_numbers() {
    assert_eq!(strict(&v("2.1.327")), Some("2.1.327"));
    assert_eq!(strict(&v("0.166.0")), Some("0.166.0"));
    assert_eq!(strict(&v("2.1.327-beta.1")), None);
    assert_eq!(strict(&v("2.1.327-rc")), None);
}

#[test]
fn claude_native_installs_the_exact_version() {
    let script = run(Tool::Claude, &on("2.1.318", Method::Native), "2.1.327");
    assert_eq!(script, "'claude' 'install' '2.1.327'");
}

#[test]
fn claude_npm_installs_the_exact_version() {
    let script = run(Tool::Claude, &on("2.1.318", Method::Npm), "2.1.327");
    assert_eq!(
        script,
        "'npm' 'install' '-g' '@anthropic-ai/claude-code@2.1.327'"
    );
}

#[test]
fn codex_npm_installs_the_exact_version() {
    let script = run(Tool::Codex, &on("0.118.0", Method::Npm), "0.166.0");
    assert_eq!(script, "'npm' 'install' '-g' '@openai/codex@0.166.0'");
}

#[test]
fn codex_standalone_runs_the_installer_with_the_release() {
    let script = run(Tool::Codex, &on("0.118.0", Method::Standalone), "0.166.0");
    assert!(script.starts_with("'sh' '-c' '"), "{script}");
    assert!(
        script.contains("https://chatgpt.com/codex/install.sh"),
        "{script}"
    );
    assert!(script.contains("--release 0.166.0"), "{script}");
}

#[test]
fn homebrew_gets_a_how_to_not_a_run() {
    for tool in [Tool::Claude, Tool::Codex] {
        let text = how_to(tool, &on("1.0.0", Method::Brew), "2.0.0");
        assert!(text.contains("Homebrew"), "{text}");
        assert!(text.contains("m1"), "{text}");
        assert!(text.contains("2.0.0"), "{text}");
    }
}

#[test]
fn apt_gets_a_how_to_not_a_run() {
    let text = how_to(Tool::Codex, &on("1.0.0", Method::Apt), "2.0.0");
    assert!(text.contains("apt"), "{text}");
    assert!(text.contains("exact version"), "{text}");
}

#[test]
fn an_unknown_method_gets_a_how_to() {
    let text = how_to(Tool::Claude, &on("1.0.0", Method::Unknown), "2.0.0");
    assert!(
        text.contains("can't tell how Claude Code was installed"),
        "{text}"
    );
}

#[test]
fn antigravity_never_runs() {
    for method in [Method::Standalone, Method::Unknown, Method::Brew] {
        let state = ToolState::Installed {
            version: None,
            method,
            path: "/home/w/.local/bin/agy".into(),
        };
        let text = how_to(Tool::Agy, &state, "1.5.9");
        assert!(text.contains("Antigravity"), "{text}");
        assert!(text.contains("agy update"), "{text}");
    }
    assert!(unread_how_to("m1", Tool::Agy, &v("1.5.9")).is_some());
    assert!(unread_how_to("m1", Tool::Claude, &v("1.5.9")).is_none());
}

#[test]
fn nothing_to_do_says_why() {
    let same = plan(
        "m1",
        Tool::Claude,
        &on("2.1.327", Method::Native),
        &v("2.1.327"),
    );
    assert!(same.unwrap_err().contains("already 2.1.327"));
    let absent = plan("m1", Tool::Claude, &ToolState::NotInstalled, &v("2.1.327"));
    assert!(absent.unwrap_err().contains("isn't installed on m1"));
    let notchling = plan(
        "m1",
        Tool::Notchling,
        &on("1.0.0", Method::Release),
        &v("2.0.0"),
    );
    assert!(notchling.is_err());
}

#[test]
fn a_version_that_is_not_strict_never_reaches_a_command() {
    for bad in ["2.1.327-beta", "2.1.327-rc.1", "1.0.0-0"] {
        let out = plan("m1", Tool::Claude, &on("2.1.318", Method::Native), &v(bad));
        assert!(out.is_err(), "{bad}: {out:?}");
    }
}

#[test]
fn an_unreadable_version_still_installs_the_exact_one() {
    let state = ToolState::Installed {
        version: None,
        method: Method::Native,
        path: "/home/w/.local/bin/claude".into(),
    };
    assert_eq!(
        run(Tool::Claude, &state, "2.1.327"),
        "'claude' 'install' '2.1.327'"
    );
}

#[test]
fn the_busy_script_names_the_tool_and_the_answer_parses() {
    let claude = busy_script(Tool::Claude);
    assert!(claude.contains("\"claude\""), "{claude}");
    assert!(claude.contains("/claude-code/"), "{claude}");
    let codex = busy_script(Tool::Codex);
    assert!(codex.contains("\"codex\""), "{codex}");
    assert!(codex.contains("/@openai/codex/"), "{codex}");
    assert_eq!(parse_busy("busy=0\n"), Some(0));
    assert_eq!(parse_busy("noise\nbusy=3\n"), Some(3));
    assert_eq!(parse_busy(""), None);
    assert_eq!(parse_busy("busy=many"), None);
}

#[test]
fn texts_name_the_tool_the_machine_and_the_cause() {
    assert!(busy_text("m1", Tool::Claude, 1).contains("1 process)"));
    assert!(busy_text("m1", Tool::Claude, 2).contains("2 processes)"));
    let eacces = failure_text(
        Tool::Codex,
        "m1",
        "",
        "npm ERR! code EACCES\nnpm ERR! path /usr/lib",
    );
    assert!(eacces.starts_with("Codex didn't install on m1"), "{eacces}");
    assert!(eacces.contains("global folder"), "{eacces}");
    let plain = failure_text(Tool::Claude, "m1", "", "");
    assert_eq!(plain, "Claude Code didn't install on m1");
}

fn run_busy(tool: Tool, ps: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("nl-busy-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fake = dir.join("ps");
    std::fs::write(&fake, format!("#!/bin/sh\n{ps}\n")).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:/usr/bin:/bin", dir.display());
    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(busy_script(tool))
        .env("PATH", path)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[cfg(unix)]
#[test]
fn a_failing_ps_gives_no_answer_so_the_update_refuses() {
    let out = run_busy(Tool::Claude, "exit 1");
    assert_eq!(parse_busy(&out), None, "{out:?}");
    let out = run_busy(Tool::Claude, "echo 'ps: unknown option' >&2; exit 64");
    assert_eq!(parse_busy(&out), None, "{out:?}");
    let out = run_busy(Tool::Codex, "exit 0");
    assert_eq!(
        parse_busy(&out),
        None,
        "no output at all is no answer: {out:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_working_ps_counts_the_tools_processes() {
    let listing = "echo '/bin/zsh -l'; echo 'claude --resume x'; echo 'node /u/lib/node_modules/@anthropic-ai/claude-code/cli.js'; echo 'vim notes'";
    assert_eq!(parse_busy(&run_busy(Tool::Claude, listing)), Some(2));
    assert_eq!(parse_busy(&run_busy(Tool::Codex, listing)), Some(0));
}
