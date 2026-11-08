//! Other apps, by bundle id: is one running, and quit it. Account switches use it (Codex's ChatGPT
//! app, Antigravity, Claude Desktop read their login at launch, so a switch quits them first).
//!
//! macOS asks `NSRunningApplication`, whose `terminate` is the polite quit the Dock's Quit sends.
//! Elsewhere nothing is running as far as this crate can tell, and [`quit_and_wait`] has nothing to
//! do. Opening an app again is `open -b <bundle id>`, which the caller runs.

use std::time::{Duration, Instant};

/// Whether an app with this bundle id is running.
pub fn is_running(bundle_id: &str) -> bool {
    imp::is_running(bundle_id)
}

/// Asks every running copy of the app to quit and waits up to `wait` for them all to be gone.
/// `Ok(true)` when some were running and all quit, `Ok(false)` when none ran, and an error when one
/// was still running at the deadline.
pub fn quit_and_wait(bundle_id: &str, wait: Duration) -> Result<bool, &'static str> {
    if !imp::is_running(bundle_id) {
        return Ok(false);
    }
    imp::request_quit(bundle_id);
    let deadline = Instant::now() + wait;
    while imp::is_running(bundle_id) {
        if Instant::now() >= deadline {
            return Err("the app didn't quit");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(true)
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;

    pub fn is_running(bundle_id: &str) -> bool {
        let id = NSString::from_str(bundle_id);
        let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&id);
        (0..apps.len()).any(|i| !apps.objectAtIndex(i).isTerminated())
    }

    pub fn request_quit(bundle_id: &str) {
        let id = NSString::from_str(bundle_id);
        let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&id);
        for i in 0..apps.len() {
            apps.objectAtIndex(i).terminate();
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn is_running(_bundle_id: &str) -> bool {
        false
    }

    pub fn request_quit(_bundle_id: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_app_that_does_not_exist_is_not_running_and_nothing_is_quit() {
        let id = "invalid.notchling.test.no-such-app";
        assert!(!is_running(id));
        assert_eq!(quit_and_wait(id, Duration::from_millis(10)), Ok(false));
    }
}
