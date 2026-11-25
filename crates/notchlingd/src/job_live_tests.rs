use super::*;
use crate::limits_ops_tests::TempDir;
use notchling_engine::machines::roots::MachineRoots;
use notchling_engine::organizations::Kind as OrgKind;
use notchling_engine::vault::list::VaultIndex;
use notchling_engine::vault::{Kind, VaultEntry};

fn fleet(tmp: &TempDir) -> LiveFleet {
    let dir = tmp.path("data");
    std::fs::create_dir_all(&dir).unwrap();
    let index = VaultIndex::in_data(&dir);
    index
        .update(|mut list| {
            list.push(VaultEntry::new("deploy-token", Kind::Token));
            list.push(VaultEntry::new("plain-token", Kind::Token));
            Ok(list)
        })
        .unwrap();
    let orgs = Organizations::new(&dir);
    orgs.create("Acme", None, None).unwrap();
    orgs.set_organization(OrgKind::Vault, "deploy-token", "acme")
        .unwrap();
    let registry = RegistryFiles {
        read_only: true,
        ..MachineRoots::in_data(&dir).registry()
    };
    LiveFleet::new(Err("no node".to_string()), registry, orgs)
}

#[test]
fn a_secret_not_in_the_vault_has_no_organization() {
    let tmp = TempDir::new("job-live-missing");
    let fleet = fleet(&tmp);
    assert_eq!(fleet.secret_organization("no-such-secret"), None);
}

#[test]
fn a_secret_in_the_vault_resolves_to_its_organization_whatever_the_case() {
    let tmp = TempDir::new("job-live-case");
    let fleet = fleet(&tmp);
    assert_eq!(
        fleet.secret_organization("deploy-token").as_deref(),
        Some("acme")
    );
    assert_eq!(
        fleet.secret_organization("Deploy-Token").as_deref(),
        Some("acme")
    );
    assert_eq!(
        fleet.secret_organization("plain-token").as_deref(),
        Some(PERSONAL_ID)
    );
}
