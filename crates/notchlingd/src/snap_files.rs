use crate::snap_copy::{copy_quiet, copy_tail, hash_file, Copied};
use crate::snap_db::backup_db;
use notchling_engine::cluster::id::NodeId;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::snapshot::{
    dir_digest, excluded, staged_path, Manifest, Part, PartKind, SetPlan, EXCLUDED_DIRS, MAX_FILE,
};
use std::fs;
use std::path::{Component, Path, PathBuf};

const STAGING: &str = ".staging";

#[derive(Clone, Debug)]
pub struct StageMeta {
    pub seq: u64,
    pub term: Term,
    pub home: NodeId,
    pub created_ms: i64,
    pub quiet_ms: i64,
    pub now_ms: i64,
}

fn private_dirs(path: &Path) -> Result<(), String> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn variants(path: &Path) -> Vec<PathBuf> {
    let mut all = vec![path.to_path_buf()];
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        if let Ok(real) = fs::canonicalize(parent) {
            let joined = real.join(name);
            if joined != path {
                all.push(joined);
            }
        }
    }
    all
}

fn root_variants(root: &Path) -> Vec<PathBuf> {
    let mut all = vec![root.to_path_buf()];
    if let Ok(real) = fs::canonicalize(root) {
        if real != root {
            all.push(real);
        }
    }
    all
}

fn excluded_any(path: &Path) -> bool {
    variants(path).iter().any(|each| excluded(each))
}

fn in_excluded_dir(path: &Path) -> bool {
    let parts: Vec<Component> = path.components().collect();
    if parts
        .iter()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return true;
    }
    EXCLUDED_DIRS.iter().any(|dir| {
        let want: Vec<Component> = Path::new(dir).components().collect();
        parts.windows(want.len()).any(|window| window == want)
    })
}

fn tail_excluded(path: &Path) -> bool {
    variants(path).iter().any(|each| in_excluded_dir(each))
}

fn valid_set_id(id: &str) -> Result<(), String> {
    let mut parts = Path::new(id).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(name)), None) if name == id && !id.ends_with(STAGING) => Ok(()),
        _ => Err(format!("{id:?} is not a set id")),
    }
}

