use notchling_engine::cluster::id::NodeId;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::snapshot::{
    dir_digest, excluded, Manifest, Part, PartKind, SetPlan,
};
use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const PAGES_PER_STEP: i32 = 256;
const STEP_PAUSE: Duration = Duration::from_millis(5);
const STEPPED_FOR: Duration = Duration::from_secs(10);
const MAX_RESTARTS: u32 = 3;
const FINAL_PAUSE: Duration = Duration::from_millis(50);
const FINAL_TRIES: u32 = 200;
const CHUNK: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Copied {
    Done { len: u64, sha256: String },
    Busy,
    Rewritten,
}

#[derive(Clone, Debug)]
pub struct StageMeta {
    pub seq: u64,
    pub term: Term,
    pub home: NodeId,
    pub created_ms: i64,
    pub quiet_ms: i64,
    pub now_ms: i64,
}

struct Stat {
    len: u64,
    modified: Option<SystemTime>,
    inode: u64,
}

impl Stat {
    fn quiet(&self, quiet_ms: i64, now_ms: i64) -> bool {
        let modified_ms = self
            .modified
            .and_then(|at| at.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        now_ms.saturating_sub(modified_ms) >= quiet_ms
    }

    fn same(&self, other: &Stat) -> bool {
        self.len == other.len && self.modified == other.modified && self.inode == other.inode
    }
}

fn inode_of(meta: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0
    }
}

