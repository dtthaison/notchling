use super::guard::{clean_path, stays_inside};
use super::porcelain::{parse_status, Status};
use crate::control::{Command, LaunchOutput, Launcher};
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

pub const MAX_DIFF_LINES: usize = 400;
pub const MAX_DIFF_BYTES: usize = 128 * 1024;
const MAX_UNTRACKED_BYTES: u64 = 2 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    NotRepo,
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

fn command(cwd: &str, args: &[&str]) -> Command {
    let mut argv = vec![
        "git",
        "--no-optional-locks",
        "-c",
        "core.fsmonitor=false",
        "-C",
        cwd,
    ];
    argv.extend(args);
    Command::new(&argv)
        .with_env("GIT_TERMINAL_PROMPT", Some("0"))
        .with_env("GIT_PAGER", Some("cat"))
}

fn run(launcher: &dyn Launcher, command: &Command) -> Result<LaunchOutput, Failure> {
    launcher
        .run(command, TIMEOUT)
        .map_err(|e| Failure::Failed(e.to_string()))
}

fn refusal(out: &LaunchOutput) -> Failure {
    let text = out.combined();
    if out.code == 128 && text.contains("not a git repository") {
        return Failure::NotRepo;
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

pub fn read_status(cwd: &str, launcher: &dyn Launcher) -> Result<Status, Failure> {
    let out = run(
        launcher,
        &command(
            cwd,
            &[
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=all",
            ],
        ),
    )?;
    if out.code != 0 {
        return Err(refusal(&out));
    }
    let mut status = parse_status(&out.output);
    if !status.changes.is_empty() {
        let base = if status.initial { "--cached" } else { "HEAD" };
        let numstat = command(
            cwd,
            &[
                "diff",
                "--numstat",
                "-z",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                base,
                "--",
            ],
        );
        if let Ok(counts) = run(launcher, &numstat) {
            if counts.code == 0 {
                status.apply_numstat(&counts.output);
            }
        }
    }
    Ok(status)
}

pub fn read_diff(cwd: &str, path: &str, launcher: &dyn Launcher) -> Result<Diff, Failure> {
    let path = clean_path(path).map_err(|why| Failure::Refused(why.to_owned()))?;
    let top = run(launcher, &command(cwd, &["rev-parse", "--show-toplevel"]))?;
    if top.code != 0 {
        return Err(refusal(&top));
    }
    let root = top.output.trim_end_matches(['\n', '\r']).to_owned();
    if root.is_empty() {
        return Err(Failure::Failed("no repository root".to_owned()));
    }
    if !stays_inside(Path::new(&root), &path) {
        return Err(Failure::Refused("path leaves the repository".to_owned()));
    }
    let spec = format!(":(literal){path}");
    let one = run(
        launcher,
        &command(
            &root,
            &[
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=all",
                "--",
                &spec,
            ],
        ),
    )?;
    if one.code != 0 {
        return Err(refusal(&one));
    }
    let status = parse_status(&one.output);
    let untracked = status.changes.first().is_some_and(|c| c.status == "?");
    let mut diff = Diff {
        path: path.clone(),
        untracked,
        ..Diff::default()
    };
    let out = if untracked {
        let too_big = std::fs::symlink_metadata(Path::new(&root).join(&path))
            .map(|m| m.len() > MAX_UNTRACKED_BYTES)
            .unwrap_or(false);
        if too_big {
            diff.truncated = true;
            return Ok(diff);
        }
        run(
            launcher,
            &command(
                &root,
                &[
                    "diff",
                    "--no-index",
                    "--no-color",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--",
                    "/dev/null",
                    &path,
                ],
            ),
        )?
    } else {
        let base = if status.initial { "--cached" } else { "HEAD" };
        run(
            launcher,
            &command(
                &root,
                &[
                    "diff",
                    "--no-color",
                    "--no-ext-diff",
                    "--no-textconv",
                    base,
                    "--",
                    &spec,
                ],
            ),
        )?
    };
    let ok = out.code == 0 || (untracked && out.code == 1);
    if !ok {
        return Err(refusal(&out));
    }
    let (text, truncated) = cap(&out.output, MAX_DIFF_LINES, MAX_DIFF_BYTES);
    diff.text = text;
    diff.truncated = truncated;
    Ok(diff)
}
