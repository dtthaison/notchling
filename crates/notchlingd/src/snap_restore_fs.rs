use crate::snap_copy::{create_private_file, hash_file, hex, remove_quietly};
use crate::snap_db::backup_db;
use notchling_engine::bridge::net::devices::{encode, read_devices};
use notchling_engine::bridge::net::files::{write_private, FileLock};
use notchling_engine::cluster::persist::CLUSTER_DB;
use notchling_engine::cluster::snapshot::dir_digest;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

const CHUNK: usize = 64 * 1024;
const TEMP: &str = "nlrestore";

pub struct DbHooks<'a> {
    pub close: &'a dyn Fn() -> Result<(), String>,
    pub reopen: &'a dyn Fn() -> Result<(), String>,
}

fn io(path: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", path.display())
}

fn open_read(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(|e| io(path, e))?;
    let meta = file.metadata().map_err(|e| io(path, e))?;
    if !meta.is_file() {
        return Err(format!("{} isn't a plain file", path.display()));
    }
    Ok(file)
}

pub fn guard(home: &Path, path: &Path) -> Result<(), String> {
    let rel = path
        .strip_prefix(home)
        .map_err(|_| format!("{} isn't under {}", path.display(), home.display()))?;
    let real_home = fs::canonicalize(home).map_err(|e| io(home, e))?;
    let mut here = home.to_path_buf();
    for part in rel.components() {
        let Component::Normal(name) = part else {
            return Err(format!("{} isn't a plain path", path.display()));
        };
        here.push(name);
        match fs::symlink_metadata(&here) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let real = fs::canonicalize(&here).map_err(|e| io(&here, e))?;
                if !real.starts_with(&real_home) {
                    return Err(format!(
                        "{} is a link that leads out of {}",
                        here.display(),
                        home.display()
                    ));
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(io(&here, e)),
        }
    }
    Ok(())
}

pub fn repo_place(
    home: &Path,
    path: &Path,
    main: Option<&Path>,
    strict: bool,
) -> Result<(), String> {
    guard(home, path)?;
    let rel = path
        .strip_prefix(home)
        .map_err(|_| format!("{} isn't under {}", path.display(), home.display()))?;
    let count = rel.components().count();
    let mut here = home.to_path_buf();
    for (n, part) in rel.components().enumerate() {
        here.push(part.as_os_str());
        match fs::symlink_metadata(&here) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!(
                    "{} is a link; a repository sits in real folders only",
                    here.display()
                ));
            }
            Ok(meta) if !meta.is_dir() => {
                return Err(format!("{} isn't a folder", here.display()));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(io(&here, e)),
        }
        let allowed = main.is_some_and(|main| main == here);
        if strict && n + 1 < count && !allowed && fs::symlink_metadata(here.join(".git")).is_ok() {
            return Err(format!(
                "{} is inside the repository {}",
                path.display(),
                here.display()
            ));
        }
    }
    Ok(())
}

pub fn real_folder(home: &Path, path: &Path) -> Result<(), String> {
    guard(home, path)?;
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(format!("{} isn't a plain folder", path.display())),
        Err(e) => Err(io(path, e)),
    }
}

