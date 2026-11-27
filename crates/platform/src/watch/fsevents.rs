use std::ffi::{c_char, c_void, CStr, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::sys_mac::{CFRelease, CfString};

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Event {
    pub paths: Vec<PathBuf>,
    pub rescan: bool,
}

const CREATE_NO_DEFER: u32 = 0x02;
const CREATE_WATCH_ROOT: u32 = 0x04;
const CREATE_FILE_EVENTS: u32 = 0x10;

const MUST_SCAN_SUB_DIRS: u32 = 0x01;
const USER_DROPPED: u32 = 0x02;
const KERNEL_DROPPED: u32 = 0x04;
const EVENT_IDS_WRAPPED: u32 = 0x08;
const ROOT_CHANGED: u32 = 0x20;
const MOUNT: u32 = 0x40;
const UNMOUNT: u32 = 0x80;
const ITEM_CREATED: u32 = 0x100;
const ITEM_REMOVED: u32 = 0x200;
const ITEM_INODE_META_MOD: u32 = 0x400;
const ITEM_RENAMED: u32 = 0x800;
const ITEM_MODIFIED: u32 = 0x1000;
const ITEM_CLONED: u32 = 0x40_0000;

const RESCAN: u32 = MUST_SCAN_SUB_DIRS
    | USER_DROPPED
    | KERNEL_DROPPED
    | EVENT_IDS_WRAPPED
    | ROOT_CHANGED
    | MOUNT
    | UNMOUNT;
const CHANGE: u32 =
    ITEM_CREATED | ITEM_REMOVED | ITEM_INODE_META_MOD | ITEM_RENAMED | ITEM_MODIFIED | ITEM_CLONED;

const SINCE_NOW: u64 = u64::MAX;

pub(super) type Roots = Vec<(PathBuf, PathBuf)>;

pub(super) fn real_roots(given: &[PathBuf]) -> Roots {
    given
        .iter()
        .filter_map(|g| Some((std::fs::canonicalize(g).ok()?, g.clone())))
        .collect()
}

pub(super) fn translate(path: &Path, flags: u32, roots: &Roots) -> (Vec<PathBuf>, bool) {
    let rescan = flags & RESCAN != 0;
    if flags & CHANGE == 0 {
        return (Vec::new(), rescan);
    }
    let paths = roots
        .iter()
        .filter_map(|(real, given)| {
            let rest = path.strip_prefix(real).ok()?;
            (!rest.as_os_str().is_empty()).then(|| given.join(rest))
        })
        .collect();
    (paths, rescan)
}

#[repr(C)]
struct Context {
    version: isize,
    info: *mut c_void,
    retain: *const c_void,
    release: *const c_void,
    copy_description: *const c_void,
}

type Callback =
    extern "C" fn(*const c_void, *mut c_void, usize, *mut c_void, *const u32, *const u64);

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn FSEventStreamCreate(
        allocator: *const c_void,
        callback: Callback,
        context: *const Context,
        paths: *const c_void,
        since_when: u64,
        latency: f64,
        flags: u32,
    ) -> *mut c_void;
    fn FSEventStreamSetDispatchQueue(stream: *mut c_void, queue: *mut c_void);
    fn FSEventStreamStart(stream: *mut c_void) -> u8;
    fn FSEventStreamStop(stream: *mut c_void);
    fn FSEventStreamInvalidate(stream: *mut c_void);
    fn FSEventStreamRelease(stream: *mut c_void);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFTypeArrayCallBacks: c_void;
    fn CFArrayCreate(
        allocator: *const c_void,
        values: *const *const c_void,
        count: isize,
        callbacks: *const c_void,
    ) -> *const c_void;
}

