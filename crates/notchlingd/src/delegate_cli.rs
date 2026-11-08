//! `notchling delegate` and `notchling delegate-models`: the engine's delegate command
//! ([`notchling_engine::delegate`]) over this machine's folders, its signals and its streams.
//!
//! It needs no daemon and is not behind the `acting` switch (the plan's ruling): it runs a tool
//! only because the agent that called it asked.
//!
//! - A signal (Ctrl+C, a task stop's SIGTERM, a closed terminal's SIGHUP) stops the other tool and
//!   reports what it had.
//! - Under `NOTCHLING_DATA_ROOT` the tools' media folders and Codex's model list are read from the
//!   fixture root (`codex`, `gemini/antigravity-cli`), as the daemon does, and the tools are still
//!   found in the real home's usual places and on the real PATH.
//! - "Allow Network in Codex Turns" is read, never written, from `settings.json` in the data
//!   folder; on unless it says off. It only matters with `--write`.

use crate::layout::Layout;
use notchling_engine::delegate::command::{self, Context};
use notchling_engine::delegate::options::DEPTH_VARIABLE;
use notchling_engine::delegate::real::{self, Folders};
use notchling_engine::settings::Settings;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

static SIGNALLED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_signal(_: libc::c_int) {
    // Only an atomic store: async-signal-safe.
    SIGNALLED.store(true, Ordering::SeqCst);
}

/// Catches SIGINT, SIGTERM and SIGHUP, and sets `stop` shortly after one arrives. The watcher
/// thread lives until the process exits.
#[cfg(unix)]
fn stop_on_signals(stop: Arc<AtomicBool>) {
    let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
    unsafe {
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGHUP, handler);
    }
    std::thread::spawn(move || loop {
        if SIGNALLED.load(Ordering::SeqCst) {
            stop.store(true, Ordering::SeqCst);
        }
        std::thread::sleep(Duration::from_millis(50));
    });
}

#[cfg(not(unix))]
fn stop_on_signals(_stop: Arc<AtomicBool>) {
    let _ = &SIGNALLED;
}

/// The folders for this run. `real_home` is where the tools are installed; the layout says where
/// their data is read.
pub fn folders(
    layout: &Layout,
    real_home: &Path,
    path: String,
    codex_home: Option<&std::ffi::OsStr>,
) -> Folders {
    let network = Settings::open(&layout.data_dir.join("settings.json"))
        .ok()
        .and_then(|s| s.get_bool("codexNetwork"))
        .unwrap_or(true);
    Folders {
        home: real_home.to_path_buf(),
        codex_home: layout.codex_dir_with(codex_home),
        antigravity: layout.antigravity_dir(),
        path,
        network,
    }
}

fn setup() -> Option<(Layout, Folders)> {
    let Some(layout) = Layout::from_env(None) else {
        eprintln!(
            "notchling: no home folder in the environment, so nothing to find the tools from"
        );
        return None;
    };
    let real_home = notchling_platform::paths::home().unwrap_or_else(|| layout.home());
    let path = std::env::var("PATH").unwrap_or_default();
    let folders = folders(
        &layout,
        &real_home,
        path,
        std::env::var_os("CODEX_HOME").as_deref(),
    );
    Some((layout, folders))
}

/// `notchling delegate <args>`: the exit code.
pub fn run(args: &[String]) -> i32 {
    let Some((_, folders)) = setup() else {
        return 2;
    };
    let Ok(cwd) = std::env::current_dir() else {
        eprintln!("notchling: can't tell which folder this runs in");
        return 2;
    };
    let raw_depth = std::env::var(DEPTH_VARIABLE).ok();
    // The tools start their own processes (agy, and what it runs) with this environment: one
    // level deeper than this one.
    if let Ok(deeper) = notchling_engine::delegate::options::enter_depth(raw_depth.as_deref()) {
        std::env::set_var(DEPTH_VARIABLE, deeper.to_string());
    }
    let stop = Arc::new(AtomicBool::new(false));
    stop_on_signals(stop.clone());
    let deps = real::deps(&folders, stop);
    // `~` in a path is the user's own home, even over a fixture root.
    let home: PathBuf = folders.home.clone();
    let ctx = Context {
        cwd: &cwd,
        home: Some(&home),
        depth: raw_depth.as_deref(),
    };
    command::run(
        args,
        &ctx,
        deps,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

/// `notchling delegate-models [codex|agy]`.
pub fn models(args: &[String]) -> i32 {
    let Some((_, folders)) = setup() else {
        return 2;
    };
    let deps = real::deps(&folders, Arc::new(AtomicBool::new(false)));
    command::models_listing(args, &deps, &mut std::io::stdout())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn layout(fixture_root: Option<&str>) -> Layout {
        Layout::resolve(None, fixture_root.map(OsStr::new), || {
            Some((PathBuf::from("/home/me"), PathBuf::from("/data/Notchling")))
        })
        .unwrap()
    }

    #[test]
    fn the_real_folders_are_the_users_own() {
        let f = folders(&layout(None), Path::new("/home/me"), "/bin".into(), None);
        assert_eq!(f.home, Path::new("/home/me"));
        assert_eq!(f.codex_home, Path::new("/home/me/.codex"));
        assert_eq!(f.antigravity, Path::new("/home/me/.gemini/antigravity-cli"));
        assert!(f.network, "on unless the settings say off");
        let custom = folders(
            &layout(None),
            Path::new("/home/me"),
            String::new(),
            Some(OsStr::new("/elsewhere/codex")),
        );
        assert_eq!(custom.codex_home, Path::new("/elsewhere/codex"));
    }

    #[test]
    fn a_fixture_root_stands_in_for_the_data_but_not_for_the_tools() {
        let f = folders(
            &layout(Some("/tmp/nl-root")),
            Path::new("/home/me"),
            "/bin".into(),
            None,
        );
        assert_eq!(
            f.home,
            Path::new("/home/me"),
            "the tools are found in the real home"
        );
        assert_eq!(f.codex_home, Path::new("/tmp/nl-root/codex"));
        assert_eq!(
            f.antigravity,
            Path::new("/tmp/nl-root/gemini/antigravity-cli")
        );
    }

    #[test]
    fn the_network_follows_the_codex_network_setting() {
        let dir = std::env::temp_dir().join(format!("nl-delegate-cli-{}", std::process::id()));
        let data = dir.join("notchling");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(
            data.join("settings.json"),
            r#"{"version":1,"values":{"codexNetwork":false}}"#,
        )
        .unwrap();
        let root = dir.to_string_lossy().into_owned();
        let f = folders(&layout(Some(&root)), Path::new("/h"), String::new(), None);
        assert!(!f.network);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
