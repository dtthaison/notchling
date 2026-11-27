use std::time::Duration;

use gpui::{App, ClipboardItem, Context};
use objc2::rc::Retained;
use objc2::{msg_send, ClassType, MainThreadMarker};
use objc2_app_kit::NSPasteboard;
use objc2_foundation::{NSArray, NSString};

use super::reveal::{Written, CLIPBOARD_CLEAR_MS};

const STRING: &str = "public.utf8-plain-text";
const CONCEALED: &str = "org.nspasteboard.ConcealedType";
const TRANSIENT: &str = "org.nspasteboard.TransientType";

trait Board {
    fn declare(&self, kinds: &[&str]) -> i64;
    fn put(&self, kind: &str, text: &str) -> bool;
    fn count(&self) -> i64;
    fn clear(&self);
}

struct General(Retained<NSPasteboard>);

impl General {
    fn open(_main: MainThreadMarker) -> Option<General> {
        let board: Option<Retained<NSPasteboard>> =
            unsafe { msg_send![NSPasteboard::class(), generalPasteboard] };
        board.map(General)
    }
}

impl Board for General {
    fn declare(&self, kinds: &[&str]) -> i64 {
        let kinds: Vec<Retained<NSString>> = kinds.iter().map(|k| NSString::from_str(k)).collect();
        unsafe {
            self.0
                .declareTypes_owner(&NSArray::from_retained_slice(&kinds), None) as i64
        }
    }

    fn put(&self, kind: &str, text: &str) -> bool {
        self.0
            .setString_forType(&NSString::from_str(text), &NSString::from_str(kind))
    }

    fn count(&self) -> i64 {
        self.0.changeCount() as i64
    }

    fn clear(&self) {
        self.0.clearContents();
    }
}

fn write_marked(board: &impl Board, text: &str) -> Option<Written> {
    let declared = Written(board.declare(&[STRING, CONCEALED, TRANSIENT]));
    let wrote = board.put(CONCEALED, "") && board.put(TRANSIENT, "") && board.put(STRING, text);
    if wrote {
        return Some(declared);
    }
    if declared.clears(board.count()) {
        board.clear();
    }
    None
}

fn write_concealed(main: MainThreadMarker, text: &str) -> Option<Written> {
    write_marked(&General::open(main)?, text)
}

fn clear_if_ours(main: MainThreadMarker, written: Written) {
    if let Some(board) = General::open(main) {
        if written.clears(board.count()) {
            board.clear();
        }
    }
}

pub fn copy_secret<V: 'static>(value: &str, cx: &mut Context<V>) -> bool {
    let Some(main) = MainThreadMarker::new() else {
        return false;
    };
    let Some(written) = write_concealed(main, value) else {
        return false;
    };
    cx.spawn(async move |_, cx| {
        cx.background_executor()
            .timer(Duration::from_millis(CLIPBOARD_CLEAR_MS))
            .await;
        let _ = cx.update(|_: &mut App| {
            if let Some(main) = MainThreadMarker::new() {
                clear_if_ours(main, written);
            }
        });
    })
    .detach();
    true
}

pub fn copy_plain<V: 'static>(value: &str, cx: &mut Context<V>) {
    cx.write_to_clipboard(ClipboardItem::new_string(value.to_owned()));
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    struct Fake {
        log: RefCell<Vec<String>>,
        count: Cell<i64>,
        fail: Option<&'static str>,
        another_app_writes: bool,
    }

    impl Fake {
        fn new(fail: Option<&'static str>, another_app_writes: bool) -> Fake {
            Fake {
                log: RefCell::default(),
                count: Cell::new(40),
                fail,
                another_app_writes,
            }
        }

        fn log(&self) -> Vec<String> {
            self.log.borrow().clone()
        }
    }

    impl Board for Fake {
        fn declare(&self, kinds: &[&str]) -> i64 {
            self.count.set(self.count.get() + 1);
            self.log
                .borrow_mut()
                .push(format!("declare {}", kinds.join(",")));
            self.count.get()
        }

        fn put(&self, kind: &str, _text: &str) -> bool {
            if self.another_app_writes && kind == CONCEALED {
                self.count.set(self.count.get() + 1);
            }
            if self.fail == Some(kind) {
                return false;
            }
            self.log.borrow_mut().push(format!("put {kind}"));
            true
        }

        fn count(&self) -> i64 {
            self.count.get()
        }

        fn clear(&self) {
            self.log.borrow_mut().push("clear".to_owned());
        }
    }

    #[test]
    fn every_type_is_declared_before_the_secret_goes_on_the_board_and_the_markers_go_first() {
        let board = Fake::new(None, false);
        let written = write_marked(&board, "fake-secret").unwrap();
        let log = board.log();
        assert_eq!(
            log[0],
            format!("declare {STRING},{CONCEALED},{TRANSIENT}"),
            "{log:?}"
        );
        assert_eq!(
            log[1..],
            [
                format!("put {CONCEALED}"),
                format!("put {TRANSIENT}"),
                format!("put {STRING}")
            ]
        );
        assert_eq!(written, Written(41));
        assert!(written.clears(board.count()));
    }

    #[test]
    fn a_refused_write_clears_the_board_only_while_it_is_still_ours() {
        for failing in [STRING, CONCEALED, TRANSIENT] {
            let ours = Fake::new(Some(failing), false);
            assert_eq!(write_marked(&ours, "fake-secret"), None);
            assert_eq!(ours.log().last().map(String::as_str), Some("clear"));
            let taken = Fake::new(Some(failing), true);
            assert_eq!(write_marked(&taken, "fake-secret"), None);
            assert!(
                !taken.log().contains(&"clear".to_owned()),
                "another app's copy is not wiped: {:?}",
                taken.log()
            );
        }
    }

    #[test]
    fn the_secret_is_never_put_before_a_marker_that_failed() {
        let board = Fake::new(Some(CONCEALED), false);
        assert_eq!(write_marked(&board, "fake-secret"), None);
        assert!(!board.log().contains(&format!("put {STRING}")));
    }
}