pub fn make_dirs(home: &Path, dir: &Path) -> Result<(), String> {
    guard(home, dir)?;
    let rel = dir
        .strip_prefix(home)
        .map_err(|_| format!("{} isn't under {}", dir.display(), home.display()))?;
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    let mut here = home.to_path_buf();
    for part in rel.components() {
        here.push(part.as_os_str());
        match fs::symlink_metadata(&here) {
            Ok(meta) if meta.is_dir() => {}
            Ok(meta) if meta.file_type().is_symlink() && here.is_dir() => {}
            Ok(_) => return Err(format!("{} isn't a folder", here.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                builder.create(&here).map_err(|e| io(&here, e))?;
            }
            Err(e) => return Err(io(&here, e)),
        }
    }
    Ok(())
}

fn temp_beside(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.{TEMP}"))
}

fn plain_or_missing(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(true),
        Ok(_) => Err(format!("{} isn't a plain file", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io(path, e)),
    }
}

fn checked(from: &Path, sha256: &str, len: Option<u64>) -> Result<(), String> {
    let (got, hash) = hash_file(from)?;
    if hash != sha256 || len.is_some_and(|want| want != got) {
        return Err(format!(
            "{} in the store doesn't match the set",
            from.display()
        ));
    }
    Ok(())
}

fn install(home: &Path, from: &Path, to: &Path, sha256: &str) -> Result<(), String> {
    let parent = to
        .parent()
        .ok_or_else(|| format!("{} has no folder", to.display()))?;
    make_dirs(home, parent)?;
    plain_or_missing(to)?;
    let temp = temp_beside(to);
    let result = (|| -> Result<(), String> {
        let mut src = open_read(from)?;
        let mut out = create_private_file(&temp).map_err(|e| io(&temp, e))?;
        let mut hasher = Sha256::new();
        let mut block = vec![0u8; CHUNK];
        loop {
            let n = src.read(&mut block).map_err(|e| io(from, e))?;
            if n == 0 {
                break;
            }
            hasher.update(&block[..n]);
            out.write_all(&block[..n]).map_err(|e| io(&temp, e))?;
        }
        if hex(&hasher.finalize()) != sha256 {
            return Err(format!("{} changed while it was read", from.display()));
        }
        out.sync_all().map_err(|e| io(&temp, e))?;
        drop(out);
        fs::rename(&temp, to).map_err(|e| io(to, e))
    })();
    if result.is_err() {
        remove_quietly(&temp);
    }
    result
}

pub fn put_file(home: &Path, from: &Path, to: &Path, sha256: &str, len: u64) -> Result<(), String> {
    checked(from, sha256, Some(len))?;
    install(home, from, to, sha256)
}

pub fn append_tail(
    home: &Path,
    from: &Path,
    to: &Path,
    offset: u64,
    end: u64,
    prefix_sha256: &str,
) -> Result<(), String> {
    guard(home, to)?;
    let changed = || format!("{} changed while it was being restored", to.display());
    let meta = fs::symlink_metadata(to).map_err(|e| io(to, e))?;
    if !meta.is_file() || meta.len() != offset || hash_file(to)?.1 != prefix_sha256 {
        return Err(changed());
    }
    let mut src = open_read(from)?;
    if src.metadata().map_err(|e| io(from, e))?.len() != end {
        return Err(format!(
            "{} in the store doesn't match the set",
            from.display()
        ));
    }
    src.seek(SeekFrom::Start(offset)).map_err(|e| io(from, e))?;
    let mut options = OpenOptions::new();
    options.append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut out = options.open(to).map_err(|e| io(to, e))?;
    if out.metadata().map_err(|e| io(to, e))?.len() != offset {
        return Err(changed());
    }
    let mut block = vec![0u8; CHUNK];
    let mut left = end - offset;
    while left > 0 {
        let want = left.min(CHUNK as u64) as usize;
        let n = src.read(&mut block[..want]).map_err(|e| io(from, e))?;
        if n == 0 {
            return Err(format!("{} is shorter than the set says", from.display()));
        }
        out.write_all(&block[..n]).map_err(|e| io(to, e))?;
        left -= n as u64;
    }
    out.sync_all().map_err(|e| io(to, e))
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| io(dir, e))? {
        let entry = entry.map_err(|e| io(dir, e))?;
        let path = entry.path();
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| format!("{} has a name that isn't text", path.display()))?;
        if name.contains('\n') {
            return Err(format!("{} has a bad name", path.display()));
        }
        let rel_name = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let meta = fs::symlink_metadata(&path).map_err(|e| io(&path, e))?;
        if meta.is_dir() {
            walk(&path, &rel_name, out)?;
        } else if meta.is_file() {
            out.push((rel_name, path));
        } else {
            return Err(format!("{} is a link or special file", path.display()));
        }
    }
    Ok(())
}

