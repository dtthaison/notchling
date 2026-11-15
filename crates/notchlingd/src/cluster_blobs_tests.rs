use super::{wipe, Blobs, MAX_OPEN, STALE};
use crate::cluster_testkit::Dir;
use notchling_engine::bridge::net::crypto::base64;
use notchling_engine::cluster::blob::sha256_of;
use notchling_local_api::ErrorCode;
use serde_json::{json, Value};
use std::time::SystemTime;

fn put(transfer: &str, offset: u64, data: &[u8]) -> Value {
    json!({"transfer": transfer, "offset": offset, "data": base64(data)})
}

fn end(transfer: &str, data: &[u8]) -> Value {
    json!({"transfer": transfer, "sha256": sha256_of(data)})
}

fn cluster(dir: &Dir) -> std::path::PathBuf {
    dir.path().join("cluster")
}

#[test]
fn a_file_arrives_in_chunks_and_is_kept_private() {
    let dir = Dir::new("blobs-ok");
    let blobs = Blobs::new(cluster(&dir));
    assert_eq!(blobs.put(&put("t1", 0, b"hello ")).unwrap()["next"], 6);
    assert_eq!(blobs.put(&put("t1", 6, b"world")).unwrap()["next"], 11);
    let done = blobs.end(&end("t1", b"hello world")).unwrap();
    assert_eq!(done["size"], 11);
    let path = blobs.finished("t1").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"hello world");
    assert!(!cluster(&dir).join("incoming/t1.part").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&cluster(&dir).join("incoming")), 0o700);
    }
}

#[test]
fn a_changed_file_is_dropped() {
    let dir = Dir::new("blobs-hash");
    let blobs = Blobs::new(cluster(&dir));
    blobs.put(&put("t1", 0, b"abc")).unwrap();
    let err = blobs.end(&end("t1", b"abd")).unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    assert!(err.message.contains("hash"), "{}", err.message);
    assert!(blobs.finished("t1").is_err());
    assert!(!cluster(&dir).join("incoming/t1.part").exists());
    assert!(blobs.end(&end("t1", b"abc")).is_err());
}

#[test]
fn chunks_out_of_order_stop_the_transfer() {
    let dir = Dir::new("blobs-order");
    let blobs = Blobs::new(cluster(&dir));
    blobs.put(&put("t1", 0, b"abc")).unwrap();
    let err = blobs.put(&put("t1", 9, b"x")).unwrap_err();
    assert!(err.message.contains("out of order"), "{}", err.message);
    assert!(!cluster(&dir).join("incoming/t1.part").exists());
    assert!(blobs.put(&put("t1", 3, b"def")).is_err());
    assert!(blobs.put(&put("never", 5, b"x")).is_err());
}

#[test]
fn a_file_over_the_cap_is_refused() {
    let dir = Dir::new("blobs-cap");
    let blobs = Blobs::with_cap(cluster(&dir), 8);
    blobs.put(&put("t1", 0, b"12345")).unwrap();
    let err = blobs.put(&put("t1", 5, b"6789")).unwrap_err();
    assert!(err.message.contains("over"), "{}", err.message);
    assert!(!cluster(&dir).join("incoming/t1.part").exists());
}

#[test]
fn transfer_names_cannot_leave_the_folder() {
    let dir = Dir::new("blobs-names");
    let blobs = Blobs::new(cluster(&dir));
    for name in ["../x", "a/b", "-x", "a.part", "", "x y", "a\nb"] {
        assert!(blobs.put(&put(name, 0, b"x")).is_err(), "{name:?}");
        assert!(blobs.end(&end(name, b"x")).is_err(), "{name:?}");
        assert!(blobs.finished(name).is_err(), "{name:?}");
    }
    assert!(!dir.path().join("x").exists());
    assert!(blobs
        .put(&json!({"transfer": "t", "offset": 0, "data": "!!"}))
        .is_err());
    assert!(blobs.put(&json!({"transfer": "t", "data": ""})).is_err());
}

