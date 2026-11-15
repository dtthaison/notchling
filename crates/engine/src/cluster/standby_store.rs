use super::blob::{safe_name, Digest, CHUNK};
use super::id::NodeId;
use super::snapshot::{dir_digest, staged_path, Manifest, Part, PartKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

pub const SNAP_OFFER: &str = "snap.offer";
pub const SNAP_PART: &str = "snap.part";
pub const SNAP_COMMIT: &str = "snap.commit";
pub const SNAP_ACK: &str = "snap.ack";

pub const MAX_PARTS: usize = 20_000;
const MAX_SET_NAME: usize = 40;
const STORE_DIR: &str = "standby";
const SETS: &str = "sets";
const TAILS: &str = "tails";
const CURRENT: &str = "current.json";
const MANIFEST: &str = "manifest.json";
const PARTIAL: &str = ".partial";
const TEMP: &str = ".nlsnap-tmp";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acked {
    pub set: String,
    pub lengths: BTreeMap<PathBuf, u64>,
}

pub struct StandbyStore {
    base: PathBuf,
    root: PathBuf,
    home: NodeId,
}

fn hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn io(path: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", path.display())
}

fn builder() -> fs::DirBuilder {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
}

fn create_new(path: &Path) -> std::io::Result<File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

pub(crate) fn open_plain(path: &Path) -> Result<File, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| io(path, e))?;
    if !meta.is_file() {
        return Err(format!("{} is not a plain file", path.display()));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).map_err(|e| io(path, e))
}

pub(crate) fn hash_range(path: &Path, from: u64, len: u64) -> Result<String, String> {
    let mut file = open_plain(path)?;
    file.seek(SeekFrom::Start(from)).map_err(|e| io(path, e))?;
    let mut digest = Digest::default();
    let mut block = vec![0; CHUNK];
    let mut left = len;
    while left > 0 {
        let want = left.min(CHUNK as u64) as usize;
        let n = file.read(&mut block[..want]).map_err(|e| io(path, e))?;
        if n == 0 {
            return Err(format!("{} is shorter than expected", path.display()));
        }
        digest.update(&block[..n]);
        left -= n as u64;
    }
    Ok(digest.finish())
}

pub(crate) fn plain_len(path: &Path) -> Result<Option<u64>, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(Some(meta.len())),
        Ok(_) => Err(format!("{} is not a plain file", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path, e)),
    }
}

pub(crate) fn hash_file(path: &Path) -> Result<(u64, String), String> {
    let len = plain_len(path)?.ok_or_else(|| format!("{} is missing", path.display()))?;
    Ok((len, hash_range(path, 0, len)?))
}

fn remove_tree(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || meta.is_file() => {
            fs::remove_file(path).map_err(|e| io(path, e))
        }
        Ok(_) => fs::remove_dir_all(path).map_err(|e| io(path, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io(path, e)),
    }
}

fn valid_set(set: &str) -> Result<(), String> {
    if safe_name(set) && set.len() <= MAX_SET_NAME {
        Ok(())
    } else {
        Err(format!("{set:?} is not a set name"))
    }
}

fn has_name(path: &Path) -> bool {
    path.components()
        .any(|part| matches!(part, Component::Normal(_)))
}

fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

fn validate(m: &Manifest) -> Result<(), String> {
    valid_set(&m.set)?;
    if m.parts.len() > MAX_PARTS {
        return Err("the set has too many parts".to_string());
    }
    let mut placed: Vec<&Path> = Vec::new();
    for part in &m.parts {
        if !hex64(&part.sha256) {
            return Err(format!("{} has no valid hash", part.path.display()));
        }
        if staged_path(Path::new("/x"), &part.path).is_none() || !has_name(&part.path) {
            return Err(format!(
                "{} is not a plain absolute path",
                part.path.display()
            ));
        }
        if let PartKind::Tail { from } = part.kind {
            if from.checked_add(part.len).is_none() {
                return Err(format!("{} has an impossible length", part.path.display()));
            }
        }
        if placed.iter().any(|other| overlaps(other, &part.path)) {
            return Err(format!("{} is in the set twice", part.path.display()));
        }
        placed.push(&part.path);
    }
    Ok(())
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| io(dir, e))? {
        let entry = entry.map_err(|e| io(dir, e))?;
        let path = entry.path();
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| format!("{} has a name that is not text", path.display()))?;
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