fn refuse_unsafe_paths(plan: &SetPlan, stage: &Path) -> Result<(), String> {
    let roots: Vec<PathBuf> = plan
        .sessions
        .iter()
        .filter_map(|s| s.repo.as_ref())
        .flat_map(|repo| root_variants(&repo.root))
        .collect();
    let stages = root_variants(stage);
    let paths = plan
        .dbs
        .iter()
        .chain(plan.files.iter())
        .chain(plan.tails.iter().map(|(path, _)| path));
    for path in paths {
        for each in variants(path) {
            if let Some(root) = roots
                .iter()
                .find(|root| each.starts_with(root) || root.starts_with(&each))
            {
                return Err(format!(
                    "{} is inside the repository {}: code never travels",
                    path.display(),
                    root.display()
                ));
            }
            if stages
                .iter()
                .any(|held| each.starts_with(held) || held.starts_with(&each))
            {
                return Err(format!(
                    "{} is inside or around the staging folder",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

enum Source {
    Missing,
    Link,
    File,
    Dir,
}

fn classify(path: &Path) -> Result<Source, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Ok(Source::Link),
        Ok(meta) if meta.is_dir() => Ok(Source::Dir),
        Ok(meta) if meta.is_file() => Ok(Source::File),
        Ok(_) => Ok(Source::Link),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Source::Missing),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn prepare_target(set_dir: &Path, source: &Path) -> Result<PathBuf, String> {
    let target = staged_path(set_dir, source)
        .ok_or_else(|| format!("{} can't be staged", source.display()))?;
    if let Some(parent) = target.parent() {
        private_dirs(parent)?;
    }
    Ok(target)
}

pub(crate) fn copy_listed(
    src: &Path,
    dst: &Path,
    meta: &StageMeta,
) -> Result<Option<(u64, String)>, String> {
    match copy_quiet(src, dst, meta.quiet_ms, meta.now_ms) {
        Ok(Copied::Done { len, sha256 }) => Ok(Some((len, sha256))),
        Ok(_) => Err(format!("{} is still being written", src.display())),
        Err(why) => match fs::symlink_metadata(src) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            _ => Err(why),
        },
    }
}

fn db_part(set_dir: &Path, src: &Path) -> Result<Option<Part>, String> {
    if excluded_any(src) {
        return Ok(None);
    }
    match classify(src)? {
        Source::Missing => return Err(format!("database {} is missing", src.display())),
        Source::File => {}
        _ => return Ok(None),
    }
    let dst = prepare_target(set_dir, src)?;
    backup_db(src, &dst).map_err(|why| format!("{}: {why}", src.display()))?;
    let (len, sha256) = hash_file(&dst)?;
    Ok(Some(Part {
        kind: PartKind::Db,
        path: src.to_path_buf(),
        sha256,
        len,
    }))
}

fn file_part(set_dir: &Path, src: &Path, meta: &StageMeta) -> Result<Option<Part>, String> {
    if excluded_any(src) {
        return Ok(None);
    }
    match classify(src)? {
        Source::Missing | Source::Link => Ok(None),
        Source::Dir => dir_part(set_dir, src, meta),
        Source::File => {
            let dst = prepare_target(set_dir, src)?;
            Ok(copy_listed(src, &dst, meta)?.map(|(len, sha256)| Part {
                kind: PartKind::File,
                path: src.to_path_buf(),
                sha256,
                len,
            }))
        }
    }
}

fn walk_files(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = entry.path();
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if name.contains('\n') {
            continue;
        }
        let rel_name = if rel.is_empty() {
            name.to_owned()
        } else {
            format!("{rel}/{name}")
        };
        if excluded_any(&path) {
            continue;
        }
        match classify(&path)? {
            Source::Dir => walk_files(&path, &rel_name, out)?,
            Source::File => out.push((rel_name, path)),
            Source::Missing | Source::Link => {}
        }
    }
    Ok(())
}

fn dir_part(set_dir: &Path, src: &Path, meta: &StageMeta) -> Result<Option<Part>, String> {
    let root = prepare_target(set_dir, src)?;
    private_dirs(&root)?;
    let mut files = Vec::new();
    walk_files(src, "", &mut files)?;
    files.sort();
    let mut entries = Vec::with_capacity(files.len());
    let mut total = 0u64;
    for (rel, path) in files {
        let dst = root.join(&rel);
        if let Some(parent) = dst.parent() {
            private_dirs(parent)?;
        }
        let Some((len, sha256)) = copy_listed(&path, &dst, meta)? else {
            continue;
        };
        total += len;
        entries.push((rel, len, sha256));
    }
    Ok(Some(Part {
        kind: PartKind::Dir,
        path: src.to_path_buf(),
        sha256: dir_digest(&entries),
        len: total,
    }))
}

fn tail_part(
    set_dir: &Path,
    src: &Path,
    from: u64,
    meta: &StageMeta,
) -> Result<Option<Part>, String> {
    if tail_excluded(src) {
        return Ok(None);
    }
    match classify(src)? {
        Source::File => {}
        _ => return Ok(None),
    }
    let mut dst = None;
    let mut start = from;
    loop {
        let len = match fs::symlink_metadata(src) {
            Ok(found) => found.len(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{}: {e}", src.display())),
        };
        if len.saturating_sub(start) > MAX_FILE {
            return Ok(None);
        }
        if dst.is_none() {
            dst = Some(prepare_target(set_dir, src)?);
        }
        let Some(target) = dst.as_deref() else {
            return Ok(None);
        };
        match copy_tail(src, start, target, meta.quiet_ms, meta.now_ms)? {
            Copied::Done { len, sha256 } => {
                return Ok(Some(Part {
                    kind: PartKind::Tail { from: start },
                    path: src.to_path_buf(),
                    sha256,
                    len,
                }));
            }
            Copied::Rewritten if start != 0 => start = 0,
            _ => return Err(format!("{} is still being written", src.display())),
        }
    }
}

fn stage_parts(plan: &SetPlan, set_dir: &Path, meta: &StageMeta) -> Result<Vec<Part>, String> {
    let mut parts = Vec::new();
    for db in &plan.dbs {
        parts.extend(db_part(set_dir, db)?);
    }
    for file in &plan.files {
        parts.extend(file_part(set_dir, file, meta)?);
    }
    for (tail, from) in &plan.tails {
        parts.extend(tail_part(set_dir, tail, *from, meta)?);
    }
    Ok(parts)
}

fn write_manifest(
    plan: &SetPlan,
    set_dir: &Path,
    meta: &StageMeta,
    parts: Vec<Part>,
) -> Result<Manifest, String> {
    let manifest = Manifest {
        set: plan.id.clone(),
        seq: meta.seq,
        term: meta.term.clone(),
        home: meta.home.clone(),
        created_ms: meta.created_ms,
        sessions: plan.sessions.clone(),
        parts,
    };
    let bytes = serde_json::to_vec(&manifest).map_err(|e| e.to_string())?;
    notchling_platform::files::write_private_no_follow(&set_dir.join("manifest.json"), &bytes)
        .map_err(|e| e.to_string())?;
    Ok(manifest)
}

fn remove_dir(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn publish(work: &Path, set_dir: &Path) -> Result<(), String> {
    remove_dir(set_dir)?;
    fs::rename(work, set_dir).map_err(|e| format!("{}: {e}", set_dir.display()))
}

pub fn stage_set(plan: &SetPlan, stage: &Path, meta: &StageMeta) -> Result<Manifest, String> {
    valid_set_id(&plan.id)?;
    if fs::symlink_metadata(stage).is_ok_and(|found| found.file_type().is_symlink()) {
        return Err(format!("{} is a symlink", stage.display()));
    }
    notchling_platform::files::ensure_private_dir(stage).map_err(|e| e.to_string())?;
    refuse_unsafe_paths(plan, stage)?;
    let set_dir = stage.join(&plan.id);
    let work = stage.join(format!("{}{STAGING}", plan.id));
    remove_dir(&work)?;
    private_dirs(&work.join("parts"))?;
    let result = stage_parts(plan, &work, meta)
        .and_then(|parts| write_manifest(plan, &work, meta, parts))
        .and_then(|manifest| publish(&work, &set_dir).map(|()| manifest));
    if result.is_err() {
        let _ = fs::remove_dir_all(&work);
    }
    result
}
