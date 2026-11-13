use super::http::{HttpClient, HttpRequest, HttpResponse};
use super::keychain::{hex, KeychainError, SecurityOutput, SecurityRunner, NOT_FOUND};
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Default)]
pub struct MemoryKeychain {
    items: Mutex<BTreeMap<(String, String), String>>,
    calls: Mutex<Vec<Vec<String>>>,
    pub print_hex: bool,
}

impl MemoryKeychain {
    pub fn new() -> MemoryKeychain {
        MemoryKeychain::default()
    }

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

    pub fn names(&self) -> Vec<(String, String)> {
        let items = self.items.lock().unwrap_or_else(|e| e.into_inner());
        items.keys().cloned().collect()
    }

    pub fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn named(args: &[String], flag: &str) -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    }

    fn quoted(line: &str, flag: &str) -> Option<String> {
        let start = line.find(&format!("{flag} \""))? + flag.len() + 2;
        let end = line[start..].find('"')? + start;
        Some(line[start..end].to_string())
    }

    fn text(line: &str) -> Option<String> {
        let start = line.find("-w \"")? + 4;
        let mut out = String::new();
        let mut chars = line[start..].chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => out.push(chars.next()?),
                '"' => return Some(out),
                c => out.push(c),
            }
        }
        None
    }

    fn add(&self, line: &str) -> i32 {
        let (Some(account), Some(service)) = (Self::quoted(line, "-a"), Self::quoted(line, "-s"))
        else {
            return 1;
        };
        if let Some(text) = Self::text(line) {
            self.put(&service, &account, &text);
            return 0;
        }
        let Some(value) = Self::quoted(line, "-X") else {
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

pub struct PanicKeychain;

impl SecurityRunner for PanicKeychain {
    fn run(&self, args: &[String], _stdin: Option<&[u8]>) -> Result<SecurityOutput, KeychainError> {
        panic!("the Keychain was called: {:?}", args.first());
    }
}

#[derive(Default)]
pub struct ScriptedHttp {
    script: Mutex<Vec<(String, HttpResponse)>>,
    seen: Mutex<Vec<(String, Option<String>)>>,
    headers: Mutex<Vec<Vec<(String, String)>>>,
}

impl ScriptedHttp {
    pub fn new() -> ScriptedHttp {
        ScriptedHttp::default()
    }

    pub fn answer(&self, url: &str, status: u16, body: &str) {
        self.script.lock().unwrap_or_else(|e| e.into_inner()).push((
            url.into(),
            HttpResponse {
                status,
                body: body.as_bytes().to_vec(),
            },
        ));
    }

    pub fn seen(&self) -> Vec<(String, Option<String>)> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn seen_headers(&self) -> Vec<Vec<(String, String)>> {
        self.headers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl HttpClient for ScriptedHttp {
    fn send(&self, request: &HttpRequest) -> HttpResponse {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).push((
            request.url.clone(),
            request.bearer.as_ref().map(|b| b.expose().to_string()),
        ));
        self.headers.lock().unwrap_or_else(|e| e.into_inner()).push(
            request
                .headers
                .iter()
                .map(|(n, v)| (n.to_string(), v.clone()))
                .collect(),
        );
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

pub struct PanicHttp;

impl HttpClient for PanicHttp {
    fn send(&self, request: &HttpRequest) -> HttpResponse {
        panic!("the network was called: {}", request.url);
    }
}