fn dir_entries(dir: &Path) -> Result<(String, u64), String> {
    let mut files = Vec::new();
    walk(dir, "", &mut files)?;
    let mut entries = Vec::with_capacity(files.len());
    let mut total = 0u64;
    for (rel, path) in files {
        let (len, sha256) = hash_file(&path)?;
        total += len;
        entries.push((rel, len, sha256));
    }
    Ok((dir_digest(&entries), total))
}

impl StandbyStore {
    pub fn new(cluster_dir: &Path, home: &NodeId) -> StandbyStore {
        StandbyStore {
            base: cluster_dir.to_path_buf(),
            root: cluster_dir.join(STORE_DIR).join(home.as_str()),
            home: home.clone(),
        }
    }

    fn ensure(&self, dir: &Path) -> Result<(), String> {
        let rel = dir
            .strip_prefix(&self.base)
            .map_err(|_| format!("{} is outside the store", dir.display()))?;
        builder()
            .recursive(true)
            .create(&self.base)
            .map_err(|e| io(&self.base, e))?;
        let mut here = self.base.clone();
        for part in rel.components() {
            let Component::Normal(name) = part else {
                return Err(format!("{} is not a plain path", dir.display()));
            };
            here.push(name);
            match fs::symlink_metadata(&here) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => return Err(format!("{} is a link or not a folder", here.display())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    builder().create(&here).map_err(|e| io(&here, e))?;
                }
                Err(e) => return Err(io(&here, e)),
            }
        }
        Ok(())
    }

    fn sets(&self) -> PathBuf {
        self.root.join(SETS)
    }

    fn partial(&self, set: &str) -> PathBuf {
        self.sets().join(format!("{set}{PARTIAL}"))
    }

    fn tails(&self) -> PathBuf {
        self.root.join(TAILS)
    }

    fn tail_path(&self, original: &Path) -> Result<PathBuf, String> {
        staged_path(&self.tails(), original)
            .ok_or_else(|| format!("{} can't be stored", original.display()))
    }

    fn place(&self, set_dir: &Path, original: &Path) -> Result<PathBuf, String> {
        staged_path(set_dir, original)
            .ok_or_else(|| format!("{} can't be stored", original.display()))
    }

    fn read_manifest(&self, dir: &Path) -> Option<Manifest> {
        let path = dir.join(MANIFEST);
        let file = open_plain(&path).ok()?;
        let mut text = String::new();
        file.take(64 * 1024 * 1024).read_to_string(&mut text).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn stored(&self, set: &str, original: &Path) -> Option<PathBuf> {
        valid_set(set).ok()?;
        self.place(&self.sets().join(set), original).ok()
    }

    pub fn stored_tail(&self, original: &Path) -> Option<PathBuf> {
        self.tail_path(original).ok()
    }

    pub fn current(&self) -> Option<Manifest> {
        let file = open_plain(&self.root.join(CURRENT)).ok()?;
        let mut text = String::new();
        file.take(64 * 1024 * 1024).read_to_string(&mut text).ok()?;
        let manifest: Manifest = serde_json::from_str(&text).ok()?;
        valid_set(&manifest.set).ok()?;
        let held = fs::symlink_metadata(self.sets().join(&manifest.set)).ok()?;
        held.is_dir().then_some(manifest)
    }

    pub fn begin(&self, m: &Manifest) -> Result<(), String> {
        validate(m)?;
        if m.home != self.home {
            return Err("that set is from a machine that isn't this one's primary".to_string());
        }
        self.ensure(&self.sets())?;
        let current = self.current();
        if let Some(held) = &current {
            if held.set == m.set {
                return Ok(());
            }
            if (m.term.n, m.seq) < (held.term.n, held.seq) {
                return Err(format!(
                    "{} is older than {}, which is held",
                    m.set, held.set
                ));
            }
        }
        let partial = self.partial(&m.set);
        if self.read_manifest(&partial).as_ref() == Some(m) {
            return Ok(());
        }
        self.clear_partials()?;
        self.ensure(&partial.join("parts"))?;
        if let Some(held) = &current {
            self.reuse(held, m, &partial);
        }
        let bytes = serde_json::to_vec(m).map_err(|e| e.to_string())?;
        notchling_platform::files::write_private_no_follow(&partial.join(MANIFEST), &bytes)
            .map_err(|e| e.to_string())
    }

    fn clear_partials(&self) -> Result<(), String> {
        let Ok(entries) = fs::read_dir(self.sets()) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().ends_with(PARTIAL) {
                remove_tree(&entry.path())?;
            }
        }
        Ok(())
    }

    fn reuse(&self, held: &Manifest, m: &Manifest, partial: &Path) {
        let from_dir = self.sets().join(&held.set);
        for part in &m.parts {
            if matches!(part.kind, PartKind::Tail { .. }) || !held.parts.contains(part) {
                continue;
            }
            let (Ok(src), Ok(dst)) = (
                self.place(&from_dir, &part.path),
                self.place(partial, &part.path),
            ) else {
                continue;
            };
            if let Some(parent) = dst.parent() {
                if self.ensure(parent).is_err() {
                    continue;
                }
            }
            if link_tree(&src, &dst).is_err() {
                let _ = remove_tree(&dst);
            }
        }
    }

    pub fn part(&self, set: &str, part: &Part, from: &Path) -> Result<(), String> {
        valid_set(set)?;
        let partial = self.partial(set);
        let m = self
            .read_manifest(&partial)
            .ok_or_else(|| format!("{set} isn't arriving"))?;
        if !m.parts.contains(part) {
            return Err(format!("{} isn't part of {set}", part.path.display()));
        }
        let dst = self.place(&partial, &part.path)?;
        match part.kind {
            PartKind::Db | PartKind::File => self.install_file(from, &dst, part),
            PartKind::Tail { from: start } => {
                self.check_tail(&part.path, start, part)?;
                self.install_file(from, &dst, part)
            }
            PartKind::Dir => self.install_dir(from, &dst, part),
        }
    }

    fn install_file(&self, from: &Path, dst: &Path, part: &Part) -> Result<(), String> {
        let (len, sha256) = hash_file(from)?;
        if len != part.len || sha256 != part.sha256 {
            return Err(format!(
                "{} arrived changed: its length or hash doesn't match",
                part.path.display()
            ));
        }
        let parent = dst
            .parent()
            .ok_or_else(|| format!("{} can't be stored", dst.display()))?;
        self.ensure(parent)?;
        let name = dst
            .file_name()
            .ok_or_else(|| format!("{} can't be stored", dst.display()))?;
        let temp = parent.join(format!("{}{TEMP}", name.to_string_lossy()));
        remove_tree(&temp)?;
        let copied = copy_into(from, &temp);
        let finished = copied.and_then(|()| {
            remove_tree(dst)?;
            fs::rename(&temp, dst).map_err(|e| io(dst, e))
        });
        if finished.is_err() {
            let _ = remove_tree(&temp);
        }
        finished
    }

    fn install_dir(&self, from: &Path, dst: &Path, part: &Part) -> Result<(), String> {
        let meta = fs::symlink_metadata(from).map_err(|e| io(from, e))?;
        if !meta.is_dir() {
            return Err(format!("{} is not a folder", from.display()));
        }
        let (digest, total) = dir_entries(from)?;
        if digest != part.sha256 || total != part.len {
            return Err(format!(
                "{} arrived changed: its hash doesn't match",
                part.path.display()
            ));
        }
        let parent = dst
            .parent()
            .ok_or_else(|| format!("{} can't be stored", dst.display()))?;
        self.ensure(parent)?;
        let name = dst
            .file_name()
            .ok_or_else(|| format!("{} can't be stored", dst.display()))?;
        let temp = parent.join(format!("{}{TEMP}", name.to_string_lossy()));
        remove_tree(&temp)?;
        let built = self.copy_dir(from, &temp);
        let finished = built.and_then(|()| {
            remove_tree(dst)?;
            fs::rename(&temp, dst).map_err(|e| io(dst, e))
        });
        if finished.is_err() {
            let _ = remove_tree(&temp);
        }
        finished
    }

    fn copy_dir(&self, from: &Path, to: &Path) -> Result<(), String> {
        self.ensure(to)?;
        let mut files = Vec::new();
        walk(from, "", &mut files)?;
        for (rel, path) in files {
            let dst = to.join(&rel);
            if let Some(parent) = dst.parent() {
                self.ensure(parent)?;
            }
            copy_into(&path, &dst)?;
        }
        Ok(())
    }

    fn stored_len(&self, original: &Path) -> Result<u64, String> {
        Ok(plain_len(&self.tail_path(original)?)?.unwrap_or(0))
    }

    fn check_tail(&self, original: &Path, start: u64, part: &Part) -> Result<(), String> {
        if start == 0 {
            return Ok(());
        }
        let stored = self.stored_len(original)?;
        if stored == start {
            return Ok(());
        }
        if stored == start + part.len
            && hash_range(&self.tail_path(original)?, start, part.len)? == part.sha256
        {
            return Ok(());
        }
        Err(format!(
            "{} is at {stored}, not {start}",
            original.display()
        ))
    }

    fn verified(&self, partial: &Path, part: &Part) -> Result<PathBuf, String> {
        let at = self.place(partial, &part.path)?;
        match part.kind {
            PartKind::Dir => {
                let (digest, total) = dir_entries(&at)?;
                if digest != part.sha256 || total != part.len {
                    return Err(format!("{} doesn't match its hash", part.path.display()));
                }
            }
            _ => {
                let (len, sha256) = hash_file(&at)?;
                if len != part.len || sha256 != part.sha256 {
                    return Err(format!("{} doesn't match its hash", part.path.display()));
                }
            }
        }
        Ok(at)
    }

    pub fn commit(&self, set: &str) -> Result<(), String> {
        valid_set(set)?;
        if self.current().is_some_and(|held| held.set == set) {
            return Ok(());
        }
        let partial = self.partial(set);
        let m = self
            .read_manifest(&partial)
            .ok_or_else(|| format!("{set} isn't arriving"))?;
        validate(&m)?;
        let mut moves = Vec::new();
        for part in &m.parts {
            let at = self
                .verified(&partial, part)
                .map_err(|why| format!("{set} is incomplete: {why}"))?;
            if let PartKind::Tail { from } = part.kind {
                moves.push(self.tail_move(part, from, at)?);
            }
        }
        for step in moves {
            step.run()?;
        }
        let held = self.sets().join(set);
        remove_tree(&held)?;
        fs::rename(&partial, &held).map_err(|e| io(&held, e))?;
        let bytes = serde_json::to_vec(&m).map_err(|e| e.to_string())?;
        notchling_platform::files::write_private_no_follow(&self.root.join(CURRENT), &bytes)
            .map_err(|e| e.to_string())
    }

    fn tail_move(&self, part: &Part, from: u64, staged: PathBuf) -> Result<TailMove, String> {
        let dst = self.tail_path(&part.path)?;
        let parent = dst
            .parent()
            .ok_or_else(|| format!("{} can't be stored", dst.display()))?
            .to_path_buf();
        self.ensure(&parent)?;
        let stored = plain_len(&dst)?.unwrap_or(0);
        let end = from + part.len;
        let action = if from == 0 {
            Action::Replace
        } else if stored < from || stored > end {
            return Err(format!(
                "{} is at {stored}, not {from}",
                part.path.display()
            ));
        } else if stored == end {
            Action::Keep
        } else {
            let seen = stored - from;
            if hash_range(&dst, from, seen)? != hash_range(&staged, 0, seen)? {
                return Err(format!(
                    "{} has bytes the set doesn't agree with",
                    part.path.display()
                ));
            }
            Action::Append { cut: seen > 0 }
        };
        Ok(TailMove {
            action,
            staged,
            dst,
            from,
        })
    }

    pub fn lengths(&self) -> BTreeMap<PathBuf, u64> {
        let base = self.tails().join("parts");
        let mut files = Vec::new();
        walk_lenient(&base, "", &mut files);
        files
            .into_iter()
            .filter(|(rel, _)| !rel.ends_with(TEMP))
            .filter_map(|(rel, path)| {
                let len = plain_len(&path).ok()??;
                Some((Path::new("/").join(rel), len))
            })
            .collect()
    }

    pub fn prune(&self, keep: usize) {
        let Ok(entries) = fs::read_dir(self.sets()) else {
            return;
        };
        let Some(current) = self.current().map(|m| m.set) else {
            return;
        };
        let mut held: Vec<((u64, u64), String)> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(PARTIAL) || valid_set(&name).is_err() {
                continue;
            }
            let order = self
                .read_manifest(&entry.path())
                .map_or((0, 0), |m| (m.term.n, m.seq));
            held.push((order, name));
        }
        held.sort_by(|a, b| b.cmp(a));
        let mut kept = 0usize;
        for (_, name) in held {
            if name == current || kept < keep {
                kept += 1;
                continue;
            }
            let _ = remove_tree(&self.sets().join(name));
        }
    }

    pub fn missing(&self, m: &Manifest) -> Vec<Part> {
        if self.current().is_some_and(|held| held.set == m.set) {
            return Vec::new();
        }
        let partial = self.partial(&m.set);
        m.parts
            .iter()
            .filter(|part| !self.arrived(&partial, part))
            .cloned()
            .collect()
    }

    fn arrived(&self, partial: &Path, part: &Part) -> bool {
        let Ok(at) = self.place(partial, &part.path) else {
            return false;
        };
        match (fs::symlink_metadata(&at), &part.kind) {
            (Ok(meta), PartKind::Dir) => meta.is_dir(),
            (Ok(meta), _) => meta.is_file() && meta.len() == part.len,
            _ => false,
        }
    }
}

