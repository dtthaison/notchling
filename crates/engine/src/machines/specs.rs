use super::registry::MachineAbilities;
use std::collections::BTreeMap;

pub const SPEC_KEYS: [&str; 4] = ["cpus", "cpu_model", "mem_kb", "disk_total_kb"];

const KB_PER_GB: i64 = 1_048_576;
const MODEL_LENGTH: usize = 80;
const MAX_CORES: i64 = 4096;

pub fn ram_gb(kb: i64) -> Option<i64> {
    (kb > 0).then(|| kb.saturating_add(KB_PER_GB - 1) / KB_PER_GB)
}

pub fn disk_gb(kb: i64) -> Option<i64> {
    (kb > 0).then(|| (kb.saturating_add(KB_PER_GB / 2) / KB_PER_GB).max(1))
}

pub fn free_gb(kb: i64) -> Option<i64> {
    (kb >= 0).then(|| kb.saturating_add(KB_PER_GB / 2) / KB_PER_GB)
}

pub fn cpu_model(text: &str) -> Option<String> {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = joined.chars().take(MODEL_LENGTH).collect();
    let cut = cut.trim().to_owned();
    cut.chars().any(char::is_alphanumeric).then_some(cut)
}

fn number(kv: &BTreeMap<String, String>, key: &str) -> Option<i64> {
    kv.get(key).and_then(|v| v.trim().parse::<i64>().ok())
}

pub fn apply(abilities: &mut MachineAbilities, kv: &BTreeMap<String, String>, now: f64) {
    if let Some(n) = number(kv, "cpus").filter(|n| (1..=MAX_CORES).contains(n)) {
        abilities.cpu_cores = Some(n);
    }
    if let Some(model) = kv.get("cpu_model").and_then(|m| cpu_model(m)) {
        abilities.cpu_model = Some(model);
    }
    if let Some(gb) = number(kv, "mem_kb").and_then(ram_gb) {
        abilities.ram_gb = Some(gb);
    }
    if let Some(gb) = number(kv, "disk_total_kb").and_then(disk_gb) {
        abilities.disk_gb = Some(gb);
    }
    if now.is_finite() && now > 0.0 {
        abilities.checked_at = Some(now as i64);
    }
}
