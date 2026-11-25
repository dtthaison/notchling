use super::fetch::{split_url, Fetch, Head, Receiver};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

const CONNECT: Duration = Duration::from_secs(15);
const IDLE: Duration = Duration::from_secs(30);
const MAX_HEAD: usize = 32 * 1024;

pub struct TlsFetch;

pub fn request_text(host: &str, path: &str, from: u64) -> String {
    let mut text = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Notchling\r\nAccept-Encoding: identity\r\nConnection: close\r\n"
    );
    if from > 0 {
        text.push_str(&format!("Range: bytes={from}-\r\n"));
    }
    text.push_str("\r\n");
    text
}

pub fn parse_head(text: &str) -> Result<Head, String> {
    let mut lines = text.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or("the answer had no status")?;
    let mut head = Head {
        status,
        ..Head::default()
    };
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "location" => head.location = Some(value.to_string()),
            "content-length" => head.length = value.parse().ok(),
            "content-range" => {
                head.range_start = value
                    .strip_prefix("bytes ")
                    .and_then(|r| r.split(['-', '/']).next())
                    .and_then(|s| s.parse().ok());
            }
            _ => {}
        }
    }
    Ok(head)
}

fn chunked(head_text: &str) -> bool {
    head_text.split("\r\n").any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    })
}

fn read_head<R: BufRead>(reader: &mut R) -> Result<String, String> {
    let mut text = Vec::new();
    loop {
        let mut line = Vec::new();
        let n = reader
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("the connection closed early".into());
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
        text.extend_from_slice(&line);
        if text.len() > MAX_HEAD {
            return Err("the answer's head is too long".into());
        }
    }
    String::from_utf8(text)
        .map(|t| t.replace("\r\n", "\n").replace('\n', "\r\n"))
        .map_err(|_| "the answer's head is not text".into())
}

fn ended(left: Option<u64>, head: Head) -> Result<Head, String> {
    match left {
        Some(l) if l > 0 => Err("the connection closed early".into()),
        _ => Ok(head),
    }
}

pub fn read_response<R: BufRead>(reader: &mut R, rx: &mut dyn Receiver) -> Result<Head, String> {
    let text = read_head(reader)?;
    let head = parse_head(&text)?;
    if !matches!(head.status, 200 | 206) {
        return Ok(head);
    }
    rx.head(&head)?;
    let mut buf = vec![0u8; 64 * 1024];
    if chunked(&text) {
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).map_err(|e| e.to_string())?;
            let size = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16)
                .map_err(|_| "bad chunk size".to_string())?;
            if size == 0 {
                return Ok(head);
            }
            let mut left = size;
            while left > 0 {
                let want = left.min(buf.len());
                reader
                    .read_exact(&mut buf[..want])
                    .map_err(|e| e.to_string())?;
                rx.data(&buf[..want])?;
                left -= want;
            }
            let mut crlf = [0u8; 2];
            reader.read_exact(&mut crlf).map_err(|e| e.to_string())?;
        }
    }
    let mut left = head.length;
    loop {
        let want = left.map_or(buf.len(), |l| l.min(buf.len() as u64) as usize);
        if want == 0 {
            return Ok(head);
        }
        match reader.read(&mut buf[..want]) {
            Ok(0) => return ended(left, head),
            Ok(n) => {
                rx.data(&buf[..n])?;
                if let Some(l) = left.as_mut() {
                    *l -= n as u64;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return ended(left, head),
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn connect(host: &str) -> Result<TcpStream, String> {
    let name = host.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("model-dns".into())
        .spawn(move || {
            let found: Result<Vec<SocketAddr>, String> = (name.as_str(), 443)
                .to_socket_addrs()
                .map(Iterator::collect)
                .map_err(|e| e.to_string());
            let _ = tx.send(found);
        })
        .map_err(|e| e.to_string())?;
    let addrs = rx
        .recv_timeout(CONNECT)
        .map_err(|_| "the name lookup took too long".to_string())??;
    let mut last = "no address".to_string();
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT) {
            Ok(sock) => return Ok(sock),
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

impl Fetch for TlsFetch {
    fn get(&self, url: &str, from: u64, rx: &mut dyn Receiver) -> Result<Head, String> {
        let (host, path) = split_url(url)?;
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
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
        let conn =
            rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
        let sock = connect(host)?;
        sock.set_read_timeout(Some(IDLE))
            .map_err(|e| e.to_string())?;
        sock.set_write_timeout(Some(IDLE))
            .map_err(|e| e.to_string())?;
        let mut tls = rustls::StreamOwned::new(conn, sock);
        tls.write_all(request_text(host, &path, from).as_bytes())
            .map_err(|e| e.to_string())?;
        let mut reader = BufReader::with_capacity(64 * 1024, tls);
        read_response(&mut reader, rx)
    }
}
