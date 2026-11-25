use super::entry::{Kind, VaultEntry};
use super::store::{KeychainError, VaultKeychain, VaultStore};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct GatedItem {
    pub data: Arc<Mutex<Option<Vec<u8>>>>,
    pub reads: Arc<Mutex<u32>>,
    entered: Arc<Mutex<Sender<()>>>,
    release: Arc<Mutex<Receiver<()>>>,
}

pub(crate) struct Gate {
    pub entered: Receiver<()>,
    pub release: Sender<()>,
}

impl GatedItem {
    pub(crate) fn new(data: &str) -> (GatedItem, Gate) {
        let (entered_tx, entered_rx) = channel();
        let (release_tx, release_rx) = channel();
        let item = GatedItem {
            data: Arc::new(Mutex::new(Some(data.as_bytes().to_vec()))),
            reads: Arc::new(Mutex::new(0)),
            entered: Arc::new(Mutex::new(entered_tx)),
            release: Arc::new(Mutex::new(release_rx)),
        };
        (
            item,
            Gate {
                entered: entered_rx,
                release: release_tx,
            },
        )
    }
}

impl VaultKeychain for GatedItem {
    fn read(&self) -> Result<Option<Vec<u8>>, KeychainError> {
        *self.reads.lock().unwrap() += 1;
        let _ = self.entered.lock().unwrap().send(());
        let _ = self
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(30));
        Ok(self.data.lock().unwrap().clone())
    }
    fn write(&self, data: &[u8]) -> Result<(), KeychainError> {
        *self.data.lock().unwrap() = Some(data.to_vec());
        Ok(())
    }
}

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-vault-lock-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn forget_never_waits_for_a_keychain_read_and_the_read_doesnt_refill_the_cache() {
    let dir = temp("forget");
    let (item, gate) = GatedItem::new(r#"{"npm":{"username":"","secret":"fake-npm-0001"}}"#);
    let store = Arc::new(VaultStore::new(
        super::list::VaultIndex::in_data(&dir),
        Box::new(item.clone()),
    ));
    let entry = VaultEntry::new("npm", Kind::Token);
    let reader = {
        let (store, entry) = (store.clone(), entry.clone());
        std::thread::spawn(move || store.value(&entry))
    };
    gate.entered
        .recv_timeout(Duration::from_secs(5))
        .expect("the read started");
    let (done_tx, done_rx) = channel();
    {
        let store = store.clone();
        std::thread::spawn(move || {
            store.forget();
            let _ = done_tx.send(());
        });
    }
    let forgot = done_rx.recv_timeout(Duration::from_secs(2));
    gate.release.send(()).unwrap();
    assert!(forgot.is_ok(), "forget waited for the Keychain read");
    let got = reader.join().unwrap().unwrap();
    assert_eq!(got.map(|v| v.secret).as_deref(), Some("fake-npm-0001"));
    gate.release.send(()).unwrap();
    store.value(&entry).unwrap();
    assert_eq!(*item.reads.lock().unwrap(), 2, "the next use reads again");
    let _ = std::fs::remove_dir_all(&dir);
}