fn stat_file(path: &Path) -> Result<Stat, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    Ok(Stat {
        len: meta.len(),
        modified: meta.modified().ok(),
        inode: inode_of(&meta),
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn open_source(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn create_private_file(path: &Path) -> std::io::Result<File> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
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

fn pump(input: &mut dyn Read, out: &mut dyn Write) -> Result<(u64, String), String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    let mut total = 0u64;
    loop {
        let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        out.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        total += n as u64;
    }
    Ok((total, hex(&hasher.finalize())))
}

fn hash_file(path: &Path) -> Result<(u64, String), String> {
    let mut file = open_source(path)?;
    pump(&mut file, &mut std::io::sink())
}

fn remove_quietly(path: &Path) {
    let _ = fs::remove_file(path);
}

pub fn copy_quiet(src: &Path, dst: &Path, quiet_ms: i64, now_ms: i64) -> Result<Copied, String> {
    copy_quiet_hooked(src, dst, quiet_ms, now_ms, &mut || {})
}

pub(crate) fn copy_quiet_hooked(
    src: &Path,
    dst: &Path,
    quiet_ms: i64,
    now_ms: i64,
    after_copy: &mut dyn FnMut(),
) -> Result<Copied, String> {
    let result = quiet_copy(src, dst, quiet_ms, now_ms, after_copy);
    if !matches!(result, Ok(Copied::Done { .. })) {
        remove_quietly(dst);
    }
    result
}

fn quiet_copy(
    src: &Path,
    dst: &Path,
    quiet_ms: i64,
    now_ms: i64,
    after_copy: &mut dyn FnMut(),
) -> Result<Copied, String> {
    let before = stat_file(src)?;
    if !before.quiet(quiet_ms, now_ms) {
        return Ok(Copied::Busy);
    }
    let input = open_source(src)?;
    let mut out = create_private_file(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    let (len, sha256) = pump(&mut input.take(before.len), &mut out)?;
    out.sync_all().map_err(|e| e.to_string())?;
    after_copy();
    let after = stat_file(src)?;
    if len != before.len || !before.same(&after) {
        return Ok(Copied::Busy);
    }
    Ok(Copied::Done { len, sha256 })
}

pub fn copy_tail(
    src: &Path,
    from: u64,
    dst: &Path,
    quiet_ms: i64,
    now_ms: i64,
) -> Result<Copied, String> {
    copy_tail_hooked(src, from, dst, quiet_ms, now_ms, &mut || {})
}

pub(crate) fn copy_tail_hooked(
    src: &Path,
    from: u64,
    dst: &Path,
    quiet_ms: i64,
    now_ms: i64,
    after_copy: &mut dyn FnMut(),
) -> Result<Copied, String> {
    let result = tail_copy(src, from, dst, quiet_ms, now_ms, after_copy);
    if !matches!(result, Ok(Copied::Done { .. })) {
        remove_quietly(dst);
    }
    result
}

fn tail_copy(
    src: &Path,
    from: u64,
    dst: &Path,
    quiet_ms: i64,
    now_ms: i64,
    after_copy: &mut dyn FnMut(),
) -> Result<Copied, String> {
    let before = stat_file(src)?;
    if !before.quiet(quiet_ms, now_ms) {
        return Ok(Copied::Busy);
    }
    if before.len < from {
        return Ok(Copied::Rewritten);
    }
    let mut input = open_source(src)?;
    input
        .seek(SeekFrom::Start(from))
        .map_err(|e| format!("{}: {e}", src.display()))?;
    let mut out = create_private_file(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    let (len, sha256) = pump(&mut input.take(before.len - from), &mut out)?;
    out.sync_all().map_err(|e| e.to_string())?;
    after_copy();
    let after = stat_file(src)?;
    if after.inode != before.inode || after.len < before.len || len != before.len - from {
        return Ok(Copied::Rewritten);
    }
    Ok(Copied::Done { len, sha256 })
}

pub fn backup_db(src: &Path, dst: &Path) -> Result<(), String> {
    let source = Connection::open_with_flags(
        src,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("{}: {e}", src.display()))?;
    source
        .busy_timeout(Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let result = copy_db(&source, dst);
    if result.is_err() {
        remove_quietly(dst);
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = dst.as_os_str().to_owned();
        name.push(suffix);
        remove_quietly(Path::new(&name));
    }
    result
}

fn copy_db(source: &Connection, dst: &Path) -> Result<(), String> {
    create_private_file(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    let mut target = Connection::open(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    {
        let backup = Backup::new(source, &mut target).map_err(|e| e.to_string())?;
        run_steps(&backup)?;
    }
    target
        .pragma_update_and_check(None, "journal_mode", "DELETE", |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?;
    target.close().map_err(|(_, e)| e.to_string())
}

fn run_steps(backup: &Backup<'_, '_>) -> Result<(), String> {
    let started = Instant::now();
    let mut restarts = 0u32;
    let mut last_remaining = i32::MAX;
    while started.elapsed() < STEPPED_FOR && restarts <= MAX_RESTARTS {
        if let StepResult::Done = backup.step(PAGES_PER_STEP).map_err(|e| e.to_string())? {
            return Ok(());
        }
        let remaining = backup.progress().remaining;
        if remaining > last_remaining {
            restarts += 1;
        }
        last_remaining = remaining;
        std::thread::sleep(STEP_PAUSE);
    }
    for _ in 0..FINAL_TRIES {
        if let StepResult::Done = backup.step(-1).map_err(|e| e.to_string())? {
            return Ok(());
        }
        std::thread::sleep(FINAL_PAUSE);
    }
    Err("the database stayed busy".to_owned())
}

fn valid_set_id(id: &str) -> Result<(), String> {
    let mut parts = Path::new(id).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(name)), None) if name == id => Ok(()),
        _ => Err(format!("{id:?} is not a set id")),
    }
}

fn refuse_repo_paths(plan: &SetPlan) -> Result<(), String> {
    let roots: Vec<&Path> = plan
        .sessions
        .iter()
        .filter_map(|s| s.repo.as_ref().map(|r| r.root.as_path()))
        .collect();
    let paths = plan
        .dbs
        .iter()
        .chain(plan.files.iter())
        .chain(plan.tails.iter().map(|(path, _)| path));
    for path in paths {
        for root in &roots {
            if path.starts_with(root) || root.starts_with(path) {
                return Err(format!(
                    "{} is inside the repository {}: code never travels",
                    path.display(),
                    root.display()
                ));
            }
        }
    }
    Ok(())
}

fn staged_path(set_dir: &Path, source: &Path) -> Result<PathBuf, String> {
    if !source.is_absolute() {
        return Err(format!("{} is not an absolute path", source.display()));
    }
    let mut target = set_dir.join("parts");
    for component in source.components() {
        match component {
            Component::Normal(name) => target.push(name),
            Component::RootDir | Component::Prefix(_) => {}
            _ => return Err(format!("{} has a . or .. part", source.display())),
        }
    }
    Ok(target)
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
    let target = staged_path(set_dir, source)?;
    if let Some(parent) = target.parent() {
        private_dirs(parent)?;
    }
    Ok(target)
}

fn quiet_part(src: &Path, dst: &Path, meta: &StageMeta) -> Result<(u64, String), String> {
    match copy_quiet(src, dst, meta.quiet_ms, meta.now_ms)? {
        Copied::Done { len, sha256 } => Ok((len, sha256)),
        _ => Err(format!("{} is still being written", src.display())),
    }
}

fn db_part(set_dir: &Path, src: &Path) -> Result<Option<Part>, String> {
    if excluded(src) {
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
    if excluded(src) {
        return Ok(None);
    }
    match classify(src)? {
        Source::Missing | Source::Link => Ok(None),
        Source::Dir => dir_part(set_dir, src, meta),
        Source::File => {
            let dst = prepare_target(set_dir, src)?;
            let (len, sha256) = quiet_part(src, &dst, meta)?;
            Ok(Some(Part {
                kind: PartKind::File,
                path: src.to_path_buf(),
                sha256,
                len,
            }))
        }
    }
}

fn walk_files(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel_name = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        if excluded(&path) {
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
        let (len, sha256) = quiet_part(&path, &dst, meta)?;
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
    if excluded(src) {
        return Ok(None);
    }
    match classify(src)? {
        Source::File => {}
        _ => return Ok(None),
    }
    let dst = prepare_target(set_dir, src)?;
    let mut start = from;
    loop {
        match copy_tail(src, start, &dst, meta.quiet_ms, meta.now_ms)? {
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

pub fn stage_set(plan: &SetPlan, stage: &Path, meta: &StageMeta) -> Result<Manifest, String> {
    valid_set_id(&plan.id)?;
    refuse_repo_paths(plan)?;
    notchling_platform::files::ensure_private_dir(stage).map_err(|e| e.to_string())?;
    let set_dir = stage.join(&plan.id);
    match fs::remove_dir_all(&set_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", set_dir.display())),
    }
    private_dirs(&set_dir.join("parts"))?;
    let result = stage_parts(plan, &set_dir, meta)
        .and_then(|parts| write_manifest(plan, &set_dir, meta, parts));
    if result.is_err() {
        let _ = fs::remove_dir_all(&set_dir);
    }
    result
}
