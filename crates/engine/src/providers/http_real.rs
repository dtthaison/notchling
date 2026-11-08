//! The real [`HttpClient`]: one blocking HTTPS request over rustls (ring), no redirects, 4 MB at
//! most, the whole exchange within the request's timeout.
//!
//! - [`Trust::System`]: the operating system's root certificates, a plain host name on port 443.
//! - [`Trust::LoopbackSelfSigned`]: Antigravity's language server, which serves a self-signed
//!   certificate. Any certificate is accepted, but only for `https://127.0.0.1:<port>`; any other
//!   host is refused before a socket opens.
//!
//! Errors say what failed in a few fixed words; none holds a header, a body or a token.

use super::http::{HttpClient, HttpRequest, HttpResponse, Trust};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_BODY: usize = 4 * 1024 * 1024;

/// The client the daemon uses.
pub struct TlsHttp;

impl HttpClient for TlsHttp {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        let target = Target::parse(&request.url, request.trust)?;
        let deadline = Instant::now() + request.timeout;
        let config = client_config(request.trust)?;
        let name = match &target.host {
            Host::Loopback => rustls::pki_types::ServerName::IpAddress(
                std::net::IpAddr::V4(Ipv4Addr::LOCALHOST).into(),
            ),
            Host::Name(n) => rustls::pki_types::ServerName::try_from(n.clone())
                .map_err(|_| "bad host".to_string())?,
        };
        let conn = rustls::ClientConnection::new(Arc::new(config), name)
            .map_err(|_| "TLS setup failed".to_string())?;
        let addrs = match &target.host {
            Host::Loopback => vec![SocketAddr::from((Ipv4Addr::LOCALHOST, target.port))],
            Host::Name(n) => resolve_within(n.clone(), target.port, left(deadline))?,
        };
        let sock = connect_within(&addrs, left(deadline))?;
        sock.set_read_timeout(Some(left(deadline).max(Duration::from_millis(1))))
            .map_err(|_| "socket failed".to_string())?;
        sock.set_write_timeout(Some(left(deadline).max(Duration::from_millis(1))))
            .map_err(|_| "socket failed".to_string())?;
        let mut tls = rustls::StreamOwned::new(conn, sock);
        tls.write_all(&build_request(request, &target))
            .map_err(|_| "the request couldn't be sent".to_string())?;
        let mut data = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            if Instant::now() >= deadline {
                return Err("the answer took too long".into());
            }
            match tls.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => data.extend_from_slice(&chunk[..n]),
                // A server that closes without close_notify still sent the whole answer.
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(_) => return Err("the answer couldn't be read".into()),
            }
            if data.len() > MAX_BODY + 16 * 1024 {
                return Err("the answer was too large".into());
            }
        }
        parse_response(&data)
    }
}

