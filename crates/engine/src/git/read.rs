use super::guard::{clean_path, stays_inside};
use super::porcelain::{parse_status, Status};
use super::sandbox::SAFE_ERROR;
use crate::control::{Command, LaunchError, LaunchOutput, Launcher};
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

pub const MAX_DIFF_LINES: usize = 400;
pub const MAX_DIFF_BYTES: usize = 128 * 1024;
pub(super) const MAX_UNTRACKED_BYTES: u64 = 2 * 1024 * 1024;
pub(super) const MAX_LIST_BYTES: usize = 4 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(3);

const ALWAYS_CLEARED: [&str; 12] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_CONFIG",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_EXTERNAL_DIFF",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    NotRepo,
    Unsupported,
    Refused(String),
    Failed(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Diff {
    pub path: String,
    pub text: String,
    pub truncated: bool,
    pub untracked: bool,
}

pub(super) fn command(cwd: &str, drivers: &[String], args: &[&str]) -> Command {
    let mut argv: Vec<String> = [
        "--no-optional-locks",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "diff.external=",
    ]
    .iter()
    .map(|a| (*a).to_owned())
    .collect();
    for name in drivers {
        for key in ["clean", "smudge", "process"] {
            argv.push("-c".to_owned());
            argv.push(format!("filter.{name}.{key}="));
        }
        argv.push("-c".to_owned());
        argv.push(format!("filter.{name}.required=false"));
    }
    argv.push("-C".to_owned());
    argv.push(cwd.to_owned());
    argv.extend(args.iter().map(|a| (*a).to_owned()));
    let mut command = Command::from_args("git", &argv);
    for (key, _) in std::env::vars_os() {
        if let Some(key) = key.to_str().filter(|k| k.starts_with("GIT_")) {
            command = command.with_env(key, None);
        }
    }
    for key in ALWAYS_CLEARED {
        command = command.with_env(key, None);
    }
    command
        .with_env("GIT_TERMINAL_PROMPT", Some("0"))
        .with_env("GIT_PAGER", Some("cat"))
        .with_env("LC_ALL", Some("C"))
}

fn launch_failure(error: LaunchError) -> Failure {
    match error {
        LaunchError::Unsupported => Failure::Unsupported,
        LaunchError::Spawn(why) if why.starts_with(SAFE_ERROR) => Failure::Failed(why),
        other => Failure::Failed(other.to_string()),
    }
}

pub(super) fn run(launcher: &dyn Launcher, command: &Command) -> Result<LaunchOutput, Failure> {
    launcher.run(command, TIMEOUT).map_err(launch_failure)
}

pub(super) fn run_capped(
    launcher: &dyn Launcher,
    command: &Command,
    max_bytes: usize,
) -> Result<(LaunchOutput, bool), Failure> {
    launcher
        .run_capped(command, TIMEOUT, max_bytes)
        .map_err(launch_failure)
}

fn driver_names(config: &str) -> Vec<String> {
    let mut names: Vec<String> = config
        .split('\0')
        .filter_map(|entry| {
            let key = entry.split('\n').next().unwrap_or("");
            let rest = key.strip_prefix("filter.")?;
            let (name, _) = rest.rsplit_once('.')?;
            Some(name.to_owned())
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

pub(super) fn drivers(cwd: &str, launcher: &dyn Launcher) -> Result<Vec<String>, Failure> {
    let out = run(
        launcher,
        &command(cwd, &[], &["config", "-z", "--get-regexp", "^filter\\."]),
    )?;
    if out.code != 0 && out.code != 1 {
        return Err(Failure::Failed(format!("git config exited {}", out.code)));
    }
    let names = driver_names(&out.output);
    if names.iter().any(|n| n.contains('=') || n.contains('\n')) {
        return Err(Failure::Refused(
            "a filter name git can't be told to skip".to_owned(),
        ));
    }
    Ok(names)
}

pub(super) fn refusal(launcher: &dyn Launcher, cwd: &str, out: &LaunchOutput) -> Failure {
    if out.code == 128 {
        let probe = command(cwd, &[], &["rev-parse", "--is-inside-work-tree"]);
        if let Ok(inside) = run(launcher, &probe) {
            if inside.code == 128 || inside.output.trim() == "false" {
                return Failure::NotRepo;
            }
        }
    }
    let line = out.stderr.lines().next().unwrap_or("").trim();
    Failure::Failed(format!("git exited {}: {line}", out.code))
}

pub fn cap(text: &str, max_lines: usize, max_bytes: usize) -> (String, bool) {
    let mut end = 0;
    for (lines, line) in text.split_inclusive('\n').enumerate() {
        if lines == max_lines {
            return (text[..end].to_owned(), true);
        }
        if end + line.len() > max_bytes {
            let mut cut = if end > 0 { end } else { max_bytes };
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            return (text[..cut].to_owned(), true);
        }
        end += line.len();
    }
    (text.to_owned(), false)
}

pub(super) fn whole_records(text: &str, cut: bool) -> &str {
    if !cut {
        return text;
    }
    match text.rfind('\0') {
        Some(at) => &text[..=at],
        None => "",
    }
}

pub(super) const STATUS_ARGS: [&str; 6] = [
    "status",
    "--porcelain=v2",
    "--branch",
    "-z",
    "--untracked-files=all",
    "--ignore-submodules=all",
];

pub fn read_status(cwd: &str, launcher: &dyn Launcher) -> Result<Status, Failure> {
    let drivers = drivers(cwd, launcher)?;
    let (out, cut) = run_capped(
        launcher,
        &command(cwd, &drivers, &STATUS_ARGS),
        MAX_LIST_BYTES,
    )?;
    if out.code != 0 {
        return Err(refusal(launcher, cwd, &out));
    }
    let mut status = parse_status(whole_records(&out.output, cut));
    status.truncated |= cut;
    if !status.changes.is_empty() {
        let base = if status.initial { "--cached" } else { "HEAD" };
        let numstat = command(
            cwd,
            &drivers,
            &[
                "diff",
                "--numstat",
                "-z",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--ignore-submodules=all",
                base,
                "--",
            ],
        );
        if let Ok((counts, cut)) = run_capped(launcher, &numstat, MAX_LIST_BYTES) {
            if counts.code == 0 {
                status.apply_numstat(whole_records(&counts.output, cut));
            }
        }
    }
    Ok(status)
}

pub fn read_diff(cwd: &str, path: &str, launcher: &dyn Launcher) -> Result<Diff, Failure> {
    let path = clean_path(path).map_err(|why| Failure::Refused(why.to_owned()))?;
    let drivers = drivers(cwd, launcher)?;
    let top = run(
        launcher,
        &command(cwd, &drivers, &["rev-parse", "--show-toplevel"]),
    )?;
    if top.code != 0 {
        return Err(refusal(launcher, cwd, &top));
    }
    let root = top.output.trim_end_matches(['\n', '\r']).to_owned();
    if root.is_empty() {
        return Err(Failure::Failed("no repository root".to_owned()));
    }
    if !stays_inside(Path::new(&root), &path) {
        return Err(Failure::Refused("path leaves the repository".to_owned()));
    }
    let spec = format!(":(literal){path}");
    let mut one_args = STATUS_ARGS.to_vec();
    one_args.push("--");
    one_args.push(&spec);
    let one = run(launcher, &command(&root, &drivers, &one_args))?;
    if one.code != 0 {
        return Err(refusal(launcher, &root, &one));
    }
    let status = parse_status(&one.output);
    let Some(entry) = status.changes.iter().find(|c| c.path == path) else {
        return Err(Failure::Refused("not a changed file".to_owned()));
    };
    if entry.status == "?" {
        return untracked_diff(launcher, &root, &drivers, &path);
    }
    let base = if status.initial { "--cached" } else { "HEAD" };
    let (out, cut) = run_capped(
        launcher,
        &command(
            &root,
            &drivers,
            &[
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--ignore-submodules=all",
                base,
                "--",
                &spec,
            ],
        ),
        MAX_DIFF_BYTES + 1,
    )?;
    if out.code != 0 {
        return Err(refusal(launcher, &root, &out));
    }
    Ok(patch(path, &out.output, cut, false))
}

pub(super) fn patch(path: String, output: &str, cut: bool, untracked: bool) -> Diff {
    let (text, truncated) = cap(output, MAX_DIFF_LINES, MAX_DIFF_BYTES);
    Diff {
        path,
        text,
        truncated: truncated || cut,
        untracked,
    }
}

pub(super) fn untracked_diff(
    launcher: &dyn Launcher,
    root: &str,
    drivers: &[String],
    path: &str,
) -> Result<Diff, Failure> {
    let too_big = std::fs::symlink_metadata(Path::new(root).join(path))
        .map(|m| m.len() > MAX_UNTRACKED_BYTES)
        .unwrap_or(false);
    if too_big {
        return Ok(Diff {
            path: path.to_owned(),
            truncated: true,
            untracked: true,
            ..Diff::default()
        });
    }
    let (out, cut) = run_capped(
        launcher,
        &command(
            root,
            drivers,
            &[
                "diff",
                "--no-index",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--",
                "/dev/null",
                path,
            ],
        ),
        MAX_DIFF_BYTES + 1,
    )?;
    if out.code != 0 && out.code != 1 {
        return Err(refusal(launcher, root, &out));
    }
    Ok(patch(path.to_owned(), &out.output, cut, true))
}
