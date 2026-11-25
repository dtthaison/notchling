use super::exclude::{excluded, excluded_dir, MAX_FILE};
use crate::cluster::grant_files::EXCLUDED_DIRS;
use crate::sessions::testutil::TempDir;
use std::path::Path;

#[test]
fn grants_and_logins_are_excluded() {
    for dir in EXCLUDED_DIRS {
        let inside = Path::new("/data").join(dir).join("anything.json");
        assert!(excluded(&inside), "{dir}");
        assert!(excluded_dir(&inside), "{dir}");
        assert!(excluded(&Path::new("/data").join(dir)), "{dir}");
    }
    assert!(excluded(Path::new("/data/cluster/logins/claude/token")));
    assert!(excluded(Path::new("/data/cluster/grants/g1.json")));
    assert!(excluded(Path::new("/data/cluster/incoming/x/part")));
    assert!(excluded(Path::new("/data/pages/../cluster/grants")));
    assert!(!excluded(Path::new("/data/cluster/cluster.db")));
    assert!(!excluded(Path::new("/data/pages")));
    assert!(!excluded(Path::new("/data/cluster/grantsish/file")));
}

#[test]
fn big_files_are_excluded() {
    let dir = TempDir::new("exclude-big");
    let big = dir.path("big.bin");
    let file = std::fs::File::create(&big).expect("create");
    file.set_len(MAX_FILE + 1).expect("len");
    assert!(excluded(&big));
    assert!(!excluded_dir(&big));
    let edge = dir.path("edge.bin");
    let file = std::fs::File::create(&edge).expect("create");
    file.set_len(MAX_FILE).expect("len");
    assert!(!excluded(&edge));
    assert!(!excluded(&dir.path("missing")));
    assert!(!excluded(dir.root()));
}
