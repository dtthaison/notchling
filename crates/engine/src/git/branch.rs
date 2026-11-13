use super::branch_parse::{assemble, finish, parse_branches, BranchStat, Branches, MAX_BRANCHES};
use super::guard::clean_ref;
use super::porcelain::{parse_status, Status};
use super::read::{
    command, drivers, refusal, run, run_capped, whole_records, Failure, MAX_LIST_BYTES,
    MAX_UNTRACKED_BYTES, STATUS_ARGS,
};
use crate::control::Launcher;
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

const COUNT_BUDGET: u64 = 8 * 1024 * 1024;
const FALLBACK_BASES: [&str; 2] = ["main", "master"];

const DIFF_FLAGS: [&str; 5] = [
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--ignore-submodules=all",
    "-M",
];

pub(super) fn base_failure(why: &str) -> Failure {
    Failure::Failed(why.to_owned())
}

pub(super) fn merge_base(
    launcher: &dyn Launcher,
    cwd: &str,
    drivers: &[String],
    base: &str,
) -> Result<Option<String>, Failure> {
    let out = run(
        launcher,
        &command(cwd, drivers, &["merge-base", base, "HEAD"]),
    )?;
    match out.code {
        0 => {
            let sha = out.output.trim();
            Ok((!sha.is_empty()).then(|| sha.to_owned()))
        }
        1 => Err(base_failure("no common history with that branch")),
        _ => Ok(None),
    }
}

fn remote_default(launcher: &dyn Launcher, cwd: &str, drivers: &[String]) -> Option<String> {
    let out = run(
        launcher,
        &command(
            cwd,
            drivers,
            &[
                "symbolic-ref",
                "--quiet",
                "--short",
                "refs/remotes/origin/HEAD",
            ],
        ),
    )
    .ok()?;
    let name = out.output.trim();
    (out.code == 0 && clean_ref(name).is_ok()).then(|| name.to_owned())
}

pub(super) fn resolve_base(
    launcher: &dyn Launcher,
    cwd: &str,
    drivers: &[String],
    requested: Option<&str>,
) -> Result<Option<(String, String)>, Failure> {
    if let Some(base) = requested {
        let base = clean_ref(base).map_err(|why| Failure::Refused(why.to_owned()))?;
        return match merge_base(launcher, cwd, drivers, base)? {
            Some(sha) => Ok(Some((base.to_owned(), sha))),
            None => Err(base_failure("no such branch")),
        };
    }
    let mut candidates: Vec<String> = Vec::new();
    candidates.extend(remote_default(launcher, cwd, drivers));
    candidates.extend(FALLBACK_BASES.iter().map(|b| (*b).to_owned()));
    for candidate in candidates {
        if let Ok(Some(sha)) = merge_base(launcher, cwd, drivers, &candidate) {
            return Ok(Some((candidate, sha)));
        }
    }
    Ok(None)
}

