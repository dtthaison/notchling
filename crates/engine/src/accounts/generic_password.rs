//! Generic passwords in the login Keychain, through `/usr/bin/security` the way Claude Code does
//! it (`Keychain` in `Accounts.swift`). Items it creates trust that tool, so reading them causes no
//! prompt, and an ad hoc signed app that changes on every build wouldn't keep its own access
//! anyway. Account logins never go through the Security framework; the vault's item does, and is
//! not here.
//!
//! A value goes in as hex on `security -i`'s standard input, so it never shows in the process
//! list. `security -i` reads lines into a 4 KB buffer, so a command that long can't go that way;
//! the Swift app then put the hex on the command line, and so does this ([`plan_write`]). That is
//! the one place a login can be seen in `ps` (by this user's own processes, for the few
//! milliseconds `security` runs), and it is a known gap, not a choice.
//!
//! Everything sits behind [`Keychain`] so no test touches a real item.

use std::collections::HashMap;
use std::sync::Mutex;

/// The login Keychain's generic passwords.
pub trait Keychain: Send + Sync {
    /// The value, trimmed; `None` when there is no such item or it is empty.
    fn read(&self, service: &str, account: &str) -> Option<String>;
    /// Adds the item or updates it in place (`-U`); true when it was written.
    fn write(&self, service: &str, account: &str, value: &str) -> bool;
    fn delete(&self, service: &str, account: &str);
}

/// `security -i` reads at most this many bytes of a line.
const INTERACTIVE_LINE_LIMIT: usize = 4000;

/// What to run to write one item.
#[derive(Clone, PartialEq, Eq)]
pub struct WritePlan {
    pub args: Vec<String>,
    pub stdin: Option<String>,
}

impl std::fmt::Debug for WritePlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WritePlan")
            .field("args", &self.args.len())
            .field("stdin_bytes", &self.stdin.as_ref().map(String::len))
            .finish()
    }
}

/// A name that is safe inside the double quotes of a `security -i` line.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['"', '\\', '\n', '\r', '\0'])
}

pub fn hex(value: &str) -> String {
    value.bytes().map(|b| format!("{b:02x}")).collect()
}

/// The command that writes `value`: through `security -i`'s stdin when the line fits, else on the
/// command line. `None` for a service or account that couldn't be quoted safely.
pub fn plan_write(service: &str, account: &str, value: &str) -> Option<WritePlan> {
    if !plain_name(service) || !plain_name(account) {
        return None;
    }
    let hex = hex(value);
    let line = format!("add-generic-password -U -a \"{account}\" -s \"{service}\" -X \"{hex}\"\n");
    if line.len() < INTERACTIVE_LINE_LIMIT {
        return Some(WritePlan {
            args: vec!["-i".into()],
            stdin: Some(line),
        });
    }
    Some(WritePlan {
        args: [
            "add-generic-password",
            "-U",
            "-a",
            account,
            "-s",
            service,
            "-X",
            &hex,
        ]
        .map(String::from)
        .to_vec(),
        stdin: None,
    })
}

/// `/usr/bin/security`, macOS only. Elsewhere nothing is read or written.
pub struct SecurityTool;

#[cfg(target_os = "macos")]
mod real {
    use super::*;
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const TOOL: &str = "/usr/bin/security";
    const TIMEOUT: Duration = Duration::from_secs(10);

