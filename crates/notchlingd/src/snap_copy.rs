use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::SystemTime;

const CHUNK: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Copied {
    Done { len: u64, sha256: String },
    Busy,
    Rewritten,
}

#[derive(Clone, Copy, Debug)]
struct Stat {
    len: u64,
    modified: Option<SystemTime>,
    inode: u64,
}

impl Stat {
    fn of(meta: &fs::Metadata) -> Stat {
        Stat {
            len: meta.len(),
            modified: meta.modified().ok(),
            inode: inode_of(meta),
        }
    }

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

struct Source {
    file: File,
    before: Stat,
}

fn open_source(path: &Path) -> Result<Source, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let meta = file
        .metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    Ok(Source {
        file,
        before: Stat::of(&meta),
    })
}

impl Source {
    fn after(&self, path: &Path) -> Result<(Stat, bool), String> {
        let meta = self
            .file
            .metadata()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let at_path = match fs::symlink_metadata(path) {
            Ok(found) => found.is_file() && inode_of(&found) == self.before.inode,
            Err(_) => false,
        };
        Ok((Stat::of(&meta), at_path))
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn create_private_file(path: &Path) -> std::io::Result<File> {
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

pub(crate) fn hash_file(path: &Path) -> Result<(u64, String), String> {
    let mut source = open_source(path)?;
    pump(&mut source.file, &mut std::io::sink())
}

pub(crate) fn remove_quietly(path: &Path) {
    let _ = fs::remove_file(path);
}

fn create_dst(dst: &Path, created: &mut bool) -> Result<File, String> {
    let out = create_private_file(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    *created = true;
    Ok(out)
}

fn settle(result: Result<Copied, String>, dst: &Path, created: bool) -> Result<Copied, String> {
    if created && !matches!(result, Ok(Copied::Done { .. })) {
        remove_quietly(dst);
    }
    result
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
    let mut created = false;
    let result = quiet_copy(src, dst, quiet_ms, now_ms, after_copy, &mut created);
    settle(result, dst, created)
}

fn quiet_copy(
    src: &Path,
    dst: &Path,
    quiet_ms: i64,
    now_ms: i64,
    after_copy: &mut dyn FnMut(),
    created: &mut bool,
) -> Result<Copied, String> {
    let mut source = open_source(src)?;
    if !source.before.quiet(quiet_ms, now_ms) {
        return Ok(Copied::Busy);
    }
    let mut out = create_dst(dst, created)?;
    let (len, sha256) = pump(&mut (&mut source.file).take(source.before.len), &mut out)?;
    out.sync_all().map_err(|e| e.to_string())?;
    after_copy();
    let (after, at_path) = source.after(src)?;
    if len != source.before.len || !source.before.same(&after) || !at_path {
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
    let mut created = false;
    let result = tail_copy(src, from, dst, quiet_ms, now_ms, after_copy, &mut created);
    settle(result, dst, created)
}

fn tail_copy(
    src: &Path,
    from: u64,
    dst: &Path,
    quiet_ms: i64,
    now_ms: i64,
    after_copy: &mut dyn FnMut(),
    created: &mut bool,
) -> Result<Copied, String> {
    let mut source = open_source(src)?;
    if !source.before.quiet(quiet_ms, now_ms) {
        return Ok(Copied::Busy);
    }
    let before = source.before;
    if before.len < from {
        return Ok(Copied::Rewritten);
    }
    source
        .file
        .seek(SeekFrom::Start(from))
        .map_err(|e| format!("{}: {e}", src.display()))?;
    let mut out = create_dst(dst, created)?;
    let (len, sha256) = pump(&mut (&mut source.file).take(before.len - from), &mut out)?;
    out.sync_all().map_err(|e| e.to_string())?;
    after_copy();
    let (after, at_path) = source.after(src)?;
    if !at_path || after.inode != before.inode || after.len < before.len {
        return Ok(Copied::Rewritten);
    }
    if len != before.len - from || !before.same(&after) {
        return Ok(Copied::Busy);
    }
    Ok(Copied::Done { len, sha256 })
}