fn count_lines(root: &Path, path: &str, spent: &mut u64) -> Option<u64> {
    let full = root.join(path);
    let meta = std::fs::symlink_metadata(&full).ok()?;
    if !meta.is_file() || meta.len() > MAX_UNTRACKED_BYTES || *spent + meta.len() > COUNT_BUDGET {
        return None;
    }
    *spent += meta.len();
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    std::fs::File::open(&full)
        .ok()?
        .take(MAX_UNTRACKED_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.contains(&0) {
        return None;
    }
    let newlines = bytes.iter().filter(|b| **b == b'\n').count() as u64;
    let open_line = u64::from(bytes.last().is_some_and(|b| *b != b'\n'));
    Some(newlines + open_line)
}

fn read_worktree(
    cwd: &str,
    drivers: &[String],
    launcher: &dyn Launcher,
) -> Result<Status, Failure> {
    let (out, cut) = run_capped(
        launcher,
        &command(cwd, drivers, &STATUS_ARGS),
        MAX_LIST_BYTES,
    )?;
    if out.code != 0 {
        return Err(refusal(launcher, cwd, &out));
    }
    let mut status = parse_status(whole_records(&out.output, cut));
    status.truncated |= cut;
    Ok(status)
}

fn top_level(cwd: &str, drivers: &[String], launcher: &dyn Launcher) -> Result<String, Failure> {
    let top = run(
        launcher,
        &command(cwd, drivers, &["rev-parse", "--show-toplevel"]),
    )?;
    if top.code != 0 {
        return Err(refusal(launcher, cwd, &top));
    }
    Ok(top.output.trim_end_matches(['\n', '\r']).to_owned())
}

fn diff_list(
    launcher: &dyn Launcher,
    cwd: &str,
    drivers: &[String],
    mode: &str,
    merge_base: &str,
) -> Result<String, Failure> {
    let mut args = vec!["diff", mode, "-z"];
    args.extend(DIFF_FLAGS);
    args.push(merge_base);
    args.push("--");
    let (out, cut) = run_capped(launcher, &command(cwd, drivers, &args), MAX_LIST_BYTES)?;
    if out.code != 0 {
        return Err(refusal(launcher, cwd, &out));
    }
    Ok(whole_records(&out.output, cut).to_owned())
}

pub fn read_branch_stat(
    cwd: &str,
    base: Option<&str>,
    launcher: &dyn Launcher,
) -> Result<BranchStat, Failure> {
    if let Some(base) = base {
        clean_ref(base).map_err(|why| Failure::Refused(why.to_owned()))?;
    }
    let drivers = drivers(cwd, launcher)?;
    let status = read_worktree(cwd, &drivers, launcher)?;
    let mut stat = BranchStat {
        branch: status.branch.clone(),
        head: status.head.clone(),
        detached: status.detached,
        truncated: status.truncated,
        ..BranchStat::default()
    };
    if status.initial {
        return Ok(stat);
    }
    let Some((base, sha)) = resolve_base(launcher, cwd, &drivers, base)? else {
        return Ok(stat);
    };
    let names = diff_list(launcher, cwd, &drivers, "--name-status", &sha)?;
    let counts = diff_list(launcher, cwd, &drivers, "--numstat", &sha)?;
    let uncommitted: HashSet<String> = status
        .changes
        .iter()
        .filter(|c| c.status != "?")
        .map(|c| c.path.clone())
        .collect();
    let new_paths: Vec<&str> = status
        .changes
        .iter()
        .filter(|c| c.status == "?")
        .map(|c| c.path.as_str())
        .collect();
    let mut untracked: Vec<(String, Option<u64>)> = Vec::new();
    if !new_paths.is_empty() {
        let root = top_level(cwd, &drivers, launcher)?;
        let mut spent = 0;
        for path in new_paths {
            let lines = count_lines(Path::new(&root), path, &mut spent);
            untracked.push((path.to_owned(), lines));
        }
    }
    let (files, cut) = assemble(&names, &counts, &uncommitted, untracked);
    stat.base = Some(base);
    stat.merge_base = Some(sha.chars().take(7).collect());
    finish(&mut stat, files, cut);
    Ok(stat)
}

pub fn read_branches(cwd: &str, launcher: &dyn Launcher) -> Result<Branches, Failure> {
    let drivers = drivers(cwd, launcher)?;
    let listed = run(
        launcher,
        &command(
            cwd,
            &drivers,
            &[
                "for-each-ref",
                "--sort=-committerdate",
                &format!("--count={}", MAX_BRANCHES + 1),
                "--format=%(HEAD) %(refname:short)",
                "refs/heads",
            ],
        ),
    )?;
    if listed.code != 0 {
        return Err(refusal(launcher, cwd, &listed));
    }
    let (current, local, truncated) = parse_branches(&listed.output);
    let remote_default = remote_default(launcher, cwd, &drivers).filter(|name| {
        run(
            launcher,
            &command(
                cwd,
                &drivers,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("{name}^{{commit}}"),
                ],
            ),
        )
        .is_ok_and(|out| out.code == 0)
    });
    let default = remote_default.clone().or_else(|| {
        FALLBACK_BASES
            .iter()
            .find(|name| local.iter().any(|l| l == **name))
            .map(|name| (*name).to_owned())
    });
    Ok(Branches {
        current,
        default,
        remote_default,
        local,
        truncated,
    })
}
