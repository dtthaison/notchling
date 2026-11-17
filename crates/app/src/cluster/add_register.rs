use notchling_engine::machines::registry::{AdminAccess, Machine, MachineAbilities, MachineOs};
use notchling_engine::machines::shell::is_safe_machine_path;
use notchling_engine::machines::step::Detected;

fn merged(old: Option<MachineAbilities>, new: Option<&MachineAbilities>) -> MachineAbilities {
    let mut out = old.unwrap_or_default();
    let Some(new) = new else {
        return out;
    };
    out.arch = new.arch.clone().or(out.arch);
    out.os_version = new.os_version.clone().or(out.os_version);
    out.home = new
        .home
        .clone()
        .filter(|h| is_safe_machine_path(h))
        .or(out.home);
    out.xcode = new.xcode.clone().or(out.xcode);
    out.docker = new.docker.or(out.docker);
    out.free_disk_gb = new.free_disk_gb.or(out.free_disk_gb);
    out.notchling_app = new.notchling_app.or(out.notchling_app);
    if !new.ios_runtimes.is_empty() {
        out.ios_runtimes = new.ios_runtimes.clone();
    }
    if !new.simulators.is_empty() {
        out.simulators = new.simulators.clone();
    }
    out
}

pub(super) fn learn(machine: &mut Machine, detected: &Detected) {
    if let Some(os) = detected.os {
        let unknown = machine.os.is_none();
        machine.os = Some(os);
        if unknown && os == MachineOs::Linux && machine.admin() == AdminAccess::Sudo {
            machine.set_admin(AdminAccess::RootSsh);
        }
    }
    if let Some(file_vault) = detected.file_vault {
        machine.file_vault = file_vault;
    }
    let had = machine.abilities.is_some();
    let mut abilities = merged(machine.abilities.take(), detected.abilities.as_ref());
    if let Some(home) = detected.home.as_ref().filter(|h| is_safe_machine_path(h)) {
        abilities.home = Some(home.clone());
    }
    if let Some(app) = detected.notchling_app {
        abilities.notchling_app = Some(app);
    }
    if had || abilities != MachineAbilities::default() {
        machine.abilities = Some(abilities);
    }
}
