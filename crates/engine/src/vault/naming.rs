pub const RULE: &str = "Name it <service or host>-<env>-<what>, in lowercase with dashes, so the user can tell what it is from the name alone: workflowspace-cdn-stag-basic-auth, emqx-dashboard-dev-admin. Never a generic word like dashboards, api or login.";

const GENERIC: &[&str] = &[
    "account",
    "admin",
    "api",
    "app",
    "auth",
    "basic",
    "cred",
    "creds",
    "credential",
    "credentials",
    "dashboard",
    "dashboards",
    "db",
    "dev",
    "key",
    "keys",
    "login",
    "pass",
    "password",
    "prod",
    "production",
    "secret",
    "secrets",
    "stag",
    "staging",
    "test",
    "token",
    "tokens",
    "user",
    "username",
    "web",
];

pub use crate::naming::generic_in;

pub fn generic(name: &str) -> bool {
    generic_in(name, GENERIC)
}

pub fn refusal(name: &str) -> String {
    format!("\"{name}\" doesn't say which service it is for. {RULE}")
}

#[cfg(test)]
#[path = "naming_tests.rs"]
mod tests;
