use crate::{fingerprint, Error};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::fmt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const CERT_FILE: &str = "node.pem";
pub const KEY_FILE: &str = "node.key";
const COMMON_NAME: &str = "notchling-node";
const YEARS: i32 = 20;

#[derive(Clone)]
pub struct Identity {
    pub cert_der: Vec<u8>,
    pub fingerprint: String,
    key_der: Vec<u8>,
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Identity({})", self.fingerprint)
    }
}

impl Identity {
    pub(crate) fn private_key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone()))
    }

    pub(crate) fn cert(&self) -> CertificateDer<'static> {
        CertificateDer::from(self.cert_der.clone())
    }
}

fn bad(why: impl Into<String>) -> Error {
    Error::Identity(why.into())
}

fn plain_file(path: &Path) -> Result<bool, Error> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err(bad(format!("{} is a link", path.display())))
        }
        Ok(meta) if meta.is_file() => Ok(true),
        Ok(_) => Err(bad(format!("{} is not a file", path.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(Error::Io(e)),
    }
}

pub fn load_or_make(dir: &Path) -> Result<Identity, Error> {
    notchling_platform::files::ensure_private_dir(dir).map_err(|e| bad(e.to_string()))?;
    let (cert_path, key_path) = (dir.join(CERT_FILE), dir.join(KEY_FILE));
    match (plain_file(&cert_path)?, plain_file(&key_path)?) {
        (true, true) => load(&cert_path, &key_path),
        (false, false) => make(&cert_path, &key_path),
        _ => Err(bad(
            "half of the node identity is missing; it is never replaced",
        )),
    }
}

fn load(cert_path: &Path, key_path: &Path) -> Result<Identity, Error> {
    let cert = CertificateDer::from_pem_file(cert_path)
        .map_err(|e| bad(format!("{} doesn't read: {e}", cert_path.display())))?;
    let key = PrivateKeyDer::from_pem_file(key_path)
        .map_err(|e| bad(format!("{} doesn't read: {e}", key_path.display())))?;
    let PrivateKeyDer::Pkcs8(key) = key else {
        return Err(bad("the node key is not PKCS#8"));
    };
    let key_der = key.secret_pkcs8_der().to_vec();
    rcgen::KeyPair::try_from(key_der.as_slice())
        .map_err(|e| bad(format!("the node key doesn't read: {e}")))?;
    let cert_der = cert.as_ref().to_vec();
    Ok(Identity {
        fingerprint: fingerprint(&cert_der),
        cert_der,
        key_der,
    })
}

fn this_year() -> i32 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    1970 + (secs / 31_556_952) as i32
}

fn make(cert_path: &Path, key_path: &Path) -> Result<Identity, Error> {
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
        .map_err(|e| bad(e.to_string()))?;
    let mut params =
        rcgen::CertificateParams::new(Vec::<String>::new()).map_err(|e| bad(e.to_string()))?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, COMMON_NAME);
    let year = this_year();
    params.not_before = rcgen::date_time_ymd(year - 1, 1, 1);
    params.not_after = rcgen::date_time_ymd(year + YEARS, 1, 1);
    let cert = params.self_signed(&key).map_err(|e| bad(e.to_string()))?;
    notchling_platform::files::write_private_no_follow(key_path, key.serialize_pem().as_bytes())
        .map_err(|e| bad(e.to_string()))?;
    notchling_platform::files::write_private_no_follow(cert_path, cert.pem().as_bytes())
        .map_err(|e| bad(e.to_string()))?;
    let cert_der = cert.der().as_ref().to_vec();
    Ok(Identity {
        fingerprint: fingerprint(&cert_der),
        cert_der,
        key_der: key.serialize_der(),
    })
}
