//! The one request the bridge makes: a blocking HTTPS GET of a Cloudflare Access team's signing
//! keys (`AccessKeySet.liveFetch`). TLS through rustls with the OS's root certificates, a 5 s
//! connect and read limit, 1 MB at most. It runs on the handshaking phone's thread, never another's,
//! and only for a request that carries Cloudflare's headers while internet access is on.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
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
    let addr = (host, 443)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    let sock = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| e.to_string())?;
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
    fn only_https_urls_with_a_plain_host() {
        assert!(get("http://example.com/x").is_err());
        assert!(get("https://user@evil/x").is_err());
    }
}