fn left(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

#[derive(Debug, PartialEq, Eq)]
enum Host {
    Loopback,
    Name(String),
}

#[derive(Debug, PartialEq, Eq)]
struct Target {
    host: Host,
    port: u16,
    path: String,
}

impl Target {
    fn parse(url: &str, trust: Trust) -> Result<Target, String> {
        let rest = url.strip_prefix("https://").ok_or("https only")?;
        let (authority, path) = rest
            .split_once('/')
            .map_or((rest, "/".to_string()), |(a, p)| (a, format!("/{p}")));
        if path.contains(['\r', '\n', ' ']) {
            return Err("bad path".into());
        }
        match trust {
            Trust::System => {
                if authority.is_empty() || authority.contains(['@', ':', ' ', '[']) {
                    return Err("bad host".into());
                }
                Ok(Target {
                    host: Host::Name(authority.to_string()),
                    port: 443,
                    path,
                })
            }
            Trust::LoopbackSelfSigned => {
                let port = authority
                    .strip_prefix("127.0.0.1:")
                    .and_then(|p| p.parse::<u16>().ok())
                    .filter(|p| *p != 0)
                    .ok_or("only 127.0.0.1 is trusted without a certificate")?;
                Ok(Target {
                    host: Host::Loopback,
                    port,
                    path,
                })
            }
        }
    }

    fn host_header(&self) -> String {
        match &self.host {
            Host::Loopback => format!("127.0.0.1:{}", self.port),
            Host::Name(n) => n.clone(),
        }
    }
}

fn build_request(request: &HttpRequest, target: &Target) -> Vec<u8> {
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
        request.method,
        target.path,
        target.host_header()
    );
    for (name, value) in &request.headers {
        // A value with a line break would add a header of its own.
        if name.contains([':', '\r', '\n']) || value.contains(['\r', '\n']) {
            continue;
        }
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = &request.body {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    let mut out = head.into_bytes();
    if let Some(body) = &request.body {
        out.extend_from_slice(body);
    }
    out
}

fn client_config(trust: Trust) -> Result<rustls::ClientConfig, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|_| "TLS setup failed".to_string())?;
    Ok(match trust {
        Trust::System => {
            let mut roots = rustls::RootCertStore::empty();
            for cert in rustls_native_certs::load_native_certs().certs {
                let _ = roots.add(cert);
            }
            builder
                .with_root_certificates(roots)
                .with_no_client_auth()
        }
        Trust::LoopbackSelfSigned => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyCertificate(provider)))
            .with_no_client_auth(),
    })
}

/// Accepts the server's certificate without checking who signed it. Only built for a connection
/// to 127.0.0.1 ([`Target::parse`] refuses everything else), where the peer is a process on this
/// computer; the handshake signatures are still verified.
#[derive(Debug)]
struct AnyCertificate(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// The addresses of `host`, or an error once `within` has passed. The system resolver has no
/// timeout of its own, so it runs on a thread of its own, left to finish alone if it hangs.
fn resolve_within(host: String, port: u16, within: Duration) -> Result<Vec<SocketAddr>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("provider-dns".into())
        .spawn(move || {
            let found = (host.as_str(), port)
                .to_socket_addrs()
                .map(Iterator::collect)
                .map_err(|_| "the name lookup failed".to_string());
            let _ = tx.send(found);
        })
        .map_err(|_| "the name lookup couldn't start".to_string())?;
    rx.recv_timeout(within)
        .map_err(|_| "the name lookup took too long".to_string())?
}

fn connect_within(addrs: &[SocketAddr], within: Duration) -> Result<TcpStream, String> {
    let deadline = Instant::now() + within;
    for addr in addrs {
        let remaining = left(deadline);
        if remaining.is_zero() {
            break;
        }
        if let Ok(sock) = TcpStream::connect_timeout(addr, remaining) {
            return Ok(sock);
        }
    }
    Err("couldn't connect".into())
}

/// Status and body of an HTTP/1.1 answer (plain or chunked).
pub fn parse_response(data: &[u8]) -> Result<HttpResponse, String> {
    let end = data
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no head")?;
    let head = std::str::from_utf8(&data[..end]).map_err(|_| "head not text")?;
    let status: u16 = head
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("no status")?;
    let body = &data[end + 4..];
    let chunked = head.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let body = if chunked {
        dechunk(body)?
    } else {
        body.to_vec()
    };
    if body.len() > MAX_BODY {
        return Err("the answer was too large".into());
    }
    Ok(HttpResponse { status, body })
}

