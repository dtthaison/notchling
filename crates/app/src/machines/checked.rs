use notchling_engine::machines::registry::MachineAbilities;
use notchling_locale as t;

use crate::cluster::replication::ago;

pub fn checked_line(abilities: Option<&MachineAbilities>, now: f64) -> Option<String> {
    let at = abilities?.checked_at?;
    let age = u64::try_from((now as i64 - at).max(0)).unwrap_or(0);
    Some(t::cluster::machine_facts_checked(&ago(age)))
}
