use super::{chunked, Login, Secret};
use serde_json::{Map, Value};
use std::fmt;
use std::time::Duration;

pub const SECURITY: &str = "/usr/bin/security";
pub const NOT_FOUND: i32 = 44;
pub const MAX_LINE: usize = 4000;
pub const TIMEOUT: Duration = Duration::from_secs(120);

pub struct SecurityOutput {
    pub status: i32,
    pub stdout: Vec<u8>,
}

impl fmt::Debug for SecurityOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SecurityOutput {{ status: {}, stdout: {} bytes }}",
            self.status,
            self.stdout.len()
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeychainError {
    Unsupported(String),
    Refused(i32),
    Run(String),
    TooLong,
    BadName,
    NotStored,
    Damaged,
}

impl fmt::Display for KeychainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeychainError::Unsupported(why) => write!(f, "no Keychain here: {why}"),
            KeychainError::Refused(status) => {
                write!(f, "the Keychain refused (security exited {status})")
            }
            KeychainError::Run(why) => write!(f, "couldn't run security: {why}"),
            KeychainError::TooLong => f.write_str(
                "the login is too long to pass to security on stdin, and Notchling never puts one on a command line",
            ),
            KeychainError::BadName => f.write_str("a Keychain name with a quote, backslash or control character"),
            KeychainError::NotStored => f.write_str("the Keychain item didn't keep the new value"),
            KeychainError::Damaged => f.write_str(
                "the saved copy in the Keychain is damaged (its parts don't add up), so it wasn't used",
            ),
        }
    }
}

pub trait SecurityRunner: Send + Sync {
    fn run(&self, args: &[String], stdin: Option<&[u8]>) -> Result<SecurityOutput, KeychainError>;
}

#[derive(Clone)]
pub struct Keys {
    pub runner: std::sync::Arc<dyn SecurityRunner>,
    pub user: String,
}

impl Keys {
    pub fn read(&self, service: &str, account: &str) -> Result<Option<Secret>, KeychainError> {
        Ok(chunked::read(&Raw(self), service, account)?.map(Secret::new))
    }

    fn read_item(&self, service: &str, account: &str) -> Result<Option<Secret>, KeychainError> {
        check_name(service)?;
        check_name(account)?;
        let args = strings(&["find-generic-password", "-s", service, "-a", account, "-w"]);
        let out = self.runner.run(&args, None)?;
        match out.status {
            0 => {
                let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                Ok((!text.is_empty()).then(|| Secret::new(text)))
            }
            NOT_FOUND => Ok(None),
            status => Err(KeychainError::Refused(status)),
        }
    }

    pub fn read_json(&self, service: &str, account: &str) -> Result<Option<Login>, KeychainError> {
        Ok(self
            .read(service, account)?
            .and_then(|text| object_from_printed(text.expose())))
    }

    pub fn read_text(&self, service: &str, account: &str) -> Result<Option<Secret>, KeychainError> {
        Ok(self
            .read(service, account)?
            .map(|text| Secret::new(text_from_printed(text.expose()))))
    }

