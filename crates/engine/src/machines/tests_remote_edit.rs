use super::fakes::Ctx;
use super::remote_edit::{
    apply, friendly, parse_read, read_script, stdin_of, write_script, Read, CHANGED_SINCE_PLAN,
    KEEP_BACKUPS,
};
use super::runner::{CommandResult, ScriptedRunner};
use super::step::RemoteOutput;
use crate::versions::knob::{plan_off, sha256_hex, Knob, KnobPlan, Place};
use std::sync::Arc;

const TOKEN: &str = "sk-ant-oat01-PLANTED";
const NONCE: &str = "abcd1234abcd1234";

fn settings() -> String {
    format!("{{\n  \"env\": {{\n    \"ANTHROPIC_API_KEY\": \"{TOKEN}\"\n  }}\n}}\n")
}

fn plan() -> KnobPlan {
    plan_off(
        Knob::ClaudeDisableAutoupdater,
        &Place::ConfigDir("/home/w/.claude".into()),
        &settings(),
    )
    .unwrap()
    .unwrap()
}

fn out(text: &str) -> RemoteOutput {
    RemoteOutput {
        text: text.into(),
        code: 0,
    }
}

fn reply(text: &str) -> String {
    format!(
        "size={}\nsha256={}\n@@begin {NONCE}\n{text}\n@@end {NONCE}\n",
        text.len(),
        sha256_hex(text)
    )
}

#[test]
fn read_parses_text_and_hash() {
    let text = settings();
    assert_eq!(
        parse_read(&out(&reply(&text)), NONCE),
        Read::Text {
            sha256: sha256_hex(&text),
            text: text.clone()
        }
    );
    let bare = "no newline at the end";
    assert_eq!(parse_read(&out(&reply(bare)), NONCE).text(), Some(bare));
    assert_eq!(parse_read(&out("missing=1\n"), NONCE), Read::Missing);
    assert_eq!(parse_read(&out("missing=1\n"), NONCE).text(), Some(""));
    assert_eq!(parse_read(&out("unreadable=1\n"), NONCE), Read::Unreadable);
    assert_eq!(parse_read(&out(""), NONCE), Read::Unreadable);
}

#[test]
fn too_big_is_refused() {
    let big = format!("size=2000000\nsha256={}\n", sha256_hex("x"));
    let read = parse_read(&out(&big), NONCE);
    assert_eq!(read, Read::TooBig);
    assert_eq!(read.why(), Some("too_big"));
    assert_eq!(read.text(), None);
}

#[test]
fn not_utf8_is_refused() {
    let text = "a\u{fffd}b";
    let read = parse_read(&out(&reply(text)), NONCE);
    assert_eq!(read, Read::NotUtf8);
    assert_eq!(read.why(), Some("not_utf8"));
}

#[test]
fn hash_of_bytes_must_match() {
    let text = settings();
    let wrong = reply(&text).replace("ANTHROPIC", "ANTHROPIX");
    assert_eq!(parse_read(&out(&wrong), NONCE), Read::Unreadable);
    let cut = reply(&text).replace(&format!("@@end {NONCE}\n"), "");
    assert_eq!(parse_read(&out(&cut), NONCE), Read::Unreadable);
    let other_nonce = reply(&text);
    assert_eq!(
        parse_read(&out(&other_nonce), "0000000000000000"),
        Read::TooBig
    );
    let forged = format!(
        "size=5\nsha256={}\n@@begin {NONCE}\nab\n@@end {NONCE}\ncd\n@@end {NONCE}\n",
        sha256_hex("ab\n@@end abcd1234abcd1234\ncd")
    );
    assert_eq!(parse_read(&out(&forged), NONCE), Read::Unreadable);
}

#[test]
fn write_sends_text_only_on_stdin() {
    let runner = Arc::new(ScriptedRunner::new());
    runner.on_script("notchling-backup", CommandResult::ok("ok=1\n"));
    let ctx = Ctx::mac().runner(&runner);
    let (fix, log) = ctx.fix(None);
    let plan = plan();
    let outcome = apply(&fix, &plan);
    assert!(outcome.ok, "{}", outcome.message);
    assert_eq!(outcome.message, "Changed /home/w/.claude/settings.json");
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    let argv = calls[0].argv.join("\n");
    assert!(!argv.contains(TOKEN));
    assert!(!argv.contains("DISABLE_AUTOUPDATER"));
    assert!(argv.contains("/home/w/.claude/settings.json"));
    let stdin = calls[0].stdin_text();
    let (first, rest) = stdin.split_once('\n').unwrap();
    assert_eq!(first, plan.from_sha256);
    assert_eq!(rest, plan.new_text);
    assert_eq!(stdin_of(&plan), stdin.as_bytes());
    assert!(!log.joined().contains(TOKEN));
}