fn dechunk(body: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut rest = body;
    loop {
        let line_end = rest
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("bad chunk")?;
        let size_text = std::str::from_utf8(&rest[..line_end]).map_err(|_| "bad chunk")?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| "bad chunk size")?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if rest.len() < size {
            return Err("short chunk".into());
        }
        out.extend_from_slice(&rest[..size]);
        rest = rest.get(size + 2..).unwrap_or(&[]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn request(url: &str, trust: Trust) -> HttpRequest {
        HttpRequest {
            method: "POST",
            url: url.into(),
            headers: vec![("x-codeium-csrf-token".into(), "tok".into())],
            body: Some(b"{}".to_vec()),
            timeout: Duration::from_secs(5),
            trust,
        }
    }

    #[test]
    fn reads_status_plain_and_chunked_bodies() {
        let plain = parse_response(b"HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\n\r\n{}").unwrap();
        assert_eq!((plain.status, plain.body.as_slice()), (404, &b"{}"[..]));
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{\"k\r\n4\r\n\":1}\r\n0\r\n\r\n";
        let r = parse_response(chunked).unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, &b"{\"k\":1}"[..]));
        assert!(parse_response(b"junk").is_err());
    }

    #[test]
    fn urls_are_https_and_the_untrusted_certificate_only_goes_to_loopback() {
        assert!(Target::parse("http://chatgpt.com/x", Trust::System).is_err());
        assert!(Target::parse("https://user@evil.test/x", Trust::System).is_err());
        assert!(Target::parse("https://evil.test:8443/x", Trust::System).is_err());
        let t = Target::parse("https://chatgpt.com/backend-api/wham/usage", Trust::System).unwrap();
        assert_eq!((t.port, t.path.as_str()), (443, "/backend-api/wham/usage"));
        for bad in [
            "https://evil.test:9000/x",
            "https://127.0.0.1/x",
            "https://127.0.0.1.evil.test:9000/x",
            "https://localhost:9000/x",
            "https://127.0.0.1:0/x",
        ] {
            assert!(Target::parse(bad, Trust::LoopbackSelfSigned).is_err(), "{bad}");
        }
        let ok = Target::parse("https://127.0.0.1:4433/a/B", Trust::LoopbackSelfSigned).unwrap();
        assert_eq!((ok.host, ok.port), (Host::Loopback, 4433));
    }

    #[test]
    fn a_header_cannot_add_another() {
        let mut r = request("https://127.0.0.1:1/p", Trust::LoopbackSelfSigned);
        r.headers
            .push(("X-Evil".into(), "a\r\nInjected: yes".into()));
        let target = Target::parse(&r.url, r.trust).unwrap();
        let text = String::from_utf8(build_request(&r, &target)).unwrap();
        assert!(text.starts_with("POST /p HTTP/1.1\r\nHost: 127.0.0.1:1\r\n"));
        assert!(text.contains("x-codeium-csrf-token: tok\r\n"));
        assert!(text.contains("Content-Length: 2\r\n"));
        assert!(!text.contains("Injected"), "{text}");
    }

    /// A TLS server on the loopback address with a throwaway self-signed certificate: the client
    /// completes the handshake, sends its header and body, and reads a chunked 200.
    #[test]
    fn talks_to_a_self_signed_loopback_server() {
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = rcgen::CertificateParams::new(vec!["127.0.0.1".to_string()])
            .unwrap()
            .self_signed(&key)
            .unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let server = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()),
            )
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let conn = rustls::ServerConnection::new(Arc::new(server)).unwrap();
            let mut tls = rustls::StreamOwned::new(conn, sock);
            let mut got = Vec::new();
            let mut buf = [0u8; 1024];
            while !got.ends_with(b"{}") {
                let n = tls.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
            }
            tls.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n")
                .unwrap();
            tls.flush().unwrap();
            tls.conn.send_close_notify();
            let _ = tls.flush();
            String::from_utf8_lossy(&got).into_owned()
        });
        let reply = TlsHttp
            .send(&request(
                &format!("https://127.0.0.1:{port}/svc/GetUserStatus"),
                Trust::LoopbackSelfSigned,
            ))
            .unwrap();
        assert_eq!((reply.status, reply.body.as_slice()), (200, &b"{}"[..]));
        let seen = handle.join().unwrap();
        assert!(seen.starts_with("POST /svc/GetUserStatus HTTP/1.1"), "{seen}");
        assert!(seen.contains("x-codeium-csrf-token: tok"));
    }
}
