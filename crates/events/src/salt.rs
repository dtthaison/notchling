use crate::field::Hex;
use crate::field_ref::Salt;
use crate::ids::random_bytes;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaltChange {
    Same,
    Created,
    Replaced { old_id: Option<Hex> },
}

enum Existing {
    Missing,
    Good(Salt),
    Bad(Option<Hex>),
}

fn refuse_link(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the salt file is a symlink",
        )),
        _ => Ok(()),
    }
}

fn existing(path: &Path) -> io::Result<Existing> {
    refuse_link(path)?;
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Existing::Missing),
        Err(_) => return Ok(Existing::Bad(None)),
    };
    let mut bytes = Vec::new();
    if (&mut file).take(65).read_to_end(&mut bytes).is_err() {
        return Ok(Existing::Bad(None));
    }
    match <[u8; 32]>::try_from(bytes.as_slice()) {
        Ok(raw) => Ok(Existing::Good(Salt(raw))),
        Err(_) => Ok(Existing::Bad(None)),
    }
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn create_new(path: &Path, raw: &[u8; 32]) -> io::Result<()> {
    let mut file = private_options().create_new(true).open(path)?;
    file.write_all(raw)?;
    file.sync_all()
}

fn tmp_path(path: &Path) -> PathBuf {
    path.with_file_name("log-salt.tmp")
}

fn replace(path: &Path, raw: &[u8; 32]) -> io::Result<()> {
    let tmp = tmp_path(path);
    let _ = fs::remove_file(&tmp);
    create_new(&tmp, raw)?;
    fs::rename(&tmp, path)
}

fn no_random() -> io::Error {
    io::Error::other("the OS gave no random bytes")
}

pub fn load_or_create(path: &Path) -> io::Result<(Salt, SaltChange)> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    for _ in 0..3 {
        match existing(path)? {
            Existing::Good(salt) => return Ok((salt, SaltChange::Same)),
            Existing::Missing => {
                let raw = random_bytes::<32>().ok_or_else(no_random)?;
                match create_new(path, &raw) {
                    Ok(()) => return Ok((Salt(raw), SaltChange::Created)),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e),
                }
            }
            Existing::Bad(old_id) => {
                let raw = random_bytes::<32>().ok_or_else(no_random)?;
                replace(path, &raw)?;
                return Ok((Salt(raw), SaltChange::Replaced { old_id }));
            }
        }
    }
    Err(io::Error::other("the salt file kept changing"))
}

pub fn read(path: &Path) -> Option<Salt> {
    match existing(path) {
        Ok(Existing::Good(salt)) => Some(salt),
        _ => None,
    }
}