pub fn put_dir(home: &Path, from: &Path, to: &Path, sha256: &str, len: u64) -> Result<(), String> {
    let mut files = Vec::new();
    walk(from, "", &mut files)?;
    let mut listed = Vec::with_capacity(files.len());
    let mut total = 0u64;
    for (rel, path) in files {
        let (size, hash) = hash_file(&path)?;
        total += size;
        listed.push((rel, path, size, hash));
    }
    let entries: Vec<(String, u64, String)> = listed
        .iter()
        .map(|(rel, _, size, hash)| (rel.clone(), *size, hash.clone()))
        .collect();
    if dir_digest(&entries) != sha256 || total != len {
        return Err(format!(
            "{} in the store doesn't match the set",
            from.display()
        ));
    }
    make_dirs(home, to)?;
    for (rel, path, _, hash) in listed {
        let dst = to.join(&rel);
        guard(home, &dst)?;
        if plain_or_missing(&dst)? && hash_file(&dst)?.1 == hash {
            continue;
        }
        install(home, &path, &dst, &hash)?;
    }
    Ok(())
}

fn unused(to: &Path) -> Result<PathBuf, String> {
    if fs::symlink_metadata(to).is_err() {
        return Ok(to.to_path_buf());
    }
    let name = to
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    (1..1000)
        .map(|n| to.with_file_name(format!("{name}.{n}")))
        .find(|candidate| fs::symlink_metadata(candidate).is_err())
        .ok_or_else(|| format!("{} has too many copies", to.display()))
}

pub fn move_aside(home: &Path, path: &Path, to: &Path) -> Result<(), String> {
    guard(home, path)?;
    if !plain_or_missing(path)? {
        return Ok(());
    }
    let parent = to
        .parent()
        .ok_or_else(|| format!("{} has no folder", to.display()))?;
    make_dirs(home, parent)?;
    let target = unused(to)?;
    match fs::rename(path, &target) {
        Ok(()) => Ok(()),
        #[cfg(unix)]
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            let (_, hash) = hash_file(path)?;
            install(home, path, &target, &hash)?;
            fs::remove_file(path).map_err(|e| io(path, e))
        }
        Err(e) => Err(io(path, e)),
    }
}

fn clear_side_files(db: &Path) {
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = db.as_os_str().to_owned();
        name.push(suffix);
        remove_quietly(Path::new(&name));
    }
}

fn sound(db: &Path) -> Result<(), String> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("{}: {e}", db.display()))?;
    let verdict: String = conn
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if verdict == "ok" {
        Ok(())
    } else {
        Err(format!("the restored database failed its check: {verdict}"))
    }
}

pub fn restore_db(
    home: &Path,
    from: &Path,
    to: &Path,
    sha256: &str,
    hooks: &DbHooks,
) -> Result<(), String> {
    checked(from, sha256, None)?;
    let parent = to
        .parent()
        .ok_or_else(|| format!("{} has no folder", to.display()))?;
    make_dirs(home, parent)?;
    plain_or_missing(to)?;
    let temp = temp_beside(to);
    let built = backup_db(from, &temp).and_then(|()| sound(&temp));
    if let Err(why) = built {
        remove_quietly(&temp);
        return Err(why);
    }
    if let Err(why) = (hooks.close)() {
        remove_quietly(&temp);
        return Err(format!("couldn't close the database first: {why}"));
    }
    let swapped = fs::rename(&temp, to).map_err(|e| io(to, e));
    if swapped.is_ok() {
        clear_side_files(to);
    } else {
        remove_quietly(&temp);
    }
    let reopened = (hooks.reopen)();
    swapped?;
    reopened.map_err(|why| format!("the database was restored but didn't reopen: {why}"))
}

