//! Generic passwords in the login Keychain, through `/usr/bin/security` the way Claude Code does
//! it (`Keychain` in `Accounts.swift`). Items it creates trust that tool, so reading them causes no
//! prompt, and an ad hoc signed app that changes on every build wouldn't keep its own access
//! anyway. Account logins never go through the Security framework; the vault's item does, and is
//! not here.
//!
//! A value goes in as hex on `security -i`'s standard input, so it never shows in the process
//! list. `security -i` reads lines into a 4 KB buffer, so a longer command can't go that way. The
//! Swift app then put the hex on the command line, where `ps` shows it; this port refuses such a
//! value instead ([`KeychainError::TooLong`], the same rule as Claude Code's logins in
//! [`super::keychain`]): a login never goes on a command line.
//!
//! Everything sits behind [`Keychain`] so no test touches a real item. The process itself is
//! launched by [`super::keychain::SystemSecurity`], which Claude Code's logins use too (see the
//! module docs of [`super`] for why there are two faces over it).

pub use super::keychain::KeychainError;
use super::keychain::{add_line, SecurityRunner, SystemSecurity};
use std::collections::HashMap;
use std::sync::Mutex;

/// The login Keychain's generic passwords.
pub trait Keychain: Send + Sync {
    /// The value, trimmed; `None` when there is no such item or it is empty.
    fn read(&self, service: &str, account: &str) -> Option<String>;
    /// Adds the item or updates it in place (`-U`). A value too long for `security -i`'s line is
    /// refused ([`KeychainError::TooLong`]), never put on the command line.
    fn write(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError>;
    fn delete(&self, service: &str, account: &str);
}

/// What to run to write one item: `security -i` with the command on stdin.
#[derive(Clone, PartialEq, Eq)]
pub struct WritePlan {
    pub args: Vec<String>,
    pub stdin: String,
}

impl std::fmt::Debug for WritePlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WritePlan")
            .field("args", &self.args.len())
            .field("stdin_bytes", &self.stdin.len())
            .finish()
    }
}

pub fn hex(value: &str) -> String {
    super::keychain::hex(value.as_bytes())
}

/// The command that writes `value`: always through `security -i`'s stdin, as hex. Refused for a
/// service or account that couldn't be quoted safely, and for a value whose line wouldn't fit.
pub fn plan_write(service: &str, account: &str, value: &str) -> Result<WritePlan, KeychainError> {
    let line = add_line(service, account, value)?;
    Ok(WritePlan {
        args: vec!["-i".into()],
        stdin: line.expose().to_string(),
    })
}

/// `/usr/bin/security`, through [`SystemSecurity`] (the one place that launches it; macOS only,
/// elsewhere nothing is read or written).
pub struct SecurityTool;

impl SecurityTool {
    /// Runs `security`; the exit code and what it printed. Standard error is dropped.
    fn run(args: &[String], input: Option<&str>) -> Option<(i32, String)> {
        let out = SystemSecurity
            .run(args, input.map(str::as_bytes))
            .ok()?;
        Some((out.status, String::from_utf8_lossy(&out.stdout).into_owned()))
    }
}

impl Keychain for SecurityTool {
    fn read(&self, service: &str, account: &str) -> Option<String> {
        let args = ["find-generic-password", "-s", service, "-a", account, "-w"]
            .map(String::from)
            .to_vec();
        let (code, out) = SecurityTool::run(&args, None)?;
        let text = out.trim();
        (code == 0 && !text.is_empty()).then(|| text.to_string())
    }

    fn write(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError> {
        let plan = plan_write(service, account, value)?;
        match SecurityTool::run(&plan.args, Some(&plan.stdin)) {
            Some((0, _)) => Ok(()),
            Some((status, _)) => Err(KeychainError::Refused(status)),
            None => Err(KeychainError::Run("security didn't finish".into())),
        }
    }

    fn delete(&self, service: &str, account: &str) {
        let args = ["delete-generic-password", "-s", service, "-a", account]
            .map(String::from)
            .to_vec();
        let _ = SecurityTool::run(&args, None);
    }
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

    /// Writes fail from now on.
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

    fn write(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError> {
        lock(&self.calls).push(format!("write {service}/{account}"));
        // The real one's refusals: a name it can't quote, a value too long for its line.
        plan_write(service, account, value)?;
        if *lock(&self.fail_writes) {
            return Err(KeychainError::Refused(1));
        }
        lock(&self.items).insert((service.into(), account.into()), value.into());
        Ok(())
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
        let stdin = plan.stdin;
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
    fn a_value_too_long_for_stdin_is_refused_and_never_put_on_the_command_line() {
        let value = "x".repeat(3000);
        assert_eq!(
            plan_write("gemini", "antigravity", &value),
            Err(KeychainError::TooLong)
        );
        let fake = FakeKeychain::new();
        assert_eq!(
            fake.write("gemini", "antigravity", &value),
            Err(KeychainError::TooLong)
        );
        assert_eq!(fake.get("gemini", "antigravity"), None);
        // The longest that fits still goes in on stdin.
        let fits = "x".repeat(1900);
        assert_eq!(plan_write("gemini", "antigravity", &fits).unwrap().args, vec!["-i"]);
    }

    #[test]
    fn a_name_that_would_break_the_quoting_is_refused() {
        for bad in ["a\"b", "a\\b", "a\nb", ""] {
            assert_eq!(plan_write(bad, "ok", "v"), Err(KeychainError::BadName), "{bad:?}");
            assert_eq!(plan_write("ok", bad, "v"), Err(KeychainError::BadName), "{bad:?}");
        }
        let fake = FakeKeychain::new();
        assert_eq!(fake.write("ok", "a\"b", "v"), Err(KeychainError::BadName));
    }

    #[test]
    fn the_fake_keeps_items_and_counts_calls() {
        let fake = FakeKeychain::new().with("s", "a", "v");
        assert_eq!(fake.read("s", "a").as_deref(), Some("v"));
        assert_eq!(fake.write("s", "b", "w"), Ok(()));
        fake.delete("s", "a");
        assert_eq!(fake.read("s", "a"), None);
        assert_eq!(fake.calls().len(), 4);
        let shown = format!("{:?}", plan_write("s", "a", "secret").unwrap());
        assert!(!shown.contains("secret") && !shown.contains(&hex("secret")));
    }
}
