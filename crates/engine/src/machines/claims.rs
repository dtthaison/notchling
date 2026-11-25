use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

pub const HELD: &str = "another Notchling process is already changing that machine";

#[derive(Clone, Debug)]
pub struct Claims {
    dir: PathBuf,
}

#[derive(Debug)]
pub struct Claim {
    _file: Option<File>,
}

impl Claims {
    pub fn in_data(data: &Path) -> Claims {
        Claims {
            dir: data.join("daemon").join("machines").join("claims"),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file_of(&self, machine: &str) -> PathBuf {
        let digest = Sha256::digest(machine.to_ascii_lowercase().as_bytes());
        let name: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
        self.dir.join(format!("{name}.lock"))
    }

    pub fn take(&self, machine: &str) -> Result<Claim, String> {
        fs::create_dir_all(&self.dir).map_err(|e| format!("can't make the claims folder: {e}"))?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.file_of(machine))
            .map_err(|e| format!("can't open the claim file: {e}"))?;
        lock(&file)?;
        Ok(Claim { _file: Some(file) })
    }
}

#[cfg(unix)]
fn lock(file: &File) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        Ok(())
    } else {
        Err(HELD.to_string())
    }
}

#[cfg(not(unix))]
fn lock(_file: &File) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nl-claims-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_second_claim_on_one_machine_is_refused_until_the_first_ends() {
        let dir = temp("two");
        let a = Claims::in_data(&dir);
        let b = Claims::in_data(&dir);
        let first = a.take("m1").unwrap();
        assert_eq!(b.take("M1").unwrap_err(), HELD);
        assert!(b.take("m2").is_ok());
        drop(first);
        assert!(b.take("m1").is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn claims_live_under_the_daemon_machines_folder() {
        let c = Claims::in_data(Path::new("/tmp/x"));
        assert!(c.dir().ends_with("daemon/machines/claims"));
    }
}
