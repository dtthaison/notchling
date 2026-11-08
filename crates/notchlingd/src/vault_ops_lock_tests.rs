//! The vault while a Keychain read waits (on macOS it can wait on the user's prompt for as long
//! as the prompt stays up): turning the vault off answers at once, and a use that was reading
//! answers "off", never the values.

use crate::acting_ops_tests::{app, FakeRoom};
use crate::switches::{SwitchOps, Switches};
use crate::vault_ops::VaultOps;
use notchling_engine::vault::{KeychainError, VaultKeychain, VaultStore};
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode, Tier};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const TOKEN: &str = "fake-token-value-0003";

/// An item whose reads wait for the test while `armed`.
#[derive(Clone)]
struct GatedItem {
    data: Arc<Mutex<Option<Vec<u8>>>>,
    reads: Arc<Mutex<u32>>,
    armed: Arc<AtomicBool>,
    entered: Arc<Mutex<Sender<()>>>,
    release: Arc<Mutex<Receiver<()>>>,
}

impl VaultKeychain for GatedItem {
    fn read(&self) -> Result<Option<Vec<u8>>, KeychainError> {
        *self.reads.lock().unwrap() += 1;
        if self.armed.load(Ordering::SeqCst) {
            let _ = self.entered.lock().unwrap().send(());
            let _ = self
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(30));
        }
        Ok(self.data.lock().unwrap().clone())
    }
    fn write(&self, data: &[u8]) -> Result<(), KeychainError> {
        *self.data.lock().unwrap() = Some(data.to_vec());
        Ok(())
    }
}

fn agent() -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: Some("/fake/peer/notchling".into()),
        pid: Some(std::process::id()),
    }
}

fn set_vault(switch_ops: &SwitchOps, on: bool) {
    use crate::op_group::OpGroup;
    switch_ops
        .call(
            &app(),
            ops::SETTINGS_SET_SWITCH,
            &json!({"switch": "vault", "on": on}),
        )
        .unwrap();
}

#[test]
fn turning_the_vault_off_never_waits_for_a_keychain_read_and_the_use_answers_off() {
    let dir = std::env::temp_dir().join(format!("nl-vault-lock-ops-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (entered_tx, entered) = channel();
    let (release, release_rx) = channel();
    let item = GatedItem {
        data: Arc::new(Mutex::new(None)),
        reads: Arc::new(Mutex::new(0)),
        armed: Arc::new(AtomicBool::new(false)),
        entered: Arc::new(Mutex::new(entered_tx)),
        release: Arc::new(Mutex::new(release_rx)),
    };
    let room = Arc::new(FakeRoom::default());
    let switches = Arc::new(Switches::off());
    let switch_ops = Arc::new(SwitchOps::new(switches.clone(), room.clone()));
    let vault = Arc::new(VaultOps::new(
        switches.vault.clone(),
        VaultStore::new(dir.join("daemon/vault.json"), Box::new(item.clone())),
        room,
    ));
    set_vault(&switch_ops, true);
    vault
        .call(
            &app(),
            ops::VAULT_SAVE,
            &json!({"entry": {"name": "gh", "kind": "token"}, "value": {"secret": TOKEN}}),
        )
        .unwrap();
    // Off and on again: the saved value is no longer in memory, so the use reads the item.
    set_vault(&switch_ops, false);
    set_vault(&switch_ops, true);
    item.armed.store(true, Ordering::SeqCst);
    let user = {
        let vault = vault.clone();
        std::thread::spawn(move || {
            vault.call(
                &agent(),
                ops::VAULT_USE,
                &json!({"names": ["gh"], "folder": "/tmp", "command": "true"}),
            )
        })
    };
    entered
        .recv_timeout(Duration::from_secs(5))
        .expect("the use reads the item");
    let (done_tx, done) = channel();
    {
        let switch_ops = switch_ops.clone();
        std::thread::spawn(move || {
            set_vault(&switch_ops, false);
            let _ = done_tx.send(());
        });
    }
    let answered = done.recv_timeout(Duration::from_secs(2));
    release.send(()).unwrap();
    assert!(
        answered.is_ok(),
        "settings.set_switch waited for the Keychain read"
    );
    let used = user.join().unwrap();
    let e = used.expect_err("the vault went off during the read: no values");
    assert_eq!(e.code, ErrorCode::Forbidden, "{}", e.message);
    assert!(!format!("{e:?}").contains(TOKEN));
    // What the read got was not kept: on again, the next use reads the item again.
    item.armed.store(false, Ordering::SeqCst);
    set_vault(&switch_ops, true);
    let reads = *item.reads.lock().unwrap();
    let got = vault
        .call(
            &agent(),
            ops::VAULT_USE,
            &json!({"names": ["gh"], "folder": "/tmp", "command": "true"}),
        )
        .unwrap();
    assert_eq!(got["env"]["GH_TOKEN"], json!(TOKEN), "{got}");
    assert_eq!(*item.reads.lock().unwrap(), reads + 1);
    let _ = std::fs::remove_dir_all(&dir);
}