    pub fn write(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError> {
        chunked::write(&Raw(self), service, account, value)
    }

    fn write_item(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError> {
        let line = match add_line(service, account, value) {
            Err(KeychainError::TooLong) => text_line(service, account, value)?,
            line => line?,
        };
        let out = self
            .runner
            .run(&strings(&["-i"]), Some(line.expose().as_bytes()))?;
        if out.status != 0 {
            return Err(KeychainError::Refused(out.status));
        }
        let stored = self
            .read_item(service, account)?
            .map(|t| text_from_printed(t.expose()));
        match stored {
            Some(stored) if stored == value => Ok(()),
            _ => Err(KeychainError::NotStored),
        }
    }

    pub fn delete(&self, service: &str, account: &str) -> Result<bool, KeychainError> {
        chunked::delete(&Raw(self), service, account)
    }

    fn delete_item(&self, service: &str, account: &str) -> Result<bool, KeychainError> {
        check_name(service)?;
        check_name(account)?;
        let args = strings(&["delete-generic-password", "-s", service, "-a", account]);
        match self.runner.run(&args, None)?.status {
            0 => Ok(true),
            NOT_FOUND => Ok(false),
            status => Err(KeychainError::Refused(status)),
        }
    }
}

struct Raw<'a>(&'a Keys);

impl chunked::Items for Raw<'_> {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeychainError> {
        Ok(self
            .0
            .read_item(service, account)?
            .map(|t| t.expose().to_string()))
    }

    fn put(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError> {
        self.0.write_item(service, account, value)
    }

    fn remove(&self, service: &str, account: &str) -> Result<bool, KeychainError> {
        self.0.delete_item(service, account)
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

fn check_name(name: &str) -> Result<(), KeychainError> {
    let bad = name.is_empty()
        || name.len() > 255
        || name
            .chars()
            .any(|c| c == '"' || c == '\\' || c.is_control());
    if bad {
        Err(KeychainError::BadName)
    } else {
        Ok(())
    }
}

pub fn add_line(service: &str, account: &str, value: &str) -> Result<Secret, KeychainError> {
    check_name(service)?;
    check_name(account)?;
    let line = format!(
        "add-generic-password -U -a \"{account}\" -s \"{service}\" -X \"{}\"\n",
        hex(value.as_bytes())
    );
    if line.len() >= MAX_LINE {
        return Err(KeychainError::TooLong);
    }
    Ok(Secret::new(line))
}

pub fn text_line(service: &str, account: &str, value: &str) -> Result<Secret, KeychainError> {
    check_name(service)?;
    check_name(account)?;
    if value.chars().any(char::is_control) {
        return Err(KeychainError::TooLong);
    }
    let quoted = value.replace('\\', "\\\\").replace('"', "\\\"");
    let line =
        format!("add-generic-password -U -a \"{account}\" -s \"{service}\" -w \"{quoted}\"\n");
    if line.len() >= MAX_LINE {
        return Err(KeychainError::TooLong);
    }
    Ok(Secret::new(line))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

pub fn object_from_printed(text: &str) -> Option<Login> {
    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) {
        return Some(Login(map));
    }
    let bytes = unhex(text)?;
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(map)) => Some(Login(map)),
        _ => None,
    }
}

fn text_from_printed(text: &str) -> String {
    if serde_json::from_str::<Value>(text).is_ok() {
        return text.to_string();
    }
    unhex(text)
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|decoded| serde_json::from_str::<Value>(decoded).is_ok())
        .unwrap_or_else(|| text.to_string())
}

pub fn empty_object() -> Login {
    Login(Map::new())
}

pub struct SystemSecurity;

impl SecurityRunner for SystemSecurity {
    #[cfg(target_os = "macos")]
    fn run(&self, args: &[String], stdin: Option<&[u8]>) -> Result<SecurityOutput, KeychainError> {
        run_security(args, stdin)
    }

    #[cfg(not(target_os = "macos"))]
    fn run(
        &self,
        _args: &[String],
        _stdin: Option<&[u8]>,
    ) -> Result<SecurityOutput, KeychainError> {
        Err(KeychainError::Unsupported(
            "Claude Code accounts are switched only on macOS".into(),
        ))
    }
}

pub struct NoKeychain(pub &'static str);

impl SecurityRunner for NoKeychain {
    fn run(
        &self,
        _args: &[String],
        _stdin: Option<&[u8]>,
    ) -> Result<SecurityOutput, KeychainError> {
        Err(KeychainError::Unsupported(self.0.into()))
    }
}

#[cfg(target_os = "macos")]
fn run_security(args: &[String], stdin: Option<&[u8]>) -> Result<SecurityOutput, KeychainError> {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::time::Instant;
    let mut child = Command::new(SECURITY)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| KeychainError::Run(e.to_string()))?;
    if let Some(mut pipe) = child.stdin.take() {
        if let Some(input) = stdin {
            let _ = pipe.write_all(input);
        }
    }
    let mut out = child
        .stdout
        .take()
        .ok_or_else(|| KeychainError::Run("no stdout".into()))?;
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        let _ = out.read_to_end(&mut data);
        data
    });
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(KeychainError::Run("security took too long".into()));
            }
            Err(e) => return Err(KeychainError::Run(e.to_string())),
        }
    };
    let stdout = reader
        .join()
        .map_err(|_| KeychainError::Run("reading security's output failed".into()))?;
    Ok(SecurityOutput {
        status: status.code().unwrap_or(-1),
        stdout,
    })
}

#[cfg(test)]
#[path = "keychain_tests.rs"]
mod tests;