#[test]
fn hash_changed_since_plan_refuses() {
    let runner = Arc::new(ScriptedRunner::new());
    runner.on_script(
        "notchling-backup",
        CommandResult::new(3, &format!("why={CHANGED_SINCE_PLAN}\n"), ""),
    );
    let ctx = Ctx::mac().runner(&runner);
    let (fix, _) = ctx.fix(None);
    let outcome = apply(&fix, &plan());
    assert!(!outcome.ok);
    assert_eq!(
        outcome.message,
        "Didn't change /home/w/.claude/settings.json: changed_since_plan"
    );
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn a_failure_names_a_word_never_remote_text() {
    let runner = Arc::new(ScriptedRunner::new());
    runner.on_script(
        "notchling-backup",
        CommandResult::new(4, &format!("why=backup_failed\n{TOKEN}\n"), TOKEN),
    );
    let ctx = Ctx::mac().runner(&runner);
    let (fix, log) = ctx.fix(None);
    let outcome = apply(&fix, &plan());
    assert_eq!(
        outcome.message,
        "Didn't change /home/w/.claude/settings.json: backup_failed"
    );
    assert!(!outcome.message.contains(TOKEN));
    let weird = Arc::new(ScriptedRunner::new());
    weird.on_script(
        "notchling-backup",
        CommandResult::new(5, &format!("why={TOKEN}\n"), ""),
    );
    let ctx = Ctx::mac().runner(&weird);
    let (fix, _) = ctx.fix(None);
    let outcome = apply(&fix, &plan());
    assert_eq!(
        outcome.message,
        "Didn't change /home/w/.claude/settings.json: write_failed"
    );
    let _ = log;
    let down = Arc::new(ScriptedRunner::new());
    down.on_script(
        "notchling-backup",
        CommandResult::new(255, "", "ssh: connect"),
    );
    let ctx = Ctx::mac().runner(&down);
    let (fix, _) = ctx.fix(None);
    assert!(apply(&fix, &plan()).message.ends_with("no_connection"));
}

#[test]
fn backup_comes_before_the_rename() {
    let script = write_script("/home/w/.claude/settings.json");
    let hash_check = script.find("changed_since_plan").unwrap();
    let backup = script.find("chmod 600 \"$bak\"").unwrap();
    let rename = script.find("mv -f").unwrap();
    assert!(hash_check < backup, "{script}");
    assert!(backup < rename, "{script}");
    assert!(script.contains("cp \"$f\" \"$bak\" &&"));
    assert!(script.contains(&format!("tail -n +{}", KEEP_BACKUPS + 1)));
    assert!(script.contains("notchling-backup-"));
    assert!(script.contains("notchling-tmp-"));
}

#[test]
fn plan_display_never_holds_a_token() {
    let plan = plan();
    assert!(!plan.shown.contains(TOKEN));
    assert!(!plan.shown.contains("ANTHROPIC"));
    let runner = Arc::new(ScriptedRunner::new());
    runner.on_script("notchling-backup", CommandResult::ok("ok=1\n"));
    let ctx = Ctx::mac().runner(&runner).hiding("claude", TOKEN);
    let (fix, log) = ctx.fix(None);
    let outcome = apply(&fix, &plan);
    assert!(outcome.ok);
    let seen = format!("{}{}{}", plan.shown, outcome.message, log.joined());
    assert!(!seen.contains(TOKEN));
    for call in runner.calls() {
        assert!(!call.argv.join(" ").contains(TOKEN));
    }
}

#[test]
fn script_never_uses_sudo_or_admin() {
    for script in [
        write_script("/home/w/.claude/settings.json"),
        read_script("/home/w/.claude/settings.json", NONCE),
    ] {
        for word in ["sudo", "doas", "su -", "admin", "chown", "rm -rf"] {
            assert!(!script.contains(word), "{word} in {script}");
        }
    }
    let runner = Arc::new(ScriptedRunner::new());
    runner.on_script("notchling-backup", CommandResult::ok("ok=1\n"));
    let ctx = Ctx::mac().runner(&runner);
    let (fix, _) = ctx.fix(None);
    apply(&fix, &plan());
    assert!(runner
        .calls()
        .iter()
        .all(|c| !c.argv.last().unwrap().contains("sudo")));
}

#[test]
fn read_script_only_reads() {
    let script = read_script("/home/w/.claude/settings.json", NONCE);
    for word in ["mv ", "cp ", "tee", "rm ", "chmod", "mkdir", "> \"", ">>"] {
        assert!(!script.contains(word), "{word} in {script}");
    }
    assert!(script.contains("cat \"$f\""));
    assert!(script.contains(&format!("@@begin {NONCE}")));
}

#[test]
fn paths_show_with_a_tilde() {
    assert_eq!(
        friendly("/home/w/.claude/settings.json", Some("/home/w")),
        "~/.claude/settings.json"
    );
    assert_eq!(
        friendly("/home/w/.claude/settings.json", None),
        "/home/w/.claude/settings.json"
    );
    assert_eq!(friendly("/home/wx/a", Some("/home/w")), "/home/wx/a");
}

#[cfg(unix)]
mod local_shell {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};

    fn temp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "notchling-remote-edit-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn run(script: &str, stdin: &[u8]) -> (i32, String) {
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin).unwrap();
        let out = child.wait_with_output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    }

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn backups(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut found: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().contains(".notchling-backup-"))
            .collect();
        found.sort();
        found
    }

    fn input(have: &str, want: &str) -> Vec<u8> {
        format!("{}\n{want}", sha256_hex(have)).into_bytes()
    }

    #[test]
    fn the_script_backs_up_keeps_the_mode_and_renames() {
        let dir = temp("write");
        let file = dir.join("settings.json");
        std::fs::write(&file, "old bytes\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        let script = write_script(file.to_str().unwrap());
        let (code, text) = run(&script, &input("old bytes\n", "new bytes\n"));
        assert_eq!((code, text.trim()), (0, "ok=1"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new bytes\n");
        assert_eq!(mode(&file), 0o640);
        let found = backups(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(std::fs::read_to_string(&found[0]).unwrap(), "old bytes\n");
        assert_eq!(mode(&found[0]), 0o600);
        let leftovers = std::fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("-tmp-")
            })
            .count();
        assert_eq!(leftovers, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_changed_file_is_not_written() {
        let dir = temp("changed");
        let file = dir.join("settings.json");
        std::fs::write(&file, "someone else wrote this\n").unwrap();
        let script = write_script(file.to_str().unwrap());
        let (code, text) = run(&script, &input("old bytes\n", "new bytes\n"));
        assert_eq!(code, 3);
        assert_eq!(text.trim(), "why=changed_since_plan");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "someone else wrote this\n"
        );
        assert!(backups(&dir).is_empty());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_created_private_with_no_backup() {
        let dir = temp("missing");
        let file = dir.join("sub").join("config.toml");
        let script = write_script(file.to_str().unwrap());
        let (code, text) = run(&script, &input("", "a = 1\n"));
        assert_eq!((code, text.trim()), (0, "ok=1"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a = 1\n");
        assert_eq!(mode(&file), 0o600);
        assert!(backups(&dir.join("sub")).is_empty());
        let (code, _) = run(
            &script_for_missing_wrong_hash(&file),
            &input("x", "b = 2\n"),
        );
        assert_eq!(code, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn script_for_missing_wrong_hash(file: &std::path::Path) -> String {
        let other = file.with_file_name("other.toml");
        write_script(other.to_str().unwrap())
    }

    #[test]
    fn only_the_newest_five_backups_are_kept() {
        let dir = temp("rotate");
        let file = dir.join("settings.json");
        std::fs::write(&file, "v0\n").unwrap();
        let script = write_script(file.to_str().unwrap());
        let mut have = "v0\n".to_string();
        for i in 1..=8 {
            let want = format!("v{i}\n");
            let (code, _) = run(&script, &input(&have, &want));
            assert_eq!(code, 0);
            have = want;
        }
        assert_eq!(backups(&dir).len(), KEEP_BACKUPS);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_read_script_round_trips_bytes_and_hash() {
        let dir = temp("read");
        let file = dir.join("settings.json");
        let text = "{\n  \"k\": \"v\"\n}\n\n";
        std::fs::write(&file, text).unwrap();
        let script = read_script(file.to_str().unwrap(), NONCE);
        let (code, got) = run(&script, b"");
        assert_eq!(code, 0);
        assert_eq!(
            parse_read(&out(&got), NONCE),
            Read::Text {
                text: text.into(),
                sha256: sha256_hex(text)
            }
        );
        let (_, none) = run(&read_script(dir.join("nope").to_str().unwrap(), NONCE), b"");
        assert_eq!(parse_read(&out(&none), NONCE), Read::Missing);
        std::fs::write(&file, vec![b'a'; 2_000_000]).unwrap();
        let (_, big) = run(&script, b"");
        assert_eq!(parse_read(&out(&big), NONCE), Read::TooBig);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
