use std::path::Path;

use super::shell_job::{scrub, shell_plan, ShellPlan, CODEX_SECRETS, ENV_PREFIXES, MAX_COMMAND};

fn plan(job: &str, cwd: &str, command: &str) -> Result<ShellPlan, String> {
    shell_plan(job, cwd, command, Path::new("/data"))
}

#[test]
fn shell_plan_scrubs_env_and_needs_absolute_cwd() {
    let got = plan("j-0123456789ab", "/srv/repo", "printf hi").expect("plan");
    assert_eq!(got.argv, vec!["/bin/sh", "-c", "printf hi"]);
    assert_eq!(got.env_remove, vec!["NOTCHLING_", "CLAUDE_CODE_"]);
    assert_eq!(got.log, Path::new("/data/cluster/jobs/j-0123456789ab.log"));
    assert_eq!(
        plan("j-0123456789ab", "srv/repo", "true").unwrap_err(),
        "the folder must be an absolute path"
    );
    assert_eq!(
        plan("j-0123456789ab", "", "true").unwrap_err(),
        "the folder must be an absolute path"
    );
}

#[test]
fn shell_plan_refuses_a_bad_job_name_or_command() {
    for job in ["", "../x", "a/b", "j 1", "j\n1", &"j".repeat(65)] {
        assert_eq!(
            plan(job, "/srv", "true").unwrap_err(),
            "not a job name",
            "{job:?}"
        );
    }
    assert_eq!(plan("j-1", "/srv", "  ").unwrap_err(), "no command");
    assert_eq!(
        plan("j-1", "/srv", "a\0b").unwrap_err(),
        "the command has a NUL byte"
    );
    let long = "x".repeat(MAX_COMMAND + 1);
    assert_eq!(
        plan("j-1", "/srv", &long).unwrap_err(),
        "the command is over 64 KB"
    );
    assert_eq!(
        plan("j-1", "/srv\0x", "true").unwrap_err(),
        "the folder has a NUL byte"
    );
}

#[test]
fn scrub_names_every_variable_with_a_listed_prefix() {
    let names = [
        "PATH",
        "NOTCHLING_DATA_ROOT",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDECODE",
        "HOME",
        "NOTCHLING_",
    ]
    .map(String::from);
    let prefixes: Vec<String> = ENV_PREFIXES.iter().map(|p| p.to_string()).collect();
    assert_eq!(
        scrub(names, &prefixes),
        vec![
            "NOTCHLING_DATA_ROOT",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "NOTCHLING_"
        ]
    );
}

#[test]
fn the_codex_secrets_refusal_says_what_is_refused() {
    assert_eq!(
        CODEX_SECRETS,
        "Codex jobs on another machine take no secrets"
    );
}
