use super::*;
use crate::sessions::testutil::TempDir;
use std::path::PathBuf;

pub(crate) const PASSPHRASE: &str = "pass-0123456789abcdefghijklmnopqrstu";

pub(crate) fn make_swift_identity(bridge: &Path) -> Vec<u8> {
    std::fs::create_dir_all(bridge).unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["Agent Console (test)".to_string()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    let der = cert.der().to_vec();
    let pfx = p12::PFX::new(
        &der,
        &key.serialize_der(),
        None,
        PASSPHRASE,
        "Agent Console",
    )
    .unwrap();
    std::fs::write(bridge.join(P12), pfx.to_der()).unwrap();
    std::fs::write(bridge.join(PASS), PASSPHRASE).unwrap();
    std::fs::write(bridge.join(CERT_PEM), cert.pem()).unwrap();
    der
}

fn listing(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out: Vec<(PathBuf, u64)> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (e.path(), e.metadata().unwrap().len())
        })
        .collect();
    out.sort();
    out
}

#[test]
fn the_swift_apps_identity_loads_with_the_same_fingerprint() {
    let dir = TempDir::new("identity-p12");
    let bridge = dir.path("bridge");
    let der = make_swift_identity(&bridge);
    let before = listing(&bridge);
    assert!(BridgeIdentity::exists(&bridge));
    let identity = BridgeIdentity::load(&bridge).unwrap();
    assert_eq!(identity.fingerprint, sha256_hex(&der));
    assert_eq!(identity.certificate.as_ref(), &der[..], "byte for byte");
    let pem = CertificateDer::from_pem_file(bridge.join(CERT_PEM)).unwrap();
    assert_eq!(
        identity.fingerprint,
        sha256_hex(pem.as_ref()),
        "cert.pem agrees"
    );
    assert_eq!(listing(&bridge), before, "loading writes nothing");
}

#[test]
fn a_cert_and_key_pem_pair_loads_too() {
    let dir = TempDir::new("identity-pem");
    let bridge = dir.path("bridge");
    std::fs::create_dir_all(&bridge).unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["x".to_string()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    std::fs::write(bridge.join(CERT_PEM), cert.pem()).unwrap();
    std::fs::write(bridge.join(KEY_PEM), key.serialize_pem()).unwrap();
    let identity = BridgeIdentity::load(&bridge).unwrap();
    assert_eq!(identity.fingerprint, sha256_hex(cert.der()));
}

#[test]
fn no_identity_is_missing_and_nothing_is_made() {
    let dir = TempDir::new("identity-none");
    let bridge = dir.path("bridge");
    assert_eq!(
        BridgeIdentity::load(&bridge).unwrap_err(),
        IdentityError::Missing
    );
    assert!(!bridge.exists(), "never generated");
    std::fs::create_dir_all(&bridge).unwrap();
    std::fs::write(bridge.join(CERT_PEM), "x").unwrap();
    assert_eq!(
        BridgeIdentity::load(&bridge).unwrap_err(),
        IdentityError::Missing,
        "a cert alone is no identity"
    );
}

#[test]
fn a_wrong_passphrase_is_unreadable() {
    let dir = TempDir::new("identity-pass");
    let bridge = dir.path("bridge");
    make_swift_identity(&bridge);
    std::fs::write(bridge.join(PASS), "wrong").unwrap();
    assert!(matches!(
        BridgeIdentity::load(&bridge),
        Err(IdentityError::Unreadable(_))
    ));
    std::fs::write(bridge.join(P12), b"junk").unwrap();
    assert!(matches!(
        BridgeIdentity::load(&bridge),
        Err(IdentityError::Unreadable(_))
    ));
}

#[cfg(target_os = "macos")]
#[test]
fn an_identity_made_by_the_swift_apps_openssl_commands_loads() {
    let openssl = Path::new("/usr/bin/openssl");
    if !openssl.exists() {
        return;
    }
    let dir = TempDir::new("identity-openssl");
    let b = dir.path("bridge");
    std::fs::create_dir_all(&b).unwrap();
    let (key, cert, p12, pass) = (b.join(KEY_PEM), b.join(CERT_PEM), b.join(P12), b.join(PASS));
    std::fs::write(&pass, PASSPHRASE).unwrap();
    let run = |args: &[&str]| {
        let status = std::process::Command::new(openssl)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "{args:?}");
    };
    let s = |p: &PathBuf| p.to_string_lossy().into_owned();
    run(&[
        "ecparam",
        "-name",
        "prime256v1",
        "-genkey",
        "-noout",
        "-out",
        &s(&key),
    ]);
    run(&[
        "req",
        "-new",
        "-x509",
        "-key",
        &s(&key),
        "-out",
        &s(&cert),
        "-days",
        "3650",
        "-sha256",
        "-subj",
        "/CN=Agent Console (test)",
    ]);
    let passout = format!("file:{}", s(&pass));
    run(&[
        "pkcs12",
        "-export",
        "-inkey",
        &s(&key),
        "-in",
        &s(&cert),
        "-out",
        &s(&p12),
        "-passout",
        &passout,
        "-certpbe",
        "PBE-SHA1-3DES",
        "-keypbe",
        "PBE-SHA1-3DES",
        "-macalg",
        "sha1",
    ]);
    std::fs::remove_file(&key).unwrap();
    let identity = BridgeIdentity::load(&b).unwrap();
    let pem = CertificateDer::from_pem_file(&cert).unwrap();
    assert_eq!(identity.fingerprint, sha256_hex(pem.as_ref()));
    assert_eq!(served_fingerprint(&identity), identity.fingerprint);
}

fn served_fingerprint(identity: &BridgeIdentity) -> String {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{ServerName, UnixTime};
    use std::sync::Mutex;
    #[derive(Debug, Default)]
    struct Seen(Mutex<Vec<u8>>);
    impl ServerCertVerifier for Seen {
        fn verify_server_cert(
            &self,
            cert: &CertificateDer<'_>,
            _: &[CertificateDer<'_>],
            _: &rustls::pki_types::ServerName<'_>,
            _: &[u8],
            _: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            *self.0.lock().unwrap() = cert.to_vec();
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &rustls::DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &rustls::DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }
    let seen = Arc::new(Seen::default());
    let client_config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .dangerous()
    .with_custom_certificate_verifier(seen.clone())
    .with_no_client_auth();
    let mut client = rustls::ClientConnection::new(
        Arc::new(client_config),
        ServerName::try_from("bridge.local").unwrap(),
    )
    .unwrap();
    let mut server = rustls::ServerConnection::new(identity.server_config().unwrap()).unwrap();
    for _ in 0..10 {
        let mut buf = Vec::new();
        client.write_tls(&mut buf).unwrap();
        server.read_tls(&mut &buf[..]).unwrap();
        server.process_new_packets().unwrap();
        let mut buf = Vec::new();
        server.write_tls(&mut buf).unwrap();
        client.read_tls(&mut &buf[..]).unwrap();
        client.process_new_packets().unwrap();
        if !client.is_handshaking() && !server.is_handshaking() {
            break;
        }
    }
    assert!(!server.is_handshaking());
    let seen = seen.0.lock().unwrap().clone();
    sha256_hex(&seen)
}

#[test]
fn the_identity_is_served_in_a_handshake() {
    let dir = TempDir::new("identity-serve");
    let bridge = dir.path("bridge");
    let der = make_swift_identity(&bridge);
    let identity = BridgeIdentity::load(&bridge).unwrap();
    assert_eq!(served_fingerprint(&identity), sha256_hex(&der));
}
