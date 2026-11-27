use std::sync::Once;

use gpui::{App, AsyncApp, Context, Global, Subscription};
use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceScreensDidSleepNotification,
    NSWorkspaceScreensDidWakeNotification, NSWorkspaceSessionDidBecomeActiveNotification,
    NSWorkspaceSessionDidResignActiveNotification, NSWorkspaceWillSleepNotification,
};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSNotificationName,
    NSNotificationSuspensionBehavior, NSString,
};

use super::lock_state::{Cause, Lock};
use super::reveal::Seen;

const SCREEN_LOCKED: &str = "com.apple.screenIsLocked";
const SCREEN_UNLOCKED: &str = "com.apple.screenIsUnlocked";

static LOCK: Lock = Lock::new();
static START: Once = Once::new();

struct LockMoved;

impl Global for LockMoved {}

pub fn seen() -> Seen {
    Seen::of(LOCK.epoch())
}

pub fn moved(seen: &mut Seen) -> bool {
    seen.moved(LOCK.epoch())
}

pub fn is_locked() -> bool {
    LOCK.is_locked()
}

pub fn unlocked() -> Option<Seen> {
    (!is_locked()).then(seen)
}

pub fn observe<V: 'static>(
    cx: &mut Context<V>,
    on: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> Subscription {
    start(cx);
    cx.observe_global::<LockMoved>(on)
}

fn start(cx: &App) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    START.call_once(|| register(mtm, cx.to_async()));
}

struct Edge {
    name: Retained<NSNotificationName>,
    cause: Cause,
    begins: bool,
}

struct WatchIvars {
    app: AsyncApp,
    edges: Vec<Edge>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NotchlingLockWatch"]
    #[ivars = WatchIvars]
    struct Watch;

    impl Watch {
        #[unsafe(method(changed:))]
        fn changed(&self, note: &NSNotification) {
            let name = note.name();
            let Some(edge) = self.ivars().edges.iter().find(|e| e.name.isEqualToString(&name))
            else {
                return;
            };
            if edge.begins {
                LOCK.enter(edge.cause);
            } else {
                LOCK.leave(edge.cause);
            }
            self.ivars()
                .app
                .spawn(async |cx: &mut AsyncApp| {
                    cx.update(|cx| cx.set_global(LockMoved)).ok();
                })
                .detach();
        }
    }
);

impl Watch {
    fn new(mtm: MainThreadMarker, ivars: WatchIvars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }
}

fn edge(name: &NSNotificationName, cause: Cause, begins: bool) -> Edge {
    Edge {
        name: name.retain(),
        cause,
        begins,
    }
}

fn register(mtm: MainThreadMarker, app: AsyncApp) {
    let workspace_edges = unsafe {
        [
            edge(NSWorkspaceWillSleepNotification, Cause::Sleep, true),
            edge(NSWorkspaceDidWakeNotification, Cause::Sleep, false),
            edge(NSWorkspaceScreensDidSleepNotification, Cause::Display, true),
            edge(NSWorkspaceScreensDidWakeNotification, Cause::Display, false),
            edge(
                NSWorkspaceSessionDidResignActiveNotification,
                Cause::Session,
                true,
            ),
            edge(
                NSWorkspaceSessionDidBecomeActiveNotification,
                Cause::Session,
                false,
            ),
        ]
    };
    let screen_edges = [
        edge(&NSString::from_str(SCREEN_LOCKED), Cause::Screen, true),
        edge(&NSString::from_str(SCREEN_UNLOCKED), Cause::Screen, false),
    ];
    let edges = workspace_edges.into_iter().chain(screen_edges).collect();
    let watch = Watch::new(mtm, WatchIvars { app, edges });
    let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
    let distributed = NSDistributedNotificationCenter::defaultCenter();
    for edge in &watch.ivars().edges {
        unsafe {
            if edge.cause == Cause::Screen {
                distributed.addObserver_selector_name_object_suspensionBehavior(
                    &watch,
                    sel!(changed:),
                    Some(&edge.name),
                    None,
                    NSNotificationSuspensionBehavior::DeliverImmediately,
                );
            } else {
                workspace.addObserver_selector_name_object(
                    &watch,
                    sel!(changed:),
                    Some(&edge.name),
                    None,
                );
            }
        }
    }
    std::mem::forget(watch);
}
