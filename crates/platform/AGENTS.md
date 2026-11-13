# platform: notes for agents

## Why the code is this way

The code carries no comments (see [AGENTS.md](../../AGENTS.md)). What a change could break without knowing it, by file:

- `crates/platform/src/app_procs.rs::app_pids`: The daemon has no main run loop, so the system's running-app list can be stale; match apps from the process table by exact executable path, ASCII case-insensitive, and not a helper app inside the bundle or `Codex.app.bak`.
- `crates/platform/src/app_procs.rs::translocation`: A Gatekeeper-translocated copy (`.../AppTranslocation/<uuid>/d/<App>.app/...`) counts when the path after `/d/` is the end of an executable, whole components only; the `/System/Volumes/Data` firmlink prefix is stripped before comparing.
- `crates/platform/src/apps.rs::is_running`: When the process table can't be read the app counts as running, so a caller waits or refuses and never writes a login; quitting still uses the polite `NSRunningApplication::terminate`.
- `crates/platform/src/awake.rs::PowerEvents`: Never hold up sleep: answer yes to the can-sleep question and to the will-sleep notice; WillPowerOn comes before HasPoweredOn and the system isn't up yet, so only HasPoweredOn is the wake.
- `crates/platform/src/files.rs::ensure_private_dir`: A symlinked folder is left as it is, since the folder it points at (a lane's shared one) is not ours to chmod.
- `crates/platform/src/notify.rs::MacNotifier`: Never use `osascript` (banners carry Script Editor's name); a process without an app bundle gets `Error::Unsupported` because `UNUserNotificationCenter` throws there, and the app then shows its own banner.
- `crates/platform/src/watch.rs::Config`: `max_latency` must stay above `debounce`, or the debounce never gets to wait; max latency is what lets a path that is written on every poll (a log, a busy session) still produce batches.
- `crates/platform/src/watch/fsevents.rs::Roots`: FSEvents spells paths with symlinks resolved (`/var` as `/private/var`) and two given folders may share one real folder (a lane's linked `projects`), so map each event back to every given folder that holds it.
