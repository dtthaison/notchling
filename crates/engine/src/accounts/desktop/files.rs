use serde_json::{json, Map, Value};
use std::fs::{File, Metadata, OpenOptions, Permissions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const BACKUP_NAME: &str = "Cookies.notchling-backup";
pub const UNNAMED_DIR: &str = "unnamed";
pub const UNNAMED_KEPT: usize = 10;
const WRITE_AHEAD: [&str; 2] = ["Cookies-wal", "Cookies-journal"];
const WAL_INDEX: &str = "Cookies-shm";
const ACCOUNT: &str = "lastKnownAccountUuid";
const SIDE_SESSIONS: &str = "bootSideSessions";

pub struct Dirs<'a> {
    pub claude: &'a Path,
    pub store: &'a Path,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    pub copy: PathBuf,
    pub account: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    Unsettled,
    Unkept,
    Failed,
}

pub struct Session {
    pub account: Option<String>,
    pub cookies: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Missing,
    Plain(u64),
    Other,
}

type Numbered = Vec<(u64, PathBuf)>;
type Stamp = (u64, Option<SystemTime>);

#[derive(PartialEq)]
struct Found {
    config: Option<(Vec<u8>, Permissions)>,
    root: Map<String, Value>,
    cookies: Option<(Vec<u8>, Stamp)>,
}

pub fn cookies(claude_dir: &Path) -> PathBuf {
    claude_dir.join("Cookies")
}

pub fn backup(claude_dir: &Path) -> PathBuf {
    claude_dir.join(BACKUP_NAME)
}

fn config(claude_dir: &Path) -> PathBuf {
    claude_dir.join("config.json")
}

pub fn saved(store_dir: &Path, uuid: &str) -> PathBuf {
    store_dir.join(format!("{uuid}.cookies"))
}

fn temp(claude_dir: &Path, name: &str, tag: &str) -> PathBuf {
    claude_dir.join(format!(".{name}.{}.{tag}", std::process::id()))
}

pub fn valid_uuid(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn kind(path: &Path) -> Kind {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Kind::Plain(meta.len()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Kind::Missing,
        _ => Kind::Other,
    }
}

pub fn is_copy(path: &Path) -> bool {
    matches!(kind(path), Kind::Plain(_))
}

#[cfg(unix)]
fn identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::symlink_metadata(path).ok()?;
    Some((meta.dev(), meta.ino()))
}

#[cfg(not(unix))]
fn identity(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || identity(a).zip(identity(b)).is_some_and(|(a, b)| a == b)
}

fn named(account: Option<&str>) -> Option<&str> {
    account.filter(|id| valid_uuid(id))
}

