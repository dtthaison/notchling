//! The one request the bridge makes: a blocking HTTPS GET of a Cloudflare Access team's signing
//! keys (`AccessKeySet.liveFetch`). TLS through rustls with the OS's root certificates, 5 s each
//! for the name lookup, the connect (every address together) and each read or write, 1 MB at most. It runs on the handshaking phone's thread, never another's,
//! and only for a request that carries Cloudflare's headers while internet access is on.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

const LIMIT: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(5);

/// The live [`super::jwt::Fetch`].
pub fn live() -> super::jwt::Fetch {
    Arc::new(|url: &str| get(url))
}

fn get(url: &str) -> Result<Vec<u8>, String> {
    let rest = url.strip_prefix("https://").ok_or("https only")?;
    let (host, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
    if host.is_empty() || host.contains(['@', ':', ' ']) {
        return Err("bad host".into());
    }
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(cert);
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let name =
        rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| e.to_string())?;
    let conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
    let lookup_host = host.to_string();
    let addrs = resolve_within(TIMEOUT, move || {
        (lookup_host.as_str(), 443)
            .to_socket_addrs()
            .map(Iterator::collect)
            .map_err(|e| e.to_string())
    })?;
    let sock = connect_within(&addrs, TIMEOUT)?;
    sock.set_read_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    sock.set_write_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    let mut tls = rustls::StreamOwned::new(conn, sock);
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\nConnection: close\r\nUser-Agent: notchling\r\n\r\n");
    tls.write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut body = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        match tls.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
            // A server that closes without close_notify still sent the whole answer.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        }
        if body.len() > LIMIT {
            return Err("answer too large".into());
        }
    }
    parse_response(&body)
}

/// The first of `addrs` that answers, all of them together within `within`.
fn connect_within(addrs: &[SocketAddr], within: Duration) -> Result<TcpStream, String> {
    let deadline = std::time::Instant::now() + within;
    let mut last = "no address".to_string();
    for addr in addrs {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err("connecting took too long".into());
        }
        match TcpStream::connect_timeout(addr, left) {
            Ok(sock) => return Ok(sock),
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

/// The addresses `lookup` finds, or an error once `within` has passed. The system resolver has no
/// timeout of its own, so it runs on a thread of its own, left to finish alone if it hangs.
fn resolve_within(
    within: Duration,
    lookup: impl FnOnce() -> Result<Vec<SocketAddr>, String> + Send + 'static,
) -> Result<Vec<SocketAddr>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("bridge-dns".into())
        .spawn(move || {
            let _ = tx.send(lookup());
        })
        .map_err(|e| e.to_string())?;
    rx.recv_timeout(within)
        .map_err(|_| "the name lookup took too long".to_string())?
}

/// The body of a `200` answer (plain or chunked); an error for anything else.
pub fn parse_response(data: &[u8]) -> Result<Vec<u8>, String> {
    let end = data
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no head")?;
    let head = std::str::from_utf8(&data[..end]).map_err(|_| "head not text")?;
    let status = head.split(' ').nth(1).ok_or("no status")?;
    if status != "200" {
        return Err(format!("status {status}"));
    }
    let body = &data[end + 4..];
    let chunked = head.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    if !chunked {
        return Ok(body.to_vec());
    }
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

    #[test]
    fn reads_plain_and_chunked_answers() {
        assert_eq!(
            parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").unwrap(),
            b"{}"
        );
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{\"k\r\n4\r\n\":1}\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked).unwrap(), b"{\"k\":1}");
        assert!(parse_response(b"HTTP/1.1 404 Not Found\r\n\r\n").is_err());
        assert!(parse_response(b"junk").is_err());
    }

    #[test]
    fn a_name_lookup_that_hangs_gives_up_at_its_deadline() {
        let started = std::time::Instant::now();
        let slow = || {
            std::thread::sleep(Duration::from_secs(3));
            Ok(vec![])
        };
        assert!(resolve_within(Duration::from_millis(100), slow).is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        let addr: std::net::SocketAddr = "192.0.2.1:443".parse().unwrap();
        assert_eq!(
            resolve_within(Duration::from_secs(1), move || Ok(vec![addr])),
            Ok(vec![addr])
        );
    }

    #[test]
    fn only_https_urls_with_a_plain_host() {
        assert!(get("http://example.com/x").is_err());
        assert!(get("https://user@evil/x").is_err());
    }
}