fn walk_lenient(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let path = entry.path();
        let rel_name = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => walk_lenient(&path, &rel_name, out),
            Ok(meta) if meta.is_file() => out.push((rel_name, path)),
            _ => {}
        }
    }
}

enum Action {
    Replace,
    Keep,
    Append { cut: bool },
}

struct TailMove {
    action: Action,
    staged: PathBuf,
    dst: PathBuf,
    from: u64,
}

impl TailMove {
    fn run(self) -> Result<(), String> {
        match self.action {
            Action::Keep => Ok(()),
            Action::Replace => {
                let name = self
                    .dst
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned());
                let temp = self
                    .dst
                    .with_file_name(format!("{}{TEMP}", name.unwrap_or_default()));
                remove_tree(&temp)?;
                copy_into(&self.staged, &temp)?;
                fs::rename(&temp, &self.dst).map_err(|e| io(&self.dst, e))
            }
            Action::Append { cut } => {
                if cut {
                    let mut options = fs::OpenOptions::new();
                    options.write(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        options.custom_flags(libc::O_NOFOLLOW);
                    }
                    let file = options.open(&self.dst).map_err(|e| io(&self.dst, e))?;
                    file.set_len(self.from).map_err(|e| io(&self.dst, e))?;
                }
                append(&self.staged, &self.dst)
            }
        }
    }
}

