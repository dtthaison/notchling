pub use super::keychain::KeychainError;
use super::keychain::{add_line, SecurityRunner, SystemSecurity};
use std::collections::HashMap;
use std::sync::Mutex;

pub trait Keychain: Send + Sync {
    fn read(&self, service: &str, account: &str) -> Option<String>;
    fn write(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError>;
    fn delete(&self, service: &str, account: &str);
}

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

pub fn plan_write(service: &str, account: &str, value: &str) -> Result<WritePlan, KeychainError> {
    let line = add_line(service, account, value)?;
    Ok(WritePlan {
        args: vec!["-i".into()],
        stdin: line.expose().to_string(),
    })
}

pub struct SecurityTool;

impl SecurityTool {
    fn run(args: &[String], input: Option<&str>) -> Option<(i32, String)> {
        let out = SystemSecurity.run(args, input.map(str::as_bytes)).ok()?;
        Some((
            out.status,
            String::from_utf8_lossy(&out.stdout).into_owned(),
        ))
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

    pub fn set_as_other_tool(&self, service: &str, account: &str, value: &str) {
        lock(&self.items).insert((service.into(), account.into()), value.into());
    }

    pub fn fail_writes(&self) {
        *lock(&self.fail_writes) = true;
    }

    pub fn get(&self, service: &str, account: &str) -> Option<String> {
        lock(&self.items)
            .get(&(service.to_string(), account.to_string()))
            .cloned()
    }

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
        let fits = "x".repeat(1900);
        assert_eq!(
            plan_write("gemini", "antigravity", &fits).unwrap().args,
            vec!["-i"]
        );
    }

    #[test]
    fn a_name_that_would_break_the_quoting_is_refused() {
        for bad in ["a\"b", "a\\b", "a\nb", ""] {
            assert_eq!(
                plan_write(bad, "ok", "v"),
                Err(KeychainError::BadName),
                "{bad:?}"
            );
            assert_eq!(
                plan_write("ok", bad, "v"),
                Err(KeychainError::BadName),
                "{bad:?}"
            );
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
