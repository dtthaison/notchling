use super::fakes_b::*;
use super::repo_common::*;
use super::tests_repo_replace::*;

fn script_of(rig: &Rig, needle: &str) -> String {
    let argv = rig
        .argvs()
        .into_iter()
        .find(|a| a[0] == "/usr/bin/ssh" && unquoted(last(a)).contains(needle))
        .unwrap_or_else(|| panic!("no ssh call with {needle}"));
    let mut text = unquoted(last(&argv));
    let start = "/bin/sh -c '".len() + super::shell::prelude().len();
    text.drain(..start);
    text.pop();
    text
}

fn stub_bin(dir: &std::path::Path, tools: &[(&str, &str)]) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    for (name, body) in tools {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn sh(script: &str, bin: &std::path::Path, home: &std::path::Path) -> (i32, String) {
    let out = std::process::Command::new("/bin/sh")
        .args(["-c", script])
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", home)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().unwrap_or(-1), text)
}

#[test]
fn the_scripts_run_in_a_real_shell_on_a_throwaway_home() {
    let dir = TempDir::new("replace");
    let home = dir.path().join("home");
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    stub_bin(&bin, &[("git", "exit 255"), ("ssh", "exit 255")]);
    let key = home.join(".ssh/agents-dtthaison-lingeva-mono");
    let key_pub = format!("{}.pub", key.display());
    std::fs::write(&key, "OLD-PRIVATE").unwrap();
    std::fs::write(&key_pub, pub_line()).unwrap();

    let rig = replace_rig(Opts::default());
    go(&rig);
    for needle in [
        "head -c 1024",
        "ssh-keygen -q",
        "-T git@github.com",
        "push --dry-run",
        "mv -f",
    ] {
        let status = std::process::Command::new("/bin/sh")
            .args(["-n", "-c", &script_of(&rig, needle)])
            .status()
            .unwrap();
        assert!(status.success(), "syntax: {needle}");
    }
    let (code, out) = sh(&script_of(&rig, "head -c 1024"), &bin, &home);
    assert_eq!(code, 0);
    assert_eq!(out, pub_line());

    let (code, out) = sh(&script_of(&rig, "ssh-keygen -q"), &bin, &home);
    assert_eq!(code, 0, "{out}");
    assert!(single_ed25519_line(&out).is_some(), "{out}");
    assert!(std::path::Path::new(&format!("{}.new", key.display())).exists());
    assert_eq!(
        std::fs::read_to_string(&key).unwrap(),
        "OLD-PRIVATE",
        "the old key is untouched until the swap"
    );
    let new_pub_text = std::fs::read_to_string(format!("{}.new.pub", key.display())).unwrap();

    let (code, out) = sh(&script_of(&rig, "mv -f"), &bin, &home);
    assert_eq!(code, 0, "{out}");
    assert_eq!(std::fs::read_to_string(&key_pub).unwrap(), new_pub_text);
    assert_ne!(std::fs::read_to_string(&key).unwrap(), "OLD-PRIVATE");
    assert!(!std::path::Path::new(&format!("{}.new", key.display())).exists());
    assert!(!std::path::Path::new(&format!("{}.new.pub", key.display())).exists());

    let (again, _) = sh(&script_of(&rig, "mv -f"), &bin, &home);
    assert_eq!(again, 43);
    assert_eq!(std::fs::read_to_string(&key_pub).unwrap(), new_pub_text);
}

#[test]
fn the_push_check_runs_a_throwaway_repo_through_the_new_key_only() {
    let dir = TempDir::new("pushcheck");
    let bin = dir.path().join("bin");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let log = dir.path().join("git.log");
    let git = format!(
        "echo \"GIT_SSH_COMMAND=$GIT_SSH_COMMAND :: $*\" >> '{}'\ncase \"$*\" in *push*) echo 'ERROR: The key you are authenticating with has been marked as read only.' >&2; exit 128;; esac\nexit 0",
        log.display()
    );
    stub_bin(&bin, &[("git", &git), ("ssh", "exit 255")]);
    let rig = replace_rig(Opts::default());
    go(&rig);
    let script = script_of(&rig, "push --dry-run");
    assert!(!script.contains("export PATH"));
    let (_, out) = sh(&script, &bin, &home);
    assert!(out.contains("marked as read only"), "{out}");
    assert_eq!(
        out.split('\n').rfind(|l| l.starts_with("@@push ")),
        Some("@@push 128")
    );
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(
        calls.contains(&format!(
            "push --dry-run git@github.com:{REPO}.git HEAD:refs/heads/agents-push-test"
        )),
        "{calls}"
    );
    assert!(
        calls.contains(&format!(
            "-F /dev/null -i '{}/.ssh/agents-dtthaison-lingeva-mono.new' -o IdentitiesOnly=yes",
            home.display()
        )),
        "{calls}"
    );
    assert!(calls.contains("-o StrictHostKeyChecking=yes"), "{calls}");
    for line in calls.lines() {
        assert!(line.contains("core.hooksPath=/dev/null"), "{line}");
    }
    assert!(
        !calls.contains("github-agents-"),
        "the alias would lead to the old key"
    );
}

#[test]
fn the_swap_ends_on_the_same_files_the_probe_and_fix_use() {
    let rig = replace_rig(Opts::default());
    go(&rig);
    let swap = rig
        .argvs()
        .into_iter()
        .map(|a| unquoted(last(&a)))
        .find(|s| s.contains("mv -f"))
        .unwrap();
    assert!(
        swap.contains("\"$HOME\"/'.ssh/agents-dtthaison-lingeva-mono'"),
        "{swap}"
    );
    assert!(
        swap.contains("\"$HOME\"/'.ssh/agents-dtthaison-lingeva-mono.pub'"),
        "{swap}"
    );
}
