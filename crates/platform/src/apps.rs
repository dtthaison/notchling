//! Other apps, by bundle id: is one running, and quit it. Account switches use it (Codex's ChatGPT
//! app, Antigravity, Claude Desktop read their login at launch, so a switch quits them first), and
//! a login is never written while the app runs, so "running" has to be right.
//!
//! macOS reads the kernel's process table, not `NSRunningApplication`: the daemon has no main run
//! loop, so the system's running-application list goes stale there (a quit app still listed, a
//! launched one missing). The bundle id becomes the file its copies run (LaunchServices, a static
//! lookup) and the processes running one of them are the app (`app_procs`). Asking an app to quit
//! still uses `NSRunningApplication::terminate`, the polite quit the Dock's Quit sends; only the
//! waiting and the answer to "running?" come from the process table. When the table can't be read
//! the app counts as running: the caller then waits or refuses, never writes a login.
//! Elsewhere nothing is running as far as this crate can tell, and [`quit_and_wait`] has nothing
//! to do. Opening an app again is `open -b <bundle id>`, which the caller runs.

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
    use crate::app_procs::app_pids;
    use crate::sys_mac::process_paths;
    use objc2_app_kit::{NSRunningApplication, NSWorkspace};
    use objc2_foundation::{NSBundle, NSString};
    use std::collections::HashSet;

    pub fn is_running(bundle_id: &str) -> bool {
        running_pids(bundle_id).is_none_or(|pids| !pids.is_empty())
    }

    /// Asks every running copy to quit: the ones the system lists for the bundle id, and the ones
    /// the process table shows (which the list may be missing). Each is asked once.
    pub fn request_quit(bundle_id: &str) {
        let id = NSString::from_str(bundle_id);
        let listed = NSRunningApplication::runningApplicationsWithBundleIdentifier(&id);
        let mut asked = HashSet::new();
        for i in 0..listed.len() {
            let app = listed.objectAtIndex(i);
            asked.insert(app.processIdentifier());
            app.terminate();
        }
        for pid in running_pids(bundle_id).unwrap_or_default() {
            if asked.contains(&pid) {
                continue;
            }
            if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
                app.terminate();
            }
        }
    }

    /// The pids running the app, `None` when the process table can't be read. An app with no
    /// installed copy has no executable to find, so nothing runs it.
    fn running_pids(bundle_id: &str) -> Option<Vec<i32>> {
        let executables = executables(bundle_id);
        if executables.is_empty() {
            return Some(Vec::new());
        }
        Some(app_pids(&process_paths()?, &executables))
    }

    /// The executable of every installed copy of the app, as written and with symlinks resolved
    /// (the kernel reports the resolved path). LaunchServices answers from its own database, so
    /// this needs no run loop.
    fn executables(bundle_id: &str) -> Vec<String> {
        let id = NSString::from_str(bundle_id);
        let copies = NSWorkspace::sharedWorkspace().URLsForApplicationsWithBundleIdentifier(&id);
        let mut found: Vec<String> = Vec::new();
        for i in 0..copies.len() {
            let Some(bundle) = copies.objectAtIndex(i).path() else {
                continue;
            };
            let Some(exe) = bundle_executable(&bundle.to_string()) else {
                continue;
            };
            let real = std::fs::canonicalize(&exe)
                .ok()
                .and_then(|p| p.to_str().map(str::to_string));
            for path in std::iter::once(exe).chain(real) {
                if !found.contains(&path) {
                    found.push(path);
                }
            }
        }
        found
    }

    /// The file a bundle runs (`CFBundleExecutable` under `Contents/MacOS`); `None` when the
    /// folder isn't a bundle or its executable is missing.
    pub(super) fn bundle_executable(bundle_path: &str) -> Option<String> {
        let bundle = NSBundle::bundleWithPath(&NSString::from_str(bundle_path))?;
        Some(bundle.executableURL()?.path()?.to_string())
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

    #[cfg(target_os = "macos")]
    mod mac {
        use super::super::imp::bundle_executable;
        use crate::app_procs::app_pids;
        use crate::files::testdir::TempDir;
        use crate::sys_mac::process_paths;

        /// A bundle in a temp folder: an Info.plist naming the executable, and the file itself.
        fn fake_bundle(root: &std::path::Path, folder: &str, exe: &str) -> std::path::PathBuf {
            let bundle = root.join(folder);
            std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
            std::fs::write(
                bundle.join("Contents/Info.plist"),
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                     <plist version=\"1.0\"><dict>\
                     <key>CFBundleIdentifier</key><string>test.notchling.fake</string>\
                     <key>CFBundleExecutable</key><string>{exe}</string>\
                     </dict></plist>"
                ),
            )
            .unwrap();
            std::fs::write(bundle.join("Contents/MacOS").join(exe), b"").unwrap();
            bundle
        }

        #[test]
        fn a_bundles_executable_is_the_file_its_info_plist_names() {
            let dir = TempDir::new();
            let bundle = fake_bundle(dir.path(), "My Fake App.app", "Fake Runner");
            let exe = bundle_executable(bundle.to_str().unwrap()).unwrap();
            assert!(
                exe.ends_with("/My Fake App.app/Contents/MacOS/Fake Runner"),
                "{exe}"
            );
        }

        #[test]
        fn a_folder_that_is_not_a_bundle_has_no_executable() {
            let dir = TempDir::new();
            assert_eq!(bundle_executable(dir.path().to_str().unwrap()), None);
            let missing = dir.path().join("Gone.app");
            assert_eq!(bundle_executable(missing.to_str().unwrap()), None);
        }

        #[test]
        fn the_process_table_lists_this_process_by_the_file_it_runs() {
            let table = process_paths().expect("the process table can be read");
            let me = std::env::current_exe().unwrap().canonicalize().unwrap();
            let exe = me.to_str().unwrap().to_string();
            let pids = app_pids(&table, std::slice::from_ref(&exe));
            assert!(
                pids.contains(&(std::process::id() as i32)),
                "{pids:?} in {exe}"
            );
        }
    }
}