fn copy_into(from: &Path, to: &Path) -> Result<(), String> {
    let mut src = open_plain(from)?;
    let mut out = create_new(to).map_err(|e| io(to, e))?;
    let mut block = vec![0; CHUNK];
    let result = (|| -> std::io::Result<()> {
        loop {
            let n = src.read(&mut block)?;
            if n == 0 {
                break;
            }
            out.write_all(&block[..n])?;
        }
        out.sync_all()
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(to);
        return Err(io(to, e));
    }
    Ok(())
}

fn append(from: &Path, to: &Path) -> Result<(), String> {
    let mut src = open_plain(from)?;
    let mut options = fs::OpenOptions::new();
    options.append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut out = options.open(to).map_err(|e| io(to, e))?;
    let mut block = vec![0; CHUNK];
    loop {
        let n = src.read(&mut block).map_err(|e| io(from, e))?;
        if n == 0 {
            break;
        }
        out.write_all(&block[..n]).map_err(|e| io(to, e))?;
    }
    out.sync_all().map_err(|e| io(to, e))
}

fn link_tree(src: &Path, dst: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(src).map_err(|e| io(src, e))?;
    if meta.is_dir() {
        builder().create(dst).map_err(|e| io(dst, e))?;
        for entry in fs::read_dir(src).map_err(|e| io(src, e))? {
            let entry = entry.map_err(|e| io(src, e))?;
            link_tree(&entry.path(), &dst.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if !meta.is_file() {
        return Err(format!("{} is not a plain file", src.display()));
    }
    fs::hard_link(src, dst).or_else(|_| copy_into(src, dst))
}
