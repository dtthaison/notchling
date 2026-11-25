pub const MACHINE_RULE: &str = "Name a machine after the machine itself, in lowercase with dashes: m1, vps-sing-1, build-mac. Never a generic word like server, host, new or machine.";

pub const CLUSTER_RULE: &str = "Name a cluster after its purpose or organization, in lowercase with dashes: fleet-workers, acme-builds. Never a generic word like cluster, team or new.";

const MACHINE_WORDS: &[&str] = &[
    "box",
    "computer",
    "default",
    "dev",
    "host",
    "hosts",
    "instance",
    "laptop",
    "linux",
    "local",
    "mac",
    "machine",
    "machines",
    "main",
    "my",
    "new",
    "node",
    "nodes",
    "pc",
    "prod",
    "production",
    "remote",
    "server",
    "servers",
    "stag",
    "staging",
    "test",
    "vm",
    "vps",
    "worker",
    "workers",
];

const CLUSTER_WORDS: &[&str] = &[
    "cluster",
    "clusters",
    "company",
    "default",
    "dev",
    "group",
    "machines",
    "main",
    "my",
    "network",
    "new",
    "nodes",
    "org",
    "organization",
    "prod",
    "production",
    "servers",
    "shared",
    "stag",
    "staging",
    "team",
    "test",
];

fn without_numbers(name: &str) -> String {
    name.split(['-', '_', '.'])
        .filter(|part| !part.is_empty() && !part.chars().all(|c| c.is_ascii_digit()))
        .collect::<Vec<_>>()
        .join("-")
}

pub fn machine_generic(name: &str) -> bool {
    generic_in(&without_numbers(name), MACHINE_WORDS)
}

pub fn cluster_generic(name: &str) -> bool {
    generic_in(&without_numbers(name), CLUSTER_WORDS)
}

pub fn machine_refusal(name: &str) -> String {
    format!("\"{name}\" doesn't say which machine it is. {MACHINE_RULE}")
}

pub fn cluster_refusal(name: &str) -> String {
    format!("\"{name}\" doesn't say what the cluster is for. {CLUSTER_RULE}")
}

pub fn generic_in(name: &str, words: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    let mut parts = lower
        .split(['-', '_', '.'])
        .filter(|part| !part.is_empty())
        .peekable();
    parts.peek().is_some() && parts.all(|part| words.contains(&part))
}

#[cfg(test)]
#[path = "naming_tests.rs"]
mod tests;
