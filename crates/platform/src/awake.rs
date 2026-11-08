//! Keep the computer awake, and sleep and wake events.
//!
//! macOS: an IOKit power assertion held by [`AwakeGuard`], and IOKit's system power notices for
//! [`PowerEvents`], both through plain C calls (no process is started, no AppKit needed). Other
//! systems return [`Error::Unsupported`] until their implementation lands.

use std::sync::mpsc::Receiver;

use crate::{Error, Result};

/// A sleep or wake notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerEvent {
    /// The system is about to sleep.
    WillSleep,
    /// The system has woken.
    DidWake,
}

/// Keeps the machine from idle-sleeping while it is alive. Dropping it releases the hold.
#[derive(Debug)]
pub struct AwakeGuard {
    #[cfg(target_os = "macos")]
    id: u32,
}

impl AwakeGuard {
    /// Holds the system awake; with `keep_display_on` the display stays on too. `reason` is shown
    /// by `pmset -g assertions`.
    pub fn acquire(reason: &str, keep_display_on: bool) -> Result<AwakeGuard> {
        imp::acquire(reason, keep_display_on)
    }
}

/// A stream of [`PowerEvent`]s. Dropping it stops the notices.
pub struct PowerEvents {
    events: Receiver<PowerEvent>,
    // Held so the watcher thread stops on drop.
    _stop: imp::Monitor,
}

impl PowerEvents {
    /// Starts listening for sleep and wake.
    pub fn start() -> Result<PowerEvents> {
        let (events, monitor) = imp::monitor()?;
        Ok(PowerEvents {
            events,
            _stop: monitor,
        })
    }

    /// The channel the notices arrive on.
    pub fn events(&self) -> &Receiver<PowerEvent> {
        &self.events
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use crate::sys_mac::*;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::mpsc::{self, Sender};
    use std::sync::Arc;
    use std::thread::JoinHandle;

    pub(super) const CAN_SYSTEM_SLEEP: u32 = 0xe000_0270;
    pub(super) const SYSTEM_WILL_SLEEP: u32 = 0xe000_0280;
    pub(super) const SYSTEM_HAS_POWERED_ON: u32 = 0xe000_0300;

    pub(super) fn acquire(reason: &str, keep_display_on: bool) -> Result<AwakeGuard> {
        let kind = if keep_display_on {
            "PreventUserIdleDisplaySleep"
        } else {
            "PreventUserIdleSystemSleep"
        };
        let (Some(kind), Some(name)) = (CfString::new(kind), CfString::new(reason)) else {
            return Err(Error::System("the reason text can't be used".into()));
        };
        let mut id = 0u32;
        // SAFETY: both strings are live CoreFoundation strings and `id` is a valid out pointer.
        let code = unsafe {
            IOPMAssertionCreateWithName(kind.as_raw(), ASSERTION_LEVEL_ON, name.as_raw(), &mut id)
        };
        if code != IO_RETURN_SUCCESS {
            return Err(Error::System(format!(
                "macOS refused the power assertion (IOReturn {code:#x})"
            )));
        }
        Ok(AwakeGuard { id })
    }

    impl Drop for AwakeGuard {
        fn drop(&mut self) {
            // SAFETY: `id` came from a successful create and is released once.
            unsafe { IOPMAssertionRelease(self.id) };
        }
    }

    /// What an IOKit system power message means for us.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Message {
        CanSleep,
        WillSleep,
        PoweredOn,
    }

    pub(super) fn classify(message: u32) -> Option<Message> {
        match message {
            CAN_SYSTEM_SLEEP => Some(Message::CanSleep),
            SYSTEM_WILL_SLEEP => Some(Message::WillSleep),
            SYSTEM_HAS_POWERED_ON => Some(Message::PoweredOn),
            _ => None,
        }
    }

    struct State {
        tx: Sender<PowerEvent>,
        root_port: AtomicU32,
    }

    extern "C" fn on_power(
        refcon: *mut c_void,
        _service: u32,
        message: u32,
        argument: *mut c_void,
    ) {
        if refcon.is_null() {
            return;
        }
        // SAFETY: `refcon` is the `State` the monitor thread keeps alive until it deregisters.
        let state = unsafe { &*(refcon as *const State) };
        let allow = || {
            // Never hold up sleep: say yes to the question and to the notice.
            // SAFETY: plain call with the port IOKit gave us and the id it passed in.
            unsafe {
                IOAllowPowerChange(state.root_port.load(Ordering::SeqCst), argument as isize)
            };
        };
        match classify(message) {
            Some(Message::CanSleep) => allow(),
            Some(Message::WillSleep) => {
                let _ = state.tx.send(PowerEvent::WillSleep);
                allow();
            }
            Some(Message::PoweredOn) => {
                let _ = state.tx.send(PowerEvent::DidWake);
            }
            None => {}
        }
    }

