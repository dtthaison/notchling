//! `BridgeIdentity`: the TLS certificate and key the LAN port presents, read byte for byte from the
//! Swift app's files in `AgentConsole/bridge/`, so a paired phone's pinned fingerprint (SHA-256 of
//! the DER certificate, from the QR) still matches.
//!
//! The Swift app keeps the key only inside `identity.p12` (EC P-256, PBE-SHA1-3DES, made with
//! `/usr/bin/openssl`), with its passphrase in `identity.pass`; it deletes `key.pem` after making
//! it. So that pair is read first. `cert.pem` with a `key.pem` beside it is read too, for a folder
//! that has them. Nothing here ever makes or replaces a certificate: with no identity the bridge
//! doesn't start and says why.

use super::crypto::sha256_hex;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls_pki_types::pem::PemObject;
use std::path::Path;
use std::sync::Arc;

pub const P12: &str = "identity.p12";
pub const PASS: &str = "identity.pass";
pub const CERT_PEM: &str = "cert.pem";
pub const KEY_PEM: &str = "key.pem";

pub struct BridgeIdentity {
    pub certificate: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    /// SHA-256 of the DER certificate, lowercase hex.
    pub fingerprint: String,
}

impl std::fmt::Debug for BridgeIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BridgeIdentity")
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum IdentityError {
    /// No identity in the folder: the Swift app makes it the first time its bridge starts.
    Missing,
    Unreadable(String),
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdentityError::Missing => f.write_str(
                "no bridge certificate yet: the Swift app makes it when its phone bridge first starts",
            ),
            IdentityError::Unreadable(why) => write!(f, "the bridge certificate can't be read: {why}"),
        }
    }
}

impl std::error::Error for IdentityError {}

fn unreadable(why: impl std::fmt::Display) -> IdentityError {
    IdentityError::Unreadable(why.to_string())
}

impl BridgeIdentity {
    /// Whether the folder holds an identity (reading nothing but the names).
    pub fn exists(dir: &Path) -> bool {
        dir.join(P12).is_file() || (dir.join(CERT_PEM).is_file() && dir.join(KEY_PEM).is_file())
    }

    /// Loads the identity from `dir` (`AgentConsole/bridge`), writing nothing.
    pub fn load(dir: &Path) -> Result<BridgeIdentity, IdentityError> {
        if dir.join(P12).is_file() {
            return Self::from_p12(dir);
        }
        if dir.join(CERT_PEM).is_file() && dir.join(KEY_PEM).is_file() {
            return Self::from_pem(dir);
        }
        Err(IdentityError::Missing)
    }

    fn from_p12(dir: &Path) -> Result<BridgeIdentity, IdentityError> {
        let der = std::fs::read(dir.join(P12)).map_err(unreadable)?;
        let pass = std::fs::read_to_string(dir.join(PASS)).map_err(unreadable)?;
        let pfx = p12::PFX::parse(&der).map_err(|e| unreadable(format!("{P12}: {e:?}")))?;
        // The passphrase as Swift hands it to SecPKCS12Import: the file as it is.
        let pass = [pass.as_str(), pass.trim_end()]
            .into_iter()
            .find(|p| pfx.verify_mac(p))
            .ok_or_else(|| unreadable(format!("{PASS} doesn't open {P12}")))?;
        let cert = pfx
            .cert_x509_bags(pass)
            .map_err(|e| unreadable(format!("{e:?}")))?
            .into_iter()
            .next()
            .ok_or_else(|| unreadable("no certificate in identity.p12"))?;
        let key = pfx
            .key_bags(pass)
            .map_err(|e| unreadable(format!("{e:?}")))?
            .into_iter()
            .next()
            .ok_or_else(|| unreadable("no key in identity.p12"))?;
        Self::new(
            CertificateDer::from(cert),
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
        )
    }

    fn from_pem(dir: &Path) -> Result<BridgeIdentity, IdentityError> {
        let cert = CertificateDer::from_pem_file(dir.join(CERT_PEM)).map_err(unreadable)?;
        let key = PrivateKeyDer::from_pem_file(dir.join(KEY_PEM)).map_err(unreadable)?;
        Self::new(cert, key)
    }

    fn new(
        certificate: CertificateDer<'static>,
        key: PrivateKeyDer<'static>,
    ) -> Result<BridgeIdentity, IdentityError> {
        let identity = BridgeIdentity {
            fingerprint: sha256_hex(certificate.as_ref()),
            certificate,
            key,
        };
        identity.server_config()?;
        Ok(identity)
    }

    /// The TLS server settings: TLS 1.2 and 1.3 with ring, no client certificates.
    ///
    /// The certificate is served as it is, never parsed: the Swift app's comes from macOS's
    /// LibreSSL `req -x509`, which makes an X.509 v1 certificate, and rustls's own check of a
    /// certificate against its key (webpki) refuses v1. The phone pins the fingerprint instead.
    pub fn server_config(&self) -> Result<Arc<rustls::ServerConfig>, IdentityError> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let signer =
            rustls::crypto::ring::sign::any_supported_type(&self.key).map_err(unreadable)?;
        let certified = rustls::sign::CertifiedKey::new(vec![self.certificate.clone()], signer);
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
            .map_err(unreadable)?
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(OneCertificate(Arc::new(certified))));
        Ok(Arc::new(config))
    }
}

/// Serves the one certificate to every client.
#[derive(Debug)]
struct OneCertificate(Arc<rustls::sign::CertifiedKey>);

impl rustls::server::ResolvesServerCert for OneCertificate {
    fn resolve(
        &self,
        _hello: rustls::server::ClientHello<'_>,
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        Some(self.0.clone())
    }
}

#[cfg(test)]
#[path = "identity_tests.rs"]
pub(crate) mod tests;
