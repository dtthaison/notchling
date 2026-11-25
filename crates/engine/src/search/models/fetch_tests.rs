use super::fake::FakeFetch;
use super::fetch::{fetch_file, CANCELLED};
use super::fetch::{Fetch, Head, Receiver};
use super::manifest::ModelFile;
use super::net::{parse_head, read_response, request_text};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const REPO: &str = "o/m";
const REV: &str = "0123456789012345678901234567890123456789";
const URL: &str =
    "https://huggingface.co/o/m/resolve/0123456789012345678901234567890123456789/w.bin";

fn hosts() -> Vec<String> {
    vec!["huggingface.co".into(), "*.hf.co".into()]
}

fn body() -> Vec<u8> {
    (0..20_000u32).map(|i| (i % 251) as u8).collect()
}

fn file_for(data: &[u8]) -> ModelFile {
    let digest = Sha256::digest(data);
    ModelFile {
        repo: REPO.into(),
        revision: REV.into(),
        path: "w.bin".into(),
        save_as: "w.bin".into(),
        size: data.len() as u64,
        sha256: digest.iter().map(|b| format!("{b:02x}")).collect(),
        license: "Apache-2.0".into(),
    }
}

fn run(net: &FakeFetch, file: &ModelFile, dir: &Path) -> Result<std::path::PathBuf, String> {
    fetch_file(net, &hosts(), file, dir, &AtomicBool::new(false), &|_| {})
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir() -> Scratch {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let path = std::env::temp_dir().join(format!(
        "nl-models-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&path).unwrap();
    Scratch(path)
}

#[test]
fn a_good_file_lands_and_matches() {
    let data = body();
    let net = FakeFetch::new();
    net.serve(URL, &data);
    let tmp = dir();
    let seen = Mutex::new(Vec::new());
    let path = fetch_file(
        &net,
        &hosts(),
        &file_for(&data),
        &tmp.path().join("m"),
        &AtomicBool::new(false),
        &|p| seen.lock().unwrap().push(p.bytes),
    )
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), data);
    assert!(!tmp.path().join("m/w.bin.part").exists());
    assert_eq!(*seen.lock().unwrap().last().unwrap(), data.len() as u64);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = std::fs::metadata(tmp.path().join("m"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
    }
}

#[test]
fn a_wrong_hash_is_deleted() {
    let data = body();
    let mut other = data.clone();
    other[10] ^= 1;
    let net = FakeFetch::new();
    net.serve(URL, &other);
    let tmp = dir();
    let err = run(&net, &file_for(&data), tmp.path()).unwrap_err();
    assert!(err.contains("checksum"));
    assert!(!tmp.path().join("w.bin.part").exists());
    assert!(!tmp.path().join("w.bin").exists());
}

#[test]
fn too_many_bytes_aborts() {
    let data = body();
    let mut longer = data.clone();
    longer.extend_from_slice(&[1, 2, 3]);
    let net = FakeFetch::new();
    net.serve(URL, &longer);
    let tmp = dir();
    assert!(run(&net, &file_for(&data), tmp.path())
        .unwrap_err()
        .contains("larger"));
    assert!(!tmp.path().join("w.bin.part").exists());
    assert!(!tmp.path().join("w.bin").exists());
}

#[test]
fn an_unlisted_redirect_is_refused() {
    let data = body();
    for to in [
        "https://evil.example/w.bin",
        "http://huggingface.co/w.bin",
        "https://user@huggingface.co.evil.example/w",
    ] {
        let net = FakeFetch::new();
        net.redirect(URL, to);
        net.serve("https://evil.example/w.bin", &data);
        let tmp = dir();
        assert!(run(&net, &file_for(&data), tmp.path()).is_err(), "{to}");
        assert_eq!(net.requests().len(), 1);
        assert!(!tmp.path().join("w.bin.part").exists());
        assert!(!tmp.path().join("w.bin").exists());
    }
}

#[test]
fn redirects_to_listed_hosts_and_relative_ones_work_up_to_five() {
    let data = body();
    let net = FakeFetch::new();
    net.redirect(URL, "/api/resolve-cache/w.bin");
    net.redirect(
        "https://huggingface.co/api/resolve-cache/w.bin",
        "https://us.aws.cdn.hf.co/x/w.bin",
    );
    net.serve("https://us.aws.cdn.hf.co/x/w.bin", &data);
    let tmp = dir();
    run(&net, &file_for(&data), tmp.path()).unwrap();
    let loop_net = FakeFetch::new();
    loop_net.redirect(URL, URL);
    let tmp2 = dir();
    assert!(run(&loop_net, &file_for(&data), tmp2.path())
        .unwrap_err()
        .contains("redirects"));
    assert_eq!(loop_net.requests().len(), 6);
}

#[test]
fn range_resume_continues_the_part() {
    let data = body();
    let net = FakeFetch::new();
    net.cut(URL, &data, 8192);
    let tmp = dir();
    let file = file_for(&data);
    assert!(run(&net, &file, tmp.path()).is_err());
    let part = tmp.path().join("w.bin.part");
    assert_eq!(std::fs::metadata(&part).unwrap().len(), 8192);
    let net = FakeFetch::new();
    net.serve(URL, &data);
    let path = run(&net, &file, tmp.path()).unwrap();
    assert_eq!(net.requests(), vec![(URL.to_string(), 8192)]);
    assert_eq!(std::fs::read(path).unwrap(), data);
}

#[test]
fn a_server_without_ranges_starts_over() {
    let data = body();
    let tmp = dir();
    let file = file_for(&data);
    let cut = FakeFetch::new();
    cut.cut(URL, &data, 4096);
    assert!(run(&cut, &file, tmp.path()).is_err());
    let net = FakeFetch::new();
    net.serve_without_ranges(URL, &data);
    let path = run(&net, &file, tmp.path()).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), data);
}

