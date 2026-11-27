use crate::scratch::Scratch;

#[test]
fn two_scratch_folders_never_share_a_path() {
    let a = Scratch::new("nl-scratch-test").expect("first");
    let b = Scratch::new("nl-scratch-test").expect("second");
    assert_ne!(a.path(), b.path());
    assert!(a.path().is_dir() && b.path().is_dir());
}

#[test]
fn a_scratch_folder_is_removed_with_its_files() {
    let path = {
        let s = Scratch::new("nl-scratch-test").expect("scratch");
        std::fs::write(s.join("table.bin"), b"x").expect("write");
        s.path().to_path_buf()
    };
    assert!(!path.exists());
}

#[cfg(unix)]
#[test]
fn a_scratch_folder_is_private_to_its_owner() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("nl-scratch-test").expect("scratch");
    let mode = std::fs::metadata(s.path())
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}
