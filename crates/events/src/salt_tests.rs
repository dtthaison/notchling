use crate::ids::new_op_id;
use crate::salt::{load_or_create, read, SaltChange};
use std::fs;
use std::path::PathBuf;

struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        let dir = std::env::temp_dir().join(format!("nl-salt-{}", new_op_id().as_str()));
        fs::create_dir_all(&dir).expect("temp dir");
        Dir(dir)
    }

    fn file(&self) -> PathBuf {
        self.0.join("log-salt")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn creates_a_32_byte_0600_salt() {
    let dir = Dir::new();
    let (salt, change) = load_or_create(&dir.file()).expect("creates");
    assert_eq!(change, SaltChange::Created);
    let bytes = fs::read(dir.file()).expect("reads");
    assert_eq!(bytes.len(), 32);
    assert_eq!(bytes.as_slice(), salt.0.as_slice());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(dir.file()).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn same_salt_on_second_load() {
    let dir = Dir::new();
    let (first, _) = load_or_create(&dir.file()).expect("creates");
    let (second, change) = load_or_create(&dir.file()).expect("loads");
    assert_eq!(change, SaltChange::Same);
    assert_eq!(first, second);
    assert_eq!(read(&dir.file()), Some(first));
}

#[test]
fn short_file_is_replaced_and_reported() {
    let dir = Dir::new();
    fs::write(dir.file(), b"short").expect("writes");
    let (salt, change) = load_or_create(&dir.file()).expect("replaces");
    assert!(matches!(change, SaltChange::Replaced { .. }));
    assert_eq!(fs::read(dir.file()).expect("reads").as_slice(), &salt.0);
    assert!(!dir.0.join("log-salt.tmp").exists());
    assert_eq!(read(&dir.file()), Some(salt));
}

#[test]
fn missing_file_reads_as_none() {
    let dir = Dir::new();
    assert_eq!(read(&dir.file()), None);
}

#[cfg(unix)]
#[test]
fn symlink_in_its_place_is_refused() {
    let dir = Dir::new();
    let target = dir.0.join("elsewhere");
    fs::write(&target, [7u8; 32]).expect("writes");
    std::os::unix::fs::symlink(&target, dir.file()).expect("links");
    assert!(load_or_create(&dir.file()).is_err());
    assert_eq!(read(&dir.file()), None);
    assert_eq!(fs::read(&target).expect("reads"), vec![7u8; 32]);
}
