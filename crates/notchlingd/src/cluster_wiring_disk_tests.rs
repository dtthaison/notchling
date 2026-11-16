use super::{on_disk, IDENTITY_DIR};
use crate::cluster_testkit::Dir;
use crate::grant_use::tests::{id, member};
use crate::grant_use::OnDisk;
use notchling_cluster_net::load_or_make;
use notchling_engine::cluster::persist::{save, CLUSTER_DB};
use notchling_engine::cluster::state::{start, Command};
use notchling_engine::cluster::NodeId;

fn persisted(dir: &Dir) {
    let (_, entry, changes) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "home".to_string(),
            home: member("m5"),
        },
        1,
    )
    .expect("create");
    save(dir.path(), &entry, &changes).expect("save");
}

#[test]
fn no_persisted_cluster_is_nothing() {
    let dir = Dir::new("on-disk");
    assert!(matches!(on_disk(dir.path()), OnDisk::Nothing));
}

#[test]
fn a_persisted_cluster_names_its_vault_and_the_own_id_from_the_identity() {
    let dir = Dir::new("on-disk");
    persisted(&dir);
    match on_disk(dir.path()) {
        OnDisk::Cluster { vault, me } => {
            assert_eq!(vault, id("m5"));
            assert_eq!(me, None, "no identity yet, and none is made");
        }
        other => panic!("{other:?}"),
    }
    assert!(!dir.path().join(IDENTITY_DIR).exists());
    let made = load_or_make(&dir.path().join(IDENTITY_DIR)).expect("identity");
    match on_disk(dir.path()) {
        OnDisk::Cluster { me, .. } => {
            assert_eq!(me, NodeId::from_fingerprint(&made.fingerprint));
            assert!(me.is_some());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_broken_identity_gives_no_own_id() {
    let dir = Dir::new("on-disk");
    persisted(&dir);
    let node = dir.path().join(IDENTITY_DIR);
    std::fs::create_dir_all(&node).unwrap();
    std::fs::write(node.join("node.pem"), b"not a certificate").unwrap();
    match on_disk(dir.path()) {
        OnDisk::Cluster { me, .. } => assert_eq!(me, None),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_cluster_file_that_cannot_be_read_is_unreadable_not_nothing() {
    let dir = Dir::new("on-disk");
    let db = dir.path().join(CLUSTER_DB);
    {
        let conn = rusqlite::Connection::open(&db).expect("db");
        conn.execute_batch("CREATE TABLE odd (x); PRAGMA user_version = 99;")
            .expect("batch");
    }
    assert!(matches!(on_disk(dir.path()), OnDisk::Unreadable));
}