    /// The thread that runs IOKit's notification source; stopped and joined on drop.
    pub(super) struct Monitor {
        stop: Arc<AtomicBool>,
        run_loop: usize,
        thread: Option<JoinHandle<()>>,
    }

    impl Drop for Monitor {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            // SAFETY: `run_loop` was retained by the thread's creator and is released below, after
            // the join. A stop sent before the loop runs is covered by the stop flag.
            unsafe { CFRunLoopStop(self.run_loop as CFRunLoopRef) };
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            // SAFETY: matches the retain made when the thread reported its run loop.
            unsafe { CFRelease(self.run_loop as CFTypeRef) };
        }
    }

    pub(super) fn monitor() -> Result<(Receiver<PowerEvent>, Monitor)> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel::<std::result::Result<usize, String>>();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = std::thread::Builder::new()
            .name("notchling-power".into())
            .spawn(move || listen(tx, ready_tx, thread_stop))
            .map_err(Error::Io)?;
        match ready_rx.recv() {
            Ok(Ok(run_loop)) => Ok((
                rx,
                Monitor {
                    stop,
                    run_loop,
                    thread: Some(thread),
                },
            )),
            Ok(Err(why)) => {
                let _ = thread.join();
                Err(Error::System(why))
            }
            Err(_) => Err(Error::System("the power listener stopped at once".into())),
        }
    }

    fn listen(
        tx: Sender<PowerEvent>,
        ready: Sender<std::result::Result<usize, String>>,
        stop: Arc<AtomicBool>,
    ) {
        let state = Box::new(State {
            tx,
            root_port: AtomicU32::new(0),
        });
        let mut port: *mut c_void = std::ptr::null_mut();
        let mut notifier = 0u32;
        let refcon = &*state as *const State as *mut c_void;
        // SAFETY: out pointers are valid; `refcon` outlives the registration (deregistered below).
        let root = unsafe { IORegisterForSystemPower(refcon, &mut port, on_power, &mut notifier) };
        if root == 0 || port.is_null() {
            let _ = ready.send(Err("macOS refused the sleep and wake registration".into()));
            return;
        }
        state.root_port.store(root, Ordering::SeqCst);
        // SAFETY: `port` is the live notification port; the run loop is this thread's own.
        let run_loop = unsafe {
            let run_loop = CFRunLoopGetCurrent();
            CFRetain(run_loop as CFTypeRef);
            let source = IONotificationPortGetRunLoopSource(port);
            CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
            run_loop
        };
        let _ = ready.send(Ok(run_loop as usize));
        while !stop.load(Ordering::SeqCst) {
            // A short slice so a missed stop still ends the loop; the source wakes it sooner.
            // SAFETY: plain call on this thread's run loop.
            let code = unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.5, 0) };
            if code == 1 {
                break; // kCFRunLoopRunFinished
            }
        }
        // SAFETY: tears down exactly what was set up above, once.
        unsafe {
            IODeregisterForSystemPower(&mut notifier);
            IOServiceClose(root);
            IONotificationPortDestroy(port);
        }
        drop(state);
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn classifies_the_messages_we_act_on() {
            assert_eq!(classify(0xe000_0270), Some(Message::CanSleep));
            assert_eq!(classify(0xe000_0280), Some(Message::WillSleep));
            assert_eq!(classify(0xe000_0300), Some(Message::PoweredOn));
            // WillPowerOn comes before HasPoweredOn; the system isn't up yet.
            assert_eq!(classify(0xe000_0320), None);
            assert_eq!(classify(0), None);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;

    pub(super) struct Monitor;

    pub(super) fn acquire(_reason: &str, _keep_display_on: bool) -> Result<AwakeGuard> {
        Err(Error::Unsupported("keeping the computer awake"))
    }

    pub(super) fn monitor() -> Result<(Receiver<PowerEvent>, Monitor)> {
        Err(Error::Unsupported("sleep and wake notices"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[cfg(target_os = "macos")]
    #[test]
    fn holds_and_releases_an_assertion() {
        let guard = AwakeGuard::acquire("Notchling test: holding awake", false).unwrap();
        drop(guard);
        let display = AwakeGuard::acquire("Notchling test: holding display", true).unwrap();
        drop(display);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reason_with_a_nul_is_refused() {
        assert!(AwakeGuard::acquire("bad\0reason", false).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn power_events_start_and_stop_promptly() {
        let events = PowerEvents::start().unwrap();
        assert!(events.events().try_recv().is_err());
        let started = Instant::now();
        drop(events);
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn other_systems_say_unsupported() {
        assert!(matches!(
            AwakeGuard::acquire("x", false),
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(PowerEvents::start(), Err(Error::Unsupported(_))));
        // Keep the imports used on every system.
        let _ = (Duration::ZERO, Instant::now());
    }
}