#[cfg(unix)]
fn open_plain(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_plain(path: &Path) -> std::io::Result<File> {
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    File::open(path)
}

fn read_file(path: &Path) -> std::io::Result<(Vec<u8>, Metadata)> {
    let mut file = open_plain(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    Ok((data, meta))
}

fn read_if_there(path: &Path) -> std::io::Result<Option<(Vec<u8>, Metadata)>> {
    match read_file(path) {
        Ok(read) => Ok(Some(read)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn read_plain(path: &Path) -> std::io::Result<Vec<u8>> {
    read_file(path).map(|(data, _)| data)
}

fn write_new(path: &Path, data: &[u8], mode: Option<&Permissions>) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    let written = file
        .write_all(data)
        .and_then(|()| mode.map_or(Ok(()), |mode| file.set_permissions(mode.clone())))
        .and_then(|()| file.sync_all());
    drop(file);
    if written.is_err() {
        let _ = std::fs::remove_file(path);
    }
    written
}

fn add_new(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let name = path.file_name().and_then(|name| name.to_str());
    let (Some(dir), Some(name)) = (path.parent(), name) else {
        return Err(std::io::ErrorKind::InvalidInput.into());
    };
    let aside = temp(dir, name, "new.tmp");
    let _ = std::fs::remove_file(&aside);
    let added = write_new(&aside, data, None).and_then(|()| std::fs::hard_link(&aside, path));
    let _ = std::fs::remove_file(&aside);
    added
}

fn remove_if_there(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[derive(Default)]
struct Temps(Vec<PathBuf>);

impl Temps {
    fn stage(
        &mut self,
        path: PathBuf,
        data: &[u8],
        mode: Option<&Permissions>,
    ) -> Result<PathBuf, Refused> {
        let _ = std::fs::remove_file(&path);
        write_new(&path, data, mode).map_err(|_| Refused::Failed)?;
        self.0.push(path.clone());
        Ok(path)
    }
}

impl Drop for Temps {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn settings(data: &[u8]) -> std::io::Result<Map<String, Value>> {
    match serde_json::from_slice(data) {
        Ok(Value::Object(root)) => Ok(root),
        _ => Err(std::io::ErrorKind::InvalidData.into()),
    }
}

fn steady<T: PartialEq>(
    settled: impl Fn() -> bool,
    read: impl Fn() -> std::io::Result<T>,
) -> std::io::Result<T> {
    let busy = || std::io::Error::from(std::io::ErrorKind::ResourceBusy);
    let still = || settled().then_some(()).ok_or_else(busy);
    still()?;
    let first = read()?;
    still()?;
    let second = read()?;
    still()?;
    (first == second).then_some(first).ok_or_else(busy)
}

fn found(claude_dir: &Path) -> std::io::Result<Found> {
    let unread = |_| std::io::Error::from(std::io::ErrorKind::InvalidData);
    let config = read_if_there(&config(claude_dir)).map_err(unread)?;
    let config = config.map(|(data, meta)| (data, meta.permissions()));
    let root = match &config {
        Some((data, _)) => settings(data)?,
        None => Map::new(),
    };
    let stamp = |meta: Metadata| (meta.len(), meta.modified().ok());
    let cookies = read_if_there(&cookies(claude_dir))?.map(|(data, meta)| (data, stamp(meta)));
    Ok(Found {
        config,
        root,
        cookies,
    })
}

fn whole(claude_dir: &Path) -> std::io::Result<Found> {
    steady(|| settled(claude_dir), || found(claude_dir))
}

impl Found {
    fn account(&self) -> Option<String> {
        let id = self.root.get(ACCOUNT).and_then(Value::as_str);
        id.map(str::to_string)
    }

    fn session(&self) -> Option<Session> {
        let held = self.cookies.as_ref().map(|(cookies, _)| cookies);
        held.filter(|cookies| !cookies.is_empty())
            .map(|cookies| Session {
                account: self.account(),
                cookies: cookies.clone(),
            })
    }

    fn still(&self, claude_dir: &Path) -> bool {
        settled(claude_dir) && found(claude_dir).is_ok_and(|now| now == *self)
    }

    fn plan(&self, claude_dir: &Path, account: Option<&str>) -> Result<Option<Config>, Refused> {
        let mut root = self.root.clone();
        match account {
            Some(account) => {
                root.insert(ACCOUNT.into(), json!(account));
                root.insert(
                    SIDE_SESSIONS.into(),
                    json!({"accountUuid": account, "supported": true}),
                );
            }
            None => {
                root.remove(ACCOUNT);
                root.remove(SIDE_SESSIONS);
            }
        }
        if root == self.root {
            return Ok(None);
        }
        let new = serde_json::to_vec(&Value::Object(root)).map_err(|_| Refused::Failed)?;
        Ok(Some(Config {
            path: config(claude_dir),
            new,
            old: self.config.clone(),
        }))
    }
}

pub fn session(claude_dir: &Path) -> std::io::Result<Option<Session>> {
    Ok(whole(claude_dir)?.session())
}

pub fn live(claude_dir: &Path) -> Option<(String, Vec<u8>)> {
    let session = session(claude_dir).ok()??;
    Some((session.account?, session.cookies))
}

pub fn save(mine: &Path, data: &[u8]) -> bool {
    let same = || read_plain(mine).is_ok_and(|kept| kept == data);
    match kind(mine) {
        Kind::Other => false,
        Kind::Plain(_) if same() => true,
        _ => mine.parent().is_some_and(|dir| {
            notchling_platform::files::ensure_private_dir(dir).is_ok()
                && notchling_platform::files::write_private_no_follow(mine, data).is_ok()
                && same()
        }),
    }
}

fn unnamed(dir: &Path) -> std::io::Result<(Numbered, Option<u64>)> {
    let mut plain = Numbered::new();
    let mut top = None;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let number = name.to_str().and_then(|name| name.strip_suffix(".cookies"));
        let Some(n) = number.and_then(|number| number.parse::<u64>().ok()) else {
            continue;
        };
        top = top.max(Some(n));
        if entry.file_type()?.is_file() {
            plain.push((n, entry.path()));
        }
    }
    plain.sort();
    Ok((plain, top))
}

fn numbered(dir: &Path, top: Option<u64>) -> Option<PathBuf> {
    let next = top.map_or(Some(1), |n| n.checked_add(1))?;
    Some(dir.join(format!("{next:012}.cookies")))
}

fn keep_unnamed(store_dir: &Path, data: &[u8], fresh: &mut Temps) -> Option<PathBuf> {
    let dir = store_dir.join(UNNAMED_DIR);
    notchling_platform::files::ensure_private_dir(&dir).ok()?;
    if !std::fs::symlink_metadata(&dir).is_ok_and(|meta| meta.is_dir()) {
        return None;
    }
    let (plain, top) = unnamed(&dir).ok()?;
    let same = |path: &Path| read_plain(path).is_ok_and(|kept| kept == data);
    if let Some((_, kept)) = plain.into_iter().rev().find(|(_, path)| same(path)) {
        return Some(kept);
    }
    let path = numbered(&dir, top)?;
    add_new(&path, data).ok()?;
    fresh.0.push(path.clone());
    same(&path).then_some(path)
}

fn own_copy(store_dir: &Path, session: &Session) -> Option<PathBuf> {
    named(session.account.as_deref()).map(|id| saved(store_dir, id))
}

fn keep(dirs: &Dirs, session: &Session, fresh: &mut Temps) -> Result<Kept, Refused> {
    let copy = match own_copy(dirs.store, session) {
        Some(mine) => save(&mine, &session.cookies).then_some(mine),
        None => keep_unnamed(dirs.store, &session.cookies, fresh),
    };
    let copy = copy.ok_or(Refused::Unkept)?;
    let first = backup(dirs.claude);
    if kind(&first) == Kind::Missing && add_new(&first, &session.cookies).is_ok() {
        fresh.0.push(first);
    }
    Ok(Kept {
        copy,
        account: session.account.clone(),
    })
}

fn newest(store_dir: &Path, kept: PathBuf, saved: Option<&Path>) -> PathBuf {
    let dir = store_dir.join(UNNAMED_DIR);
    let Ok((plain, top)) = unnamed(&dir) else {
        return kept;
    };
    let last = plain.last().filter(|(n, _)| Some(*n) == top);
    let stays = last.is_some_and(|(_, path)| same_file(path, &kept))
        || saved.is_some_and(|saved| same_file(saved, &kept));
    match numbered(&dir, top) {
        Some(next) if !stays && std::fs::hard_link(&kept, &next).is_ok() => {
            let _ = std::fs::remove_file(&kept);
            next
        }
        _ => kept,
    }
}

fn prune_unnamed(store_dir: &Path, used: &[&Path]) {
    let Ok((plain, _)) = unnamed(&store_dir.join(UNNAMED_DIR)) else {
        return;
    };
    let over = plain.len().saturating_sub(UNNAMED_KEPT);
    let held = |path: &Path| used.iter().any(|used| same_file(used, path));
    let unused = plain.iter().filter(|(_, path)| !held(path));
    for (_, old) in unused.take(over) {
        let _ = std::fs::remove_file(old);
    }
}

fn tidy(store_dir: &Path, kept: Option<Kept>, saved: Option<&Path>) -> Option<Kept> {
    let kept = kept.map(|kept| match named(kept.account.as_deref()) {
        Some(_) => kept,
        None => Kept {
            copy: newest(store_dir, kept.copy, saved),
            account: kept.account,
        },
    });
    let used = kept.iter().map(|kept| kept.copy.as_path());
    let used: Vec<&Path> = used.chain(saved).collect();
    prune_unnamed(store_dir, &used);
    kept
}

pub fn settled(claude_dir: &Path) -> bool {
    let holds = |name: &str| kind(&claude_dir.join(name));
    WRITE_AHEAD
        .iter()
        .all(|name| matches!(holds(name), Kind::Missing | Kind::Plain(0)))
        && holds(WAL_INDEX) != Kind::Other
}

fn clear_write_ahead(claude_dir: &Path) -> Result<(), Refused> {
    if !settled(claude_dir) {
        return Err(Refused::Unsettled);
    }
    WRITE_AHEAD
        .iter()
        .chain(&[WAL_INDEX])
        .try_for_each(|name| remove_if_there(&claude_dir.join(name)))
        .map_err(|_| Refused::Failed)
}

struct Config {
    path: PathBuf,
    new: Vec<u8>,
    old: Option<(Vec<u8>, Permissions)>,
}

struct Swap {
    path: PathBuf,
    new: PathBuf,
    old: Option<PathBuf>,
}

impl Config {
    fn stage(self, claude_dir: &Path, temps: &mut Temps) -> Result<Swap, Refused> {
        let mode = self.old.as_ref().map(|(_, mode)| mode);
        let new = temps.stage(temp(claude_dir, "config.json", "tmp"), &self.new, mode)?;
        let old = match &self.old {
            Some((data, mode)) => {
                let aside = temp(claude_dir, "config.json", "old.tmp");
                Some(temps.stage(aside, data, Some(mode))?)
            }
            None => None,
        };
        Ok(Swap {
            path: self.path,
            new,
            old,
        })
    }
}

impl Swap {
    fn apply(&self) -> std::io::Result<()> {
        std::fs::rename(&self.new, &self.path)
    }

    fn undo(&self) {
        let _ = match &self.old {
            Some(old) => std::fs::rename(old, &self.path),
            None => std::fs::remove_file(&self.path),
        };
    }
}

fn commit(swap: Option<&Swap>, last: impl FnOnce() -> std::io::Result<()>) -> Result<(), Refused> {
    swap.map_or(Ok(()), Swap::apply)
        .and_then(|()| {
            last().inspect_err(|_| {
                if let Some(swap) = swap {
                    swap.undo();
                }
            })
        })
        .map_err(|_| Refused::Failed)
}

struct Ready<'a> {
    dirs: &'a Dirs<'a>,
    saved: Option<&'a Path>,
    login: Found,
    session: Option<Session>,
    staged: Option<PathBuf>,
    swap: Option<Swap>,
    _temps: Temps,
}

fn refusal(e: &std::io::Error) -> Refused {
    match e.kind() {
        std::io::ErrorKind::InvalidData => Refused::Failed,
        std::io::ErrorKind::ResourceBusy => Refused::Unsettled,
        _ => Refused::Unkept,
    }
}

fn copy_of(saved: &Path) -> Result<Vec<u8>, Refused> {
    let data = read_plain(saved).ok().filter(|data| !data.is_empty());
    data.ok_or(Refused::Failed)
}

fn ready<'a>(
    dirs: &'a Dirs<'a>,
    to: Option<(&'a Path, Option<&str>)>,
) -> Result<Ready<'a>, Refused> {
    if !settled(dirs.claude) {
        return Err(Refused::Unsettled);
    }
    let (saved, account) = to.unzip();
    let data = saved.map(copy_of).transpose()?;
    let login = whole(dirs.claude).map_err(|e| refusal(&e))?;
    let config = login.plan(dirs.claude, account.flatten())?;
    let session = login.session();
    let own = session.as_ref().and_then(|live| own_copy(dirs.store, live));
    let in_use = own
        .zip(saved)
        .is_some_and(|(own, saved)| same_file(&own, saved));
    let live = session.as_ref().filter(|_| in_use);
    let data = live.map(|live| live.cookies.clone()).or(data);
    let mut temps = Temps::default();
    let aside = temp(dirs.claude, "Cookies", "notchling.tmp");
    let staged = data.map(|data| temps.stage(aside, &data, None));
    let staged = staged.transpose()?;
    let swap = config.map(|config| config.stage(dirs.claude, &mut temps));
    let swap = swap.transpose()?;
    Ok(Ready {
        dirs,
        saved,
        login,
        session,
        staged,
        swap,
        _temps: temps,
    })
}

impl Ready<'_> {
    fn land(self) -> Result<Option<Kept>, Refused> {
        let (claude, store) = (self.dirs.claude, self.dirs.store);
        if !self.login.still(claude) {
            return Err(Refused::Unsettled);
        }
        let mut fresh = Temps::default();
        let kept = self.session.as_ref();
        let kept = kept.map(|live| keep(self.dirs, live, &mut fresh));
        let kept = kept.transpose()?;
        clear_write_ahead(claude)?;
        let live = cookies(claude);
        commit(self.swap.as_ref(), || match &self.staged {
            Some(staged) => std::fs::rename(staged, &live),
            None => remove_if_there(&live),
        })?;
        fresh.0.clear();
        Ok(tidy(store, kept, self.saved))
    }
}

fn change(dirs: &Dirs, to: Option<(&Path, Option<&str>)>) -> Result<Option<Kept>, Refused> {
    ready(dirs, to)?.land()
}

pub fn restore(dirs: &Dirs, saved: &Path, account: Option<&str>) -> Result<Option<Kept>, Refused> {
    change(dirs, Some((saved, account)))
}

pub fn sign_out(dirs: &Dirs) -> Result<Option<Kept>, Refused> {
    change(dirs, None)
}

#[cfg(test)]
#[path = "files_land_tests.rs"]
mod land_tests;
#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
