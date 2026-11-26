use super::markdown::image_shape;
use crate::testutil::Scratch;

fn within<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx.recv_timeout(std::time::Duration::from_secs(3)).ok()
}

#[cfg(unix)]
#[test]
fn a_picture_path_that_is_a_pipe_does_not_block_the_render() {
    let s = Scratch::new("shape-fifo");
    let pipe = s.path().join("pipe.png");
    let made = std::process::Command::new("mkfifo").arg(&pipe).status();
    assert!(made.is_ok_and(|m| m.success()));
    let text = pipe.to_string_lossy().into_owned();
    let shape = within(move || image_shape(&text)).expect("image_shape must not block on a pipe");
    assert_eq!(shape, None);
}

#[cfg(unix)]
#[test]
fn a_picture_path_that_is_a_device_has_no_shape() {
    let shape = within(|| image_shape("/dev/zero")).expect("no block");
    assert_eq!(shape, None);
}