#[test]
fn a_changed_file_fails_its_hash_after_resume() {
    let data = body();
    let tmp = dir();
    let file = file_for(&data);
    let cut = FakeFetch::new();
    cut.cut(URL, &data, 8192);
    assert!(run(&cut, &file, tmp.path()).is_err());
    let mut changed = data.clone();
    for b in changed.iter_mut().skip(9000).take(100) {
        *b ^= 0xff;
    }
    let net = FakeFetch::new();
    net.serve(URL, &changed);
    assert!(run(&net, &file, tmp.path())
        .unwrap_err()
        .contains("checksum"));
    assert!(!tmp.path().join("w.bin.part").exists());
    assert!(!tmp.path().join("w.bin").exists());
}

#[cfg(unix)]
#[test]
fn no_part_through_a_symlink() {
    let data = body();
    let tmp = dir();
    let target = tmp.path().join("elsewhere");
    std::fs::write(&target, b"keep").unwrap();
    std::os::unix::fs::symlink(&target, tmp.path().join("w.bin.part")).unwrap();
    let net = FakeFetch::new();
    net.serve(URL, &data);
    assert!(run(&net, &file_for(&data), tmp.path())
        .unwrap_err()
        .contains("link"));
    assert!(net.requests().is_empty());
    assert_eq!(std::fs::read(&target).unwrap(), b"keep");
}

#[cfg(unix)]
#[test]
fn a_write_error_cleans_up() {
    use std::os::unix::fs::PermissionsExt;
    let data = body();
    let tmp = dir();
    let part = tmp.path().join("w.bin.part");
    std::fs::write(&part, &data[..100]).unwrap();
    std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o400)).unwrap();
    let net = FakeFetch::new();
    net.serve(URL, &data);
    let err = run(&net, &file_for(&data), tmp.path()).unwrap_err();
    assert!(err.contains("free space"));
    assert!(!part.exists());
    assert!(!tmp.path().join("w.bin").exists());
}

