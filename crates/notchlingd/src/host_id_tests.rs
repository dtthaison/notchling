use super::resolve;
use crate::limits_ops_tests::TempDir;
use std::path::Path;

fn write_certificate(bridge: &Path, name: &str) -> String {
    std::fs::create_dir_all(bridge).unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec![name.to_string()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    std::fs::write(bridge.join("cert.pem"), cert.pem()).unwrap();
    std::fs::write(bridge.join("key.pem"), key.serialize_pem()).unwrap();
    let identity = notchling_engine::bridge::net::identity::BridgeIdentity::load(bridge).unwrap();
    identity.fingerprint
}

#[test]
fn random_id_made_once_and_kept() {
    let tmp = TempDir::new("host-id-random");
    let bridge = tmp.path("bridge");
    let file = tmp.path("daemon/host-id");
    let (first, changed) = resolve(&bridge, &file).unwrap();
    assert_eq!(first.as_str().len(), 16);
    assert!(changed.is_none());
    let (again, changed) = resolve(&bridge, &file).unwrap();
    assert_eq!(again, first);
    assert!(changed.is_none());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), first.as_str());
}

#[cfg(unix)]
#[test]
fn the_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new("host-id-mode");
    let file = tmp.path("daemon/host-id");
    resolve(&tmp.path("bridge"), &file).unwrap();
    let mode = std::fs::metadata(&file).unwrap().permissions().mode();
    assert_eq!(mode & 0o077, 0, "{mode:o}");
}

#[test]
fn certificate_id_replaces_random_and_reports_change() {
    let tmp = TempDir::new("host-id-cert");
    let bridge = tmp.path("bridge");
    let file = tmp.path("daemon/host-id");
    let (random, _) = resolve(&bridge, &file).unwrap();
    let fingerprint = write_certificate(&bridge, "Notchling test");
    let (id, changed) = resolve(&bridge, &file).unwrap();
    assert_eq!(id.as_str(), &fingerprint[..16]);
    assert_eq!(changed, Some((random, id.clone())));
    let (again, changed) = resolve(&bridge, &file).unwrap();
    assert_eq!(again, id);
    assert!(changed.is_none());
}

#[test]
fn a_first_run_with_a_certificate_reports_no_change() {
    let tmp = TempDir::new("host-id-first-cert");
    let bridge = tmp.path("bridge");
    let file = tmp.path("daemon/host-id");
    let fingerprint = write_certificate(&bridge, "Notchling test");
    let (id, changed) = resolve(&bridge, &file).unwrap();
    assert_eq!(id.as_str(), &fingerprint[..16]);
    assert!(changed.is_none());
}

#[test]
fn a_new_certificate_changes_the_id_again() {
    let tmp = TempDir::new("host-id-new-cert");
    let bridge = tmp.path("bridge");
    let file = tmp.path("daemon/host-id");
    write_certificate(&bridge, "one");
    let (first, _) = resolve(&bridge, &file).unwrap();
    write_certificate(&bridge, "two");
    let (second, changed) = resolve(&bridge, &file).unwrap();
    assert_ne!(first, second);
    assert_eq!(changed, Some((first, second)));
}
