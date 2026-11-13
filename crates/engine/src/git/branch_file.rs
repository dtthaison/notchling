use super::branch::resolve_base;
use super::guard::{clean_path, clean_ref, stays_inside};
use super::porcelain::parse_status;
use super::read::{
    command, drivers, patch, refusal, run, run_capped, untracked_diff, Diff, Failure,
    MAX_DIFF_BYTES, STATUS_ARGS,
};
use crate::control::Launcher;
use std::path::Path;

pub fn read_branch_diff(
    cwd: &str,
    base: &str,
    path: &str,
    from: Option<&str>,
    launcher: &dyn Launcher,
) -> Result<Diff, Failure> {
    let path = clean_path(path).map_err(|why| Failure::Refused(why.to_owned()))?;
    let from = from
        .map(|f| clean_path(f).map_err(|why| Failure::Refused(why.to_owned())))
        .transpose()?;
    clean_ref(base).map_err(|why| Failure::Refused(why.to_owned()))?;
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
    for name in std::iter::once(&path).chain(from.iter()) {
        if !stays_inside(Path::new(&root), name) {
            return Err(Failure::Refused("path leaves the repository".to_owned()));
        }
    }
    let Some((_, sha)) = resolve_base(launcher, &root, &drivers, Some(base))? else {
        return Err(Failure::Failed("no such branch".to_owned()));
    };
    let spec = format!(":(literal){path}");
    let from_spec = from.as_ref().map(|f| format!(":(literal){f}"));
    let mut args = vec![
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--ignore-submodules=all",
        "-M",
        &sha,
        "--",
        &spec,
    ];
    if let Some(old) = &from_spec {
        args.push(old);
    }
    let (out, cut) = run_capped(
        launcher,
        &command(&root, &drivers, &args),
        MAX_DIFF_BYTES + 1,
    )?;
    if out.code != 0 {
        return Err(refusal(launcher, &root, &out));
    }
    if !out.output.is_empty() {
        return Ok(patch(path, &out.output, cut, false));
    }
    let mut one_args = STATUS_ARGS.to_vec();
    one_args.push("--");
    one_args.push(&spec);
    let one = run(launcher, &command(&root, &drivers, &one_args))?;
    if one.code != 0 {
        return Err(refusal(launcher, &root, &one));
    }
    let status = parse_status(&one.output);
    match status.changes.iter().find(|c| c.path == path) {
        Some(entry) if entry.status == "?" => untracked_diff(launcher, &root, &drivers, &path),
        _ => Err(Failure::Refused("not a changed file".to_owned())),
    }
}