    /// Runs `security`; the exit code and what it printed. Standard error is dropped.
    fn run(args: &[String], input: Option<&str>) -> Option<(i32, String)> {
        let mut child = Command::new(TOOL)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        if let Some(mut stdin) = child.stdin.take() {
            if let Some(input) = input {
                let _ = stdin.write_all(input.as_bytes());
            }
        }
        let mut out = child.stdout.take()?;
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = out.read_to_string(&mut text);
            text
        });
        let deadline = Instant::now() + TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
            }
        };
        let text = reader.join().unwrap_or_default();
        Some((status.code().unwrap_or(-1), text))
    }

    impl Keychain for SecurityTool {
        fn read(&self, service: &str, account: &str) -> Option<String> {
            let args = ["find-generic-password", "-s", service, "-a", account, "-w"]
                .map(String::from)
                .to_vec();
            let (code, out) = run(&args, None)?;
            let text = out.trim();
            (code == 0 && !text.is_empty()).then(|| text.to_string())
        }

        fn write(&self, service: &str, account: &str, value: &str) -> bool {
            let Some(plan) = plan_write(service, account, value) else {
                return false;
            };
            matches!(run(&plan.args, plan.stdin.as_deref()), Some((0, _)))
        }

        fn delete(&self, service: &str, account: &str) {
            let args = ["delete-generic-password", "-s", service, "-a", account]
                .map(String::from)
                .to_vec();
            let _ = run(&args, None);
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl Keychain for SecurityTool {
    fn read(&self, _service: &str, _account: &str) -> Option<String> {
        None
    }
    fn write(&self, _service: &str, _account: &str, _value: &str) -> bool {
        false
    }
    fn delete(&self, _service: &str, _account: &str) {}
}

/// An in-memory Keychain that counts every call, for tests. [`FakeKeychain::calls`] staying 0 is
/// how a switched-off daemon is shown not to touch the Keychain.
#[derive(Default)]
pub struct FakeKeychain {
    items: Mutex<HashMap<(String, String), String>>,
    calls: Mutex<Vec<String>>,
    fail_writes: Mutex<bool>,
}

impl FakeKeychain {
    pub fn new() -> FakeKeychain {
        FakeKeychain::default()
    }

    pub fn with(self, service: &str, account: &str, value: &str) -> FakeKeychain {
        lock(&self.items).insert((service.into(), account.into()), value.into());
        self
    }

    /// Writes answer false from now on.
    pub fn fail_writes(&self) {
        *lock(&self.fail_writes) = true;
    }

    pub fn get(&self, service: &str, account: &str) -> Option<String> {
        lock(&self.items)
            .get(&(service.to_string(), account.to_string()))
            .cloned()
    }

    /// Every call so far, as `read service/account`, `write …`, `delete …`.
    pub fn calls(&self) -> Vec<String> {
        lock(&self.calls).clone()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Keychain for FakeKeychain {
    fn read(&self, service: &str, account: &str) -> Option<String> {
        lock(&self.calls).push(format!("read {service}/{account}"));
        self.get(service, account).filter(|v| !v.is_empty())
    }

    fn write(&self, service: &str, account: &str, value: &str) -> bool {
        lock(&self.calls).push(format!("write {service}/{account}"));
        if *lock(&self.fail_writes) || !plain_name(service) || !plain_name(account) {
            return false;
        }
        lock(&self.items).insert((service.into(), account.into()), value.into());
        true
    }

    fn delete(&self, service: &str, account: &str) {
        lock(&self.calls).push(format!("delete {service}/{account}"));
        lock(&self.items).remove(&(service.to_string(), account.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_value_goes_in_as_hex_on_stdin_and_never_in_the_arguments() {
        let plan = plan_write("Notchling Codex", "u:w", "secret-value").unwrap();
        assert_eq!(plan.args, vec!["-i"]);
        let stdin = plan.stdin.unwrap();
        assert_eq!(
            stdin,
            format!(
                "add-generic-password -U -a \"u:w\" -s \"Notchling Codex\" -X \"{}\"\n",
                hex("secret-value")
            )
        );
        assert!(!stdin.contains("secret-value"));
    }

    #[test]
    fn a_long_value_is_the_known_gap_and_still_hex() {
        let value = "x".repeat(3000);
        let plan = plan_write("gemini", "antigravity", &value).unwrap();
        assert!(plan.stdin.is_none());
        assert_eq!(plan.args[0], "add-generic-password");
        assert!(plan.args.contains(&hex(&value)));
        assert!(!plan.args.iter().any(|a| a.contains("xxxx")));
    }

    #[test]
    fn a_name_that_would_break_the_quoting_is_refused() {
        for bad in ["a\"b", "a\\b", "a\nb", ""] {
            assert!(plan_write(bad, "ok", "v").is_none(), "{bad:?}");
            assert!(plan_write("ok", bad, "v").is_none(), "{bad:?}");
        }
        let fake = FakeKeychain::new();
        assert!(!fake.write("ok", "a\"b", "v"));
    }

    #[test]
    fn the_fake_keeps_items_and_counts_calls() {
        let fake = FakeKeychain::new().with("s", "a", "v");
        assert_eq!(fake.read("s", "a").as_deref(), Some("v"));
        assert!(fake.write("s", "b", "w"));
        fake.delete("s", "a");
        assert_eq!(fake.read("s", "a"), None);
        assert_eq!(fake.calls().len(), 4);
        let shown = format!("{:?}", plan_write("s", "a", "secret").unwrap());
        assert!(!shown.contains("secret") && !shown.contains(&hex("secret")));
    }
}
