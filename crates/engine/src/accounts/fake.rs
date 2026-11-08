//! Stand-ins for the Keychain and the network, for tests (the engine's and the daemon's). Nothing
//! here reaches a real login: [`MemoryKeychain`] answers `security`'s commands from a map, and
//! [`ScriptedHttp`] answers requests from a list. Each records what it was asked, so a test can
//! check that nothing was asked at all.

use super::http::{HttpClient, HttpRequest, HttpResponse};
use super::keychain::{hex, KeychainError, SecurityOutput, SecurityRunner, NOT_FOUND};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// An in-memory Keychain that speaks the three `security` commands Notchling uses:
/// `find-generic-password -s -a -w`, `delete-generic-password -s -a`, and `-i` with an
/// `add-generic-password -U -a "…" -s "…" -X "<hex>"` line on stdin.
#[derive(Default)]
pub struct MemoryKeychain {
    items: Mutex<BTreeMap<(String, String), String>>,
    calls: Mutex<Vec<Vec<String>>>,
    /// When set, a value is printed as hex, as `security -w` does for one that isn't plain text.
    pub print_hex: bool,
}

impl MemoryKeychain {
    pub fn new() -> MemoryKeychain {
        MemoryKeychain::default()
    }

    /// One that prints every value as hex.
    pub fn printing_hex() -> MemoryKeychain {
        MemoryKeychain {
            print_hex: true,
            ..MemoryKeychain::default()
        }
    }

    pub fn put(&self, service: &str, account: &str, value: &str) {
        self.items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((service.into(), account.into()), value.into());
    }

    pub fn get(&self, service: &str, account: &str) -> Option<String> {
        self.items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(service.into(), account.into()))
            .cloned()
    }

    /// Every (service, account) held now.
    pub fn names(&self) -> Vec<(String, String)> {
        let items = self.items.lock().unwrap_or_else(|e| e.into_inner());
        items.keys().cloned().collect()
    }

    /// The argument lists it was run with, in order.
    pub fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn named(args: &[String], flag: &str) -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    }

    /// `"<value>"` after `flag` in a `-i` line.
    fn quoted(line: &str, flag: &str) -> Option<String> {
        let start = line.find(&format!("{flag} \""))? + flag.len() + 2;
        let end = line[start..].find('"')? + start;
        Some(line[start..end].to_string())
    }

    fn add(&self, line: &str) -> i32 {
        let (Some(account), Some(service), Some(value)) = (
            Self::quoted(line, "-a"),
            Self::quoted(line, "-s"),
            Self::quoted(line, "-X"),
        ) else {
            return 1;
        };
        let bytes: Option<Vec<u8>> = (0..value.len())
            .step_by(2)
            .map(|i| {
                value
                    .get(i..i + 2)
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
            })
            .collect();
        match bytes.and_then(|b| String::from_utf8(b).ok()) {
            Some(text) => {
                self.put(&service, &account, &text);
                0
            }
            None => 1,
        }
    }
}

impl SecurityRunner for MemoryKeychain {
    fn run(&self, args: &[String], stdin: Option<&[u8]>) -> Result<SecurityOutput, KeychainError> {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(args.to_vec());
        let out = |status: i32, stdout: Vec<u8>| Ok(SecurityOutput { status, stdout });
        match args.first().map(String::as_str) {
            Some("-i") => {
                let line = String::from_utf8_lossy(stdin.unwrap_or_default()).into_owned();
                out(self.add(line.trim_end()), Vec::new())
            }
            Some("find-generic-password") => {
                let key = (
                    Self::named(args, "-s").unwrap_or_default(),
                    Self::named(args, "-a").unwrap_or_default(),
                );
                match self.get(&key.0, &key.1) {
                    Some(value) if self.print_hex => {
                        out(0, format!("{}\n", hex(value.as_bytes())).into_bytes())
                    }
                    Some(value) => out(0, format!("{value}\n").into_bytes()),
                    None => out(NOT_FOUND, Vec::new()),
                }
            }
            Some("delete-generic-password") => {
                let key = (
                    Self::named(args, "-s").unwrap_or_default(),
                    Self::named(args, "-a").unwrap_or_default(),
                );
                let removed = self
                    .items
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&key);
                out(if removed.is_some() { 0 } else { NOT_FOUND }, Vec::new())
            }
            _ => out(1, Vec::new()),
        }
    }
}

/// A runner that fails the test if it is ever called: for paths that must not touch the Keychain.
pub struct PanicKeychain;

impl SecurityRunner for PanicKeychain {
    fn run(&self, args: &[String], _stdin: Option<&[u8]>) -> Result<SecurityOutput, KeychainError> {
        panic!("the Keychain was called: {:?}", args.first());
    }
}

/// Answers requests in order from a script of (URL prefix, response); records each request's URL
/// and bearer token. A request no entry matches gets status 0 ("no answer").
#[derive(Default)]
pub struct ScriptedHttp {
    script: Mutex<Vec<(String, HttpResponse)>>,
    seen: Mutex<Vec<(String, Option<String>)>>,
}

impl ScriptedHttp {
    pub fn new() -> ScriptedHttp {
        ScriptedHttp::default()
    }

    /// The next request whose URL starts with `url` gets this answer (once).
    pub fn answer(&self, url: &str, status: u16, body: &str) {
        self.script.lock().unwrap_or_else(|e| e.into_inner()).push((
            url.into(),
            HttpResponse {
                status,
                body: body.as_bytes().to_vec(),
            },
        ));
    }

    /// (URL, bearer token) of each request so far.
    pub fn seen(&self) -> Vec<(String, Option<String>)> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl HttpClient for ScriptedHttp {
    fn send(&self, request: &HttpRequest) -> HttpResponse {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).push((
            request.url.clone(),
            request.bearer.as_ref().map(|b| b.expose().to_string()),
        ));
        let mut script = self.script.lock().unwrap_or_else(|e| e.into_inner());
        match script
            .iter()
            .position(|(url, _)| request.url.starts_with(url.as_str()))
        {
            Some(i) => script.remove(i).1,
            None => HttpResponse {
                status: 0,
                body: Vec::new(),
            },
        }
    }
}

/// A client that fails the test if it is ever called.
pub struct PanicHttp;

impl HttpClient for PanicHttp {
    fn send(&self, request: &HttpRequest) -> HttpResponse {
        panic!("the network was called: {}", request.url);
    }
}