pub const MOVE_STAGE: &str = "move-stage";

pub fn staged_db(cluster: &Path) -> PathBuf {
    cluster.join(MOVE_STAGE).join(CLUSTER_DB)
}

pub fn stage_db(home: &Path, from: &Path, stage: &Path, sha256: &str) -> Result<(), String> {
    checked(from, sha256, None)?;
    let parent = stage
        .parent()
        .ok_or_else(|| format!("{} has no folder", stage.display()))?;
    make_dirs(home, parent)?;
    plain_or_missing(stage)?;
    let temp = temp_beside(stage);
    let built = backup_db(from, &temp).and_then(|()| sound(&temp));
    if let Err(why) = built {
        remove_quietly(&temp);
        return Err(why);
    }
    clear_side_files(stage);
    fs::rename(&temp, stage).map_err(|e| {
        remove_quietly(&temp);
        io(stage, e)
    })
}

pub fn adopt_staged(cluster: &Path) -> Result<(), String> {
    let stage = staged_db(cluster);
    if !plain_or_missing(&stage)? {
        return Err("no restored database is waiting".to_string());
    }
    let db = cluster.join(CLUSTER_DB);
    plain_or_missing(&db)?;
    fs::rename(&stage, &db).map_err(|e| io(&db, e))?;
    clear_side_files(&db);
    if let Some(parent) = stage.parent() {
        let _ = fs::remove_dir(parent);
    }
    Ok(())
}

pub fn merge_devices(home: &Path, from: &Path, to: &Path, sha256: &str) -> Result<(), String> {
    checked(from, sha256, None)?;
    let theirs = read_devices(from).map_err(|e| e.to_string())?;
    let parent = to
        .parent()
        .ok_or_else(|| format!("{} has no folder", to.display()))?;
    make_dirs(home, parent)?;
    plain_or_missing(to)?;
    let _lock = FileLock::acquire(to).map_err(|e| io(to, e))?;
    let mut mine = read_devices(to).map_err(|e| e.to_string())?;
    let before = mine.len();
    for device in theirs {
        if !mine.iter().any(|own| own.id == device.id) {
            mine.push(device);
        }
    }
    if mine.len() == before {
        return Ok(());
    }
    write_private(to, &encode(&mine)).map_err(|e| io(to, e))
}

fn line_id(line: &str) -> Option<String> {
    let value: Value = serde_json::from_str(line).ok()?;
    value.get("id")?.as_str().map(str::to_owned)
}

fn read_text(path: &Path) -> Result<Option<String>, String> {
    if !plain_or_missing(path)? {
        return Ok(None);
    }
    let mut text = String::new();
    open_read(path)?
        .read_to_string(&mut text)
        .map_err(|e| io(path, e))?;
    Ok(Some(text))
}

pub fn merge_index(
    home: &Path,
    from: &Path,
    to: &Path,
    session: &str,
    sha256: &str,
) -> Result<(), String> {
    checked(from, sha256, None)?;
    let theirs = read_text(from)?.unwrap_or_default();
    let Some(line) = theirs
        .lines()
        .find(|line| line_id(line).as_deref() == Some(session))
    else {
        return Ok(());
    };
    guard(home, to)?;
    let parent = to
        .parent()
        .ok_or_else(|| format!("{} has no folder", to.display()))?;
    make_dirs(home, parent)?;
    let mut mine = read_text(to)?.unwrap_or_default();
    if mine
        .lines()
        .any(|have| line_id(have).as_deref() == Some(session))
    {
        return Ok(());
    }
    if !mine.is_empty() && !mine.ends_with('\n') {
        mine.push('\n');
    }
    mine.push_str(line);
    mine.push('\n');
    notchling_platform::files::write_private_no_follow(to, mine.as_bytes())
        .map_err(|e| e.to_string())
}
