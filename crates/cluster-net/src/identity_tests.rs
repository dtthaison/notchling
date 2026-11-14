use super::*;
use crate::testutil::Scratch;

#[test]
fn identity_is_made_once_and_kept() {
    let scratch = Scratch::new();
    let dir = scratch.path().join("node");
    let first = load_or_make(&dir).unwrap();
    let second = load_or_make(&dir).unwrap();
    assert_eq!(first.fingerprint, second.fingerprint);
    assert_eq!(first.cert_der, second.cert_der);
    assert_eq!(first.fingerprint, crate::fingerprint(&first.cert_der));
    assert_eq!(first.fingerprint.len(), 64);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join(CERT_FILE)), 0o600);
        assert_eq!(mode(&dir.join(KEY_FILE)), 0o600);
    }
}

#[test]
fn unreadable_identity_is_an_error_not_replaced() {
    let scratch = Scratch::new();
    let dir = scratch.path().join("node");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(CERT_FILE), b"not a certificate").unwrap();
    std::fs::write(dir.join(KEY_FILE), b"not a key").unwrap();
    assert!(load_or_make(&dir).is_err());
    assert_eq!(
        std::fs::read(dir.join(CERT_FILE)).unwrap(),
        b"not a certificate"
    );
    assert_eq!(std::fs::read(dir.join(KEY_FILE)).unwrap(), b"not a key");
}

#[test]
fn half_an_identity_is_an_error() {
    let scratch = Scratch::new();
    let dir = scratch.path().join("node");
    load_or_make(&dir).unwrap();
    std::fs::remove_file(dir.join(KEY_FILE)).unwrap();
    let cert = std::fs::read(dir.join(CERT_FILE)).unwrap();
    assert!(load_or_make(&dir).is_err());
    assert_eq!(std::fs::read(dir.join(CERT_FILE)).unwrap(), cert);
    assert!(!dir.join(KEY_FILE).exists());
}

#[cfg(unix)]
#[test]
fn link_in_place_is_refused() {
    let scratch = Scratch::new();
    let dir = scratch.path().join("node");
    load_or_make(&dir).unwrap();
    let elsewhere = scratch.path().join("elsewhere.pem");
    std::fs::rename(dir.join(CERT_FILE), &elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, dir.join(CERT_FILE)).unwrap();
    assert!(load_or_make(&dir).is_err());
}

#[test]
fn debug_shows_the_fingerprint_only() {
    let scratch = Scratch::new();
    let id = load_or_make(&scratch.path().join("node")).unwrap();
    let text = format!("{id:?}");
    assert!(text.contains(&id.fingerprint));
    assert!(!text.contains("key"), "{text}");
}

#[test]
fn two_folders_make_two_identities() {
    let scratch = Scratch::new();
    let a = load_or_make(&scratch.path().join("a")).unwrap();
    let b = load_or_make(&scratch.path().join("b")).unwrap();
    assert_ne!(a.fingerprint, b.fingerprint);
}