#[cfg(unix)]
#[test]
fn a_linked_incoming_folder_is_never_written_through() {
    let dir = Dir::new("blobs-link");
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::create_dir_all(cluster(&dir)).unwrap();
    std::os::unix::fs::symlink(&elsewhere, cluster(&dir).join("incoming")).unwrap();
    let blobs = Blobs::new(cluster(&dir));
    assert!(blobs.put(&put("t1", 0, b"abc")).is_err());
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);

    let other = Dir::new("blobs-link2");
    let real = other.path().join("real");
    std::fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(&real, other.path().join("cluster")).unwrap();
    let blobs = Blobs::new(other.path().join("cluster"));
    assert!(blobs.put(&put("t1", 0, b"abc")).is_err());
    assert_eq!(std::fs::read_dir(&real).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn a_planted_part_file_link_is_replaced_not_followed() {
    let dir = Dir::new("blobs-part");
    let blobs = Blobs::new(cluster(&dir));
    blobs.put(&put("warm", 0, b"x")).unwrap();
    let victim = dir.path().join("victim");
    std::fs::write(&victim, b"keep").unwrap();
    std::os::unix::fs::symlink(&victim, cluster(&dir).join("incoming/t1.part")).unwrap();
    blobs.put(&put("t1", 0, b"abc")).unwrap();
    blobs.end(&end("t1", b"abc")).unwrap();
    assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
    assert_eq!(
        std::fs::read(blobs.finished("t1").unwrap()).unwrap(),
        b"abc"
    );
}

#[cfg(unix)]
#[test]
fn a_linked_finished_file_is_not_served() {
    let dir = Dir::new("blobs-served");
    let blobs = Blobs::new(cluster(&dir));
    blobs.put(&put("warm", 0, b"x")).unwrap();
    let secret = dir.path().join("secret");
    std::fs::write(&secret, b"no").unwrap();
    std::os::unix::fs::symlink(&secret, cluster(&dir).join("incoming/t2")).unwrap();
    assert!(blobs.finished("t2").is_err());
}

#[test]
fn old_parts_are_swept() {
    let dir = Dir::new("blobs-sweep");
    let blobs = Blobs::new(cluster(&dir));
    blobs.put(&put("old", 0, b"x")).unwrap();
    blobs.put(&put("fresh", 0, b"x")).unwrap();
    let later = SystemTime::now() + STALE + std::time::Duration::from_secs(60);
    let old = cluster(&dir).join("incoming/old.part");
    let file = std::fs::OpenOptions::new().write(true).open(&old).unwrap();
    file.set_modified(SystemTime::now() - STALE - std::time::Duration::from_secs(60))
        .unwrap();
    blobs.sweep(SystemTime::now());
    assert!(!old.exists());
    assert!(cluster(&dir).join("incoming/fresh.part").exists());
    assert!(blobs.put(&put("old", 1, b"y")).is_err());
    blobs.sweep(later);
    assert!(!cluster(&dir).join("incoming/fresh.part").exists());
}

#[test]
fn only_a_few_files_arrive_at_once() {
    let dir = Dir::new("blobs-many");
    let blobs = Blobs::new(cluster(&dir));
    for n in 0..MAX_OPEN {
        blobs.put(&put(&format!("t{n}"), 0, b"x")).unwrap();
    }
    assert!(blobs.put(&put("extra", 0, b"x")).is_err());
    blobs.put(&put("t0", 0, b"again")).unwrap();
    blobs.discard("t1");
    blobs.put(&put("extra", 0, b"x")).unwrap();
}

#[test]
fn wipe_removes_incoming_and_outgoing() {
    let dir = Dir::new("blobs-wipe");
    let blobs = Blobs::new(cluster(&dir));
    blobs.put(&put("t1", 0, b"abc")).unwrap();
    std::fs::create_dir_all(cluster(&dir).join("outgoing")).unwrap();
    wipe(&cluster(&dir)).unwrap();
    assert!(!cluster(&dir).join("incoming").exists());
    assert!(!cluster(&dir).join("outgoing").exists());
    wipe(&cluster(&dir)).unwrap();
}