#[test]
fn cancel_keeps_the_part() {
    let data = body();
    let tmp = dir();
    let net = FakeFetch::new();
    net.chunk(1000).serve(URL, &data);
    let cancel = AtomicBool::new(false);
    let err = fetch_file(
        &net,
        &hosts(),
        &file_for(&data),
        tmp.path(),
        &cancel,
        &|p| {
            if p.bytes >= 3000 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
    )
    .unwrap_err();
    assert_eq!(err, CANCELLED);
    let kept = std::fs::metadata(tmp.path().join("w.bin.part"))
        .unwrap()
        .len();
    assert!((3000..=4000).contains(&kept));
    assert!(!tmp.path().join("w.bin").exists());
}

#[test]
fn requests_carry_only_the_user_agent() {
    let text = request_text("huggingface.co", "/o/m/resolve/r/w.bin", 0);
    let names: Vec<String> = text
        .split("\r\n")
        .skip(1)
        .filter_map(|l| l.split_once(':').map(|(n, _)| n.to_ascii_lowercase()))
        .collect();
    assert_eq!(
        names,
        ["host", "user-agent", "accept-encoding", "connection"]
    );
    assert!(text.contains("User-Agent: Notchling\r\n"));
    let resumed = request_text("huggingface.co", "/p", 100);
    assert!(resumed.contains("Range: bytes=100-\r\n"));
    assert!(!resumed.to_ascii_lowercase().contains("cookie"));
    assert!(!resumed.to_ascii_lowercase().contains("authorization"));
}

struct Collect {
    head: Option<Head>,
    bytes: Vec<u8>,
}

impl Receiver for Collect {
    fn head(&mut self, head: &Head) -> Result<(), String> {
        self.head = Some(head.clone());
        Ok(())
    }
    fn data(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}

#[test]
fn responses_are_read_plain_chunked_and_ranged() {
    let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
    let mut rx = Collect {
        head: None,
        bytes: Vec::new(),
    };
    read_response(&mut &plain[..], &mut rx).unwrap();
    assert_eq!(rx.bytes, b"hello");
    let chunked =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
    let mut rx = Collect {
        head: None,
        bytes: Vec::new(),
    };
    read_response(&mut &chunked[..], &mut rx).unwrap();
    assert_eq!(rx.bytes, b"abcde");
    let ranged = b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 10-14/15\r\nContent-Length: 5\r\n\r\nhello";
    let mut rx = Collect {
        head: None,
        bytes: Vec::new(),
    };
    read_response(&mut &ranged[..], &mut rx).unwrap();
    assert_eq!(rx.head.unwrap().range_start, Some(10));
    let short = b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nhello";
    let mut rx = Collect {
        head: None,
        bytes: Vec::new(),
    };
    assert!(read_response(&mut &short[..], &mut rx).is_err());
    let moved = b"HTTP/1.1 302 Found\r\nLocation: /x\r\nContent-Length: 3\r\n\r\nabc";
    let mut rx = Collect {
        head: None,
        bytes: Vec::new(),
    };
    let head = read_response(&mut &moved[..], &mut rx).unwrap();
    assert_eq!(head.location.as_deref(), Some("/x"));
    assert!(rx.bytes.is_empty() && rx.head.is_none());
    assert!(parse_head("garbage").is_err());
}

struct Dropped<'a>(&'a [u8]);

impl std::io::Read for Dropped<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.0.is_empty() {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        let n = self.0.len().min(out.len());
        out[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}

#[test]
fn a_connection_dropped_without_a_close_is_an_error_while_bytes_are_owed() {
    let head = b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nhello";
    let mut rx = Collect {
        head: None,
        bytes: Vec::new(),
    };
    let mut reader = std::io::BufReader::new(Dropped(head));
    let err = read_response(&mut reader, &mut rx).unwrap_err();
    assert!(err.contains("closed early"), "{err}");
    assert_eq!(rx.bytes, b"hello");
    let open = b"HTTP/1.1 200 OK\r\n\r\nhello";
    let mut rx = Collect {
        head: None,
        bytes: Vec::new(),
    };
    let mut reader = std::io::BufReader::new(Dropped(open));
    read_response(&mut reader, &mut rx).unwrap();
    assert_eq!(rx.bytes, b"hello");
}

struct Short {
    data: Vec<u8>,
}

impl Fetch for Short {
    fn get(&self, _: &str, from: u64, rx: &mut dyn Receiver) -> Result<Head, String> {
        let start = from as usize;
        let head = Head {
            status: if from > 0 { 206 } else { 200 },
            range_start: (from > 0).then_some(from),
            length: Some((self.data.len() - start) as u64),
            ..Head::default()
        };
        rx.head(&head)?;
        rx.data(&self.data[start..start + 3000])?;
        Ok(head)
    }
}

#[test]
fn a_stream_that_ends_short_keeps_the_part_to_resume() {
    let data = body();
    let tmp = dir();
    let net = Short { data: data.clone() };
    let err = fetch_file(
        &net,
        &hosts(),
        &file_for(&data),
        tmp.path(),
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap_err();
    assert!(err.contains("closed early"), "{err}");
    let kept = std::fs::metadata(tmp.path().join("w.bin.part"))
        .unwrap()
        .len();
    assert_eq!(kept, 3000);
    assert!(!tmp.path().join("w.bin").exists());
}

#[test]
fn a_model_folder_that_is_a_link_is_refused() {
    let data = body();
    let tmp = dir();
    let elsewhere = tmp.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    let folder = tmp.path().join("folder");
    std::os::unix::fs::symlink(&elsewhere, &folder).unwrap();
    let net = FakeFetch::new();
    net.serve(URL, &data);
    let err = run(&net, &file_for(&data), &folder).unwrap_err();
    assert!(err.contains("link"), "{err}");
    assert!(net.requests().is_empty());
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
}

#[test]
fn a_save_name_that_leaves_the_folder_is_refused() {
    let data = body();
    let tmp = dir();
    let net = FakeFetch::new();
    net.serve(URL, &data);
    for bad in ["../w.bin", "a/b.bin", "..", ""] {
        let mut file = file_for(&data);
        file.save_as = bad.to_string();
        let err = run(&net, &file, tmp.path()).unwrap_err();
        assert!(err.contains("name"), "{bad:?}: {err}");
    }
    assert!(net.requests().is_empty());
}