extern "C" {
    fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> *mut c_void;
    fn dispatch_sync_f(queue: *mut c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn dispatch_release(object: *mut c_void);
}

struct Shared {
    roots: Roots,
    out: Sender<Event>,
}

extern "C" fn on_events(
    _stream: *const c_void,
    info: *mut c_void,
    count: usize,
    paths: *mut c_void,
    flags: *const u32,
    _ids: *const u64,
) {
    // SAFETY: `info` is the `Shared` the stream was made with, alive until after the stream is
    // invalidated and its queue drained; without kFSEventStreamCreateFlagUseCFTypes `paths` is an
    // array of `count` C strings and `flags` has `count` entries.
    let (shared, paths, flags) = unsafe {
        (
            &*(info as *const Shared),
            std::slice::from_raw_parts(paths as *const *const c_char, count),
            std::slice::from_raw_parts(flags, count),
        )
    };
    let mut event = Event::default();
    for (path, flag) in paths.iter().zip(flags) {
        // SAFETY: each entry is a NUL-terminated string owned by FSEvents for this call.
        let bytes = unsafe { CStr::from_ptr(*path) }.to_bytes();
        let (changed, rescan) =
            translate(Path::new(OsStr::from_bytes(bytes)), *flag, &shared.roots);
        event.paths.extend(changed);
        event.rescan |= rescan;
    }
    if event.rescan || !event.paths.is_empty() {
        let _ = shared.out.send(event);
    }
}

extern "C" fn nothing(_: *mut c_void) {}

pub(super) struct Stream {
    stream: *mut c_void,
    queue: *mut c_void,
    shared: *mut Shared,
}

// SAFETY: the stream and queue are only touched through FSEvents and libdispatch calls, which may
// come from any thread; `shared` is only read by the callback and freed after the queue drains.
unsafe impl Send for Stream {}

impl Stream {
    pub(super) fn start(roots: Roots, latency: Duration, out: Sender<Event>) -> Option<Stream> {
        let mut real: Vec<&Path> = roots.iter().map(|(r, _)| r.as_path()).collect();
        real.sort();
        real.dedup();
        let names: Vec<CfString> = real
            .iter()
            .map(|p| p.to_str().and_then(CfString::new))
            .collect::<Option<_>>()?;
        if names.is_empty() {
            return None;
        }
        let values: Vec<*const c_void> = names.iter().map(|n| n.as_raw()).collect();
        // SAFETY: `values` holds `values.len()` live CFStrings; the array retains them.
        let array = unsafe {
            CFArrayCreate(
                std::ptr::null(),
                values.as_ptr(),
                values.len() as isize,
                &kCFTypeArrayCallBacks as *const c_void,
            )
        };
        if array.is_null() {
            return None;
        }
        let shared = Box::into_raw(Box::new(Shared { roots, out }));
        let context = Context {
            version: 0,
            info: shared.cast(),
            retain: std::ptr::null(),
            release: std::ptr::null(),
            copy_description: std::ptr::null(),
        };
        // SAFETY: every pointer is live for the call; FSEvents copies the context and the paths.
        let stream = unsafe {
            FSEventStreamCreate(
                std::ptr::null(),
                on_events,
                &context,
                array,
                SINCE_NOW,
                latency.as_secs_f64(),
                CREATE_FILE_EVENTS | CREATE_NO_DEFER | CREATE_WATCH_ROOT,
            )
        };
        // SAFETY: the array came from a Create function and is released once.
        unsafe { CFRelease(array) };
        if stream.is_null() {
            // SAFETY: no stream holds `shared`.
            drop(unsafe { Box::from_raw(shared) });
            return None;
        }
        // SAFETY: a static NUL-terminated label; a null attribute makes a serial queue.
        let queue = unsafe { dispatch_queue_create(c"notchling-watch".as_ptr(), std::ptr::null()) };
        let this = Stream {
            stream,
            queue,
            shared,
        };
        // SAFETY: `stream` and `queue` are live; on failure `this` drops, which invalidates.
        unsafe {
            FSEventStreamSetDispatchQueue(stream, queue);
            (FSEventStreamStart(stream) != 0).then_some(this)
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: each call runs once on the live stream and queue. Stopping a stream that never
        // started is allowed. The empty sync waits out a callback already running on the queue,
        // so `shared` is freed when nothing can read it.
        unsafe {
            FSEventStreamStop(self.stream);
            FSEventStreamInvalidate(self.stream);
            dispatch_sync_f(self.queue, std::ptr::null_mut(), nothing);
            FSEventStreamRelease(self.stream);
            dispatch_release(self.queue);
            drop(Box::from_raw(self.shared));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Roots {
        vec![
            ("/private/var/t/w".into(), "/var/t/w".into()),
            (
                "/Users/me/.claude/projects".into(),
                "/Users/me/.claude/projects".into(),
            ),
            (
                "/Users/me/.claude/projects".into(),
                "/Users/me/.claude-lanes/a/projects".into(),
            ),
        ]
    }

    #[test]
    fn a_path_comes_back_in_the_folders_own_spelling() {
        let (paths, rescan) = translate(
            Path::new("/private/var/t/w/sub/f.txt"),
            ITEM_CREATED | 0x1_0000,
            &roots(),
        );
        assert_eq!(paths, vec![PathBuf::from("/var/t/w/sub/f.txt")]);
        assert!(!rescan);
    }

    #[test]
    fn a_folder_linked_from_two_places_reports_under_both() {
        let (paths, _) = translate(
            Path::new("/Users/me/.claude/projects/p/s.jsonl"),
            ITEM_MODIFIED,
            &roots(),
        );
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/Users/me/.claude/projects/p/s.jsonl"),
                PathBuf::from("/Users/me/.claude-lanes/a/projects/p/s.jsonl"),
            ]
        );
    }

    #[test]
    fn dropped_events_and_a_moved_root_ask_for_a_rescan() {
        for flag in [
            MUST_SCAN_SUB_DIRS,
            USER_DROPPED,
            KERNEL_DROPPED,
            ROOT_CHANGED,
        ] {
            let (_, rescan) = translate(Path::new("/private/var/t/w"), flag, &roots());
            assert!(rescan, "{flag:#x}");
        }
    }

    #[test]
    fn the_root_itself_and_paths_elsewhere_are_not_reported() {
        let r = roots();
        assert!(translate(Path::new("/private/var/t/w"), ITEM_REMOVED, &r)
            .0
            .is_empty());
        assert!(
            translate(Path::new("/private/var/t/wx/f"), ITEM_CREATED, &r)
                .0
                .is_empty()
        );
        assert!(translate(Path::new("/elsewhere/f"), ITEM_CREATED, &r)
            .0
            .is_empty());
    }

    #[test]
    fn an_attribute_only_change_is_not_a_change() {
        let (paths, rescan) = translate(Path::new("/private/var/t/w/f"), 0x8000 | 0x2000, &roots());
        assert!(paths.is_empty() && !rescan);
    }
}
