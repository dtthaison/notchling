use super::checkpoint::Checkpoint;
use notchling_events::ids::random_bytes;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

pub const KEY_FILE: &str = "audit-key";
pub const HEAD_FILE: &str = "audit-head";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyRead {
    Found([u8; 32]),
    Missing,
    Denied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    Denied,
    Io(io::ErrorKind),
    NoRandom,
}

pub trait KeyStore: Send + Sync {
    fn read_key(&self) -> KeyRead;
    fn create_key(&self) -> Result<[u8; 32], KeyError>;
    fn read_head(&self) -> Option<Checkpoint>;
    fn write_head(&self, checkpoint: &Checkpoint) -> Result<(), KeyError>;
    fn head_exists(&self) -> bool;
}

pub fn account_for(data_dir: &Path) -> String {
    let path = fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    notchling_events::hex_lower(&digest[..8])
}

#[derive(Default)]
struct Memory {
    key: Option<[u8; 32]>,
    head: Option<Checkpoint>,
    deny: bool,
    block: Duration,
}

#[derive(Default)]
pub struct MemoryKeyStore {
    inner: Mutex<Memory>,
}

impl MemoryKeyStore {
    pub fn new() -> MemoryKeyStore {
        MemoryKeyStore::default()
    }

    pub fn with_key(key: [u8; 32]) -> MemoryKeyStore {
        let store = MemoryKeyStore::default();
        store.lock().key = Some(key);
        store
    }

    fn lock(&self) -> MutexGuard<'_, Memory> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn deny_reads(&self, deny: bool) {
        self.lock().deny = deny;
    }

    pub fn block_reads(&self, wait: Duration) {
        self.lock().block = wait;
    }

    pub fn set_key(&self, key: Option<[u8; 32]>) {
        self.lock().key = key;
    }

    pub fn key(&self) -> Option<[u8; 32]> {
        self.lock().key
    }
}

impl KeyStore for MemoryKeyStore {
    fn read_key(&self) -> KeyRead {
        let (key, deny, block) = {
            let inner = self.lock();
            (inner.key, inner.deny, inner.block)
        };
        if !block.is_zero() {
            std::thread::sleep(block);
        }
        match (deny, key) {
            (true, _) => KeyRead::Denied,
            (false, Some(key)) => KeyRead::Found(key),
            (false, None) => KeyRead::Missing,
        }
    }

    fn create_key(&self) -> Result<[u8; 32], KeyError> {
        let key = random_bytes::<32>().ok_or(KeyError::NoRandom)?;
        self.lock().key = Some(key);
        Ok(key)
    }

    fn read_head(&self) -> Option<Checkpoint> {
        self.lock().head.clone()
    }

    fn write_head(&self, checkpoint: &Checkpoint) -> Result<(), KeyError> {
        self.lock().head = Some(checkpoint.clone());
        Ok(())
    }

    fn head_exists(&self) -> bool {
        self.lock().head.is_some()
    }
}

pub struct FileKeyStore {
    dir: PathBuf,
}

fn open_nofollow(path: &Path) -> io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

fn create_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn io_error(e: io::Error) -> KeyError {
    KeyError::Io(e.kind())
}

impl FileKeyStore {
    pub fn new(dir: impl Into<PathBuf>) -> FileKeyStore {
        FileKeyStore { dir: dir.into() }
    }

    fn key_path(&self) -> PathBuf {
        self.dir.join(KEY_FILE)
    }

    fn head_path(&self) -> PathBuf {
        self.dir.join(HEAD_FILE)
    }

    fn read_all(path: &Path) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        open_nofollow(path)?.take(1 << 16).read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

impl KeyStore for FileKeyStore {
    fn read_key(&self) -> KeyRead {
        match FileKeyStore::read_all(&self.key_path()) {
            Ok(bytes) => match <[u8; 32]>::try_from(bytes.as_slice()) {
                Ok(key) => KeyRead::Found(key),
                Err(_) => KeyRead::Denied,
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => KeyRead::Missing,
            Err(_) => KeyRead::Denied,
        }
    }

    fn create_key(&self) -> Result<[u8; 32], KeyError> {
        let key = random_bytes::<32>().ok_or(KeyError::NoRandom)?;
        fs::create_dir_all(&self.dir).map_err(io_error)?;
        create_private(&self.key_path(), &key).map_err(io_error)?;
        Ok(key)
    }

    fn read_head(&self) -> Option<Checkpoint> {
        let bytes = FileKeyStore::read_all(&self.head_path()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn write_head(&self, checkpoint: &Checkpoint) -> Result<(), KeyError> {
        let bytes =
            serde_json::to_vec(checkpoint).map_err(|_| KeyError::Io(io::ErrorKind::InvalidData))?;
        fs::create_dir_all(&self.dir).map_err(io_error)?;
        let tmp = self.dir.join("audit-head.tmp");
        let _ = fs::remove_file(&tmp);
        create_private(&tmp, &bytes).map_err(io_error)?;
        fs::rename(&tmp, self.head_path()).map_err(io_error)
    }

    fn head_exists(&self) -> bool {
        fs::symlink_metadata(self.head_path()).is_ok()
    }
}
