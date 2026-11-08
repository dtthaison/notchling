//! Which tier a peer gets, from its executable path (decision 0007).
//!
//! The app tier needs an executable path in the allowlist: the installed app bundle's binary, or for
//! dev a path in `NOTCHLING_APP_TIER_PATHS`, read once at daemon start and only by a daemon that
//! runs from a cargo target folder (an installed one ignores it, and `ensure_daemon` never passes
//! it on). Everything else, a copy of the app elsewhere included, is the agent tier.
//!
//! Both sides are compared canonical (symlinks resolved). The kernel already reports the file a
//! process really runs (`proc_pidpath` on macOS, `/proc/<pid>/exe` on Linux), so a peer started
//! through a symlink to the app is the app's own binary and counts as it. The allowlist is
//! canonicalized once, at load: a symlink in it names the file it pointed at then, and retargeting
//! the link later grants nothing. A path that doesn't exist at load is dropped.

use crate::peer::PeerInfo;
use crate::wire::Tier;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The environment variable a dev daemon reads its extra app-tier paths from (OS path-list syntax).
pub const APP_TIER_PATHS_ENV: &str = "NOTCHLING_APP_TIER_PATHS";

/// The executables that get the app tier.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Allowlist {
    paths: BTreeSet<PathBuf>,
}

impl Allowlist {
    /// Nobody gets the app tier.
    pub fn empty() -> Allowlist {
        Allowlist::default()
    }

    /// The canonical form of each path that exists.
    pub fn new<I, P>(paths: I) -> Allowlist
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let paths = paths
            .into_iter()
            .filter_map(|p| std::fs::canonicalize(p.as_ref()).ok())
            .collect();
        Allowlist { paths }
    }

    /// The daemon's list: the installed app's binaries, plus `NOTCHLING_APP_TIER_PATHS`'s (read
    /// now) only when the daemon itself, `own_exe`, runs from a cargo target folder: a dev build.
    /// An installed daemon ignores the variable, so whoever starts it can't name its own tier.
    pub fn for_daemon(
        installed: &[PathBuf],
        env_value: Option<&OsStr>,
        own_exe: Option<&Path>,
    ) -> Allowlist {
        let mut all: Vec<PathBuf> = installed.to_vec();
        let dev_build = own_exe.is_some_and(crate::spawn::in_cargo_target);
        if let Some(value) = env_value.filter(|_| dev_build) {
            all.extend(std::env::split_paths(value).filter(|p| p.is_absolute()));
        }
        Allowlist::new(all)
    }

    /// The tier for a peer whose executable is `exe`; an unknown path is the agent tier.
    pub fn tier_of(&self, exe: Option<&Path>) -> Tier {
        let Some(exe) = exe else {
            return Tier::Agent;
        };
        match std::fs::canonicalize(exe) {
            Ok(real) if self.paths.contains(&real) => Tier::App,
            _ => Tier::Agent,
        }
    }

    /// The tier for a connection whose peer was `at_accept` when it connected and `at_hello` when
    /// it said hello: the app tier only when both are the same process still running the same
    /// listed file (see the exec race in [`crate::peer`]).
    pub fn tier_for(&self, at_accept: &PeerInfo, at_hello: Option<&PeerInfo>) -> Tier {
        match at_hello {
            Some(now) if at_accept.same_process(now) => self.tier_of(now.exe.as_deref()),
            _ => Tier::Agent,
        }
    }

    pub fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.paths.iter()
    }
}

/// Where the installed desktop app's binary sits, per OS. Only these and the dev variable count.
pub fn installed_app_paths() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        vec![PathBuf::from(
            "/Applications/Notchling.app/Contents/MacOS/notchling-app",
        )]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn exe(dir: &TempDir, name: &str) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, b"binary").unwrap();
        path
    }

    #[test]
    fn only_an_allowlisted_path_is_the_app_tier() {
        let dir = TempDir::new("tier");
        let app = exe(&dir, "app");
        let other = exe(&dir, "other");
        let list = Allowlist::new([&app]);
        assert_eq!(list.tier_of(Some(&app)), Tier::App);
        assert_eq!(list.tier_of(Some(&other)), Tier::Agent);
        assert_eq!(list.tier_of(None), Tier::Agent);
        assert_eq!(Allowlist::empty().tier_of(Some(&app)), Tier::Agent);
    }

    #[test]
    fn the_app_tier_needs_the_same_process_at_accept_and_at_hello() {
        let dir = TempDir::new("tier-twice");
        let app = exe(&dir, "app");
        let list = Allowlist::new([&app]);
        let peer = |pid: u32, exe: &Path, generation: u64| PeerInfo {
            uid: 501,
            pid,
            exe: Some(exe.to_path_buf()),
            generation: Some(generation),
        };
        let at_accept = peer(10, &app, 1);
        assert_eq!(list.tier_for(&at_accept, Some(&at_accept)), Tier::App);
        // The hello came from another process (the fork), or the same pid after an exec.
        assert_eq!(
            list.tier_for(&at_accept, Some(&peer(11, &app, 1))),
            Tier::Agent
        );
        assert_eq!(
            list.tier_for(&at_accept, Some(&peer(10, &app, 2))),
            Tier::Agent
        );
        // The process exec'd into the app after accept: the files differ.
        let other = exe(&dir, "other");
        assert_eq!(
            list.tier_for(&peer(10, &other, 1), Some(&peer(10, &app, 1))),
            Tier::Agent
        );
        // No second read at all.
        assert_eq!(list.tier_for(&at_accept, None), Tier::Agent);
    }

    #[test]
    fn a_copy_of_the_app_elsewhere_is_the_agent_tier() {
        let dir = TempDir::new("tier-copy");
        let app = exe(&dir, "app");
        let copy = dir.path().join("copy");
        std::fs::copy(&app, &copy).unwrap();
        assert_eq!(Allowlist::new([&app]).tier_of(Some(&copy)), Tier::Agent);
    }

    #[test]
    fn a_missing_path_is_dropped_at_load() {
        let dir = TempDir::new("tier-missing");
        let later = dir.path().join("later");
        let list = Allowlist::new([&later]);
        assert_eq!(list.paths().count(), 0);
        std::fs::write(&later, b"x").unwrap();
        assert_eq!(list.tier_of(Some(&later)), Tier::Agent);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_to_the_app_counts_as_the_app() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new("tier-link");
        let app = exe(&dir, "app");
        let link = dir.path().join("link");
        symlink(&app, &link).unwrap();
        // The peer path through a link resolves to the app.
        assert_eq!(Allowlist::new([&app]).tier_of(Some(&link)), Tier::App);
        // A link in the list names its target at load.
        assert_eq!(Allowlist::new([&link]).tier_of(Some(&app)), Tier::App);
    }

    #[cfg(unix)]
    #[test]
    fn retargeting_an_allowlisted_link_grants_nothing() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new("tier-retarget");
        let app = exe(&dir, "app");
        let evil = exe(&dir, "evil");
        let link = dir.path().join("link");
        symlink(&app, &link).unwrap();
        let list = Allowlist::new([&link]);
        std::fs::remove_file(&link).unwrap();
        symlink(&evil, &link).unwrap();
        assert_eq!(list.tier_of(Some(&evil)), Tier::Agent);
        assert_eq!(list.tier_of(Some(&link)), Tier::Agent);
    }

    #[test]
    fn the_dev_variable_adds_absolute_paths_only() {
        let dir = TempDir::new("tier-env");
        let app = exe(&dir, "app");
        let joined = std::env::join_paths([app.clone(), PathBuf::from("relative/app")]).unwrap();
        let daemon = exe_at(&dir, "repo/target/debug/notchling");
        let list = Allowlist::for_daemon(&[], Some(&joined), Some(&daemon));
        assert_eq!(list.paths().count(), 1);
        assert_eq!(list.tier_of(Some(&app)), Tier::App);
        assert_eq!(
            Allowlist::for_daemon(&[], None, Some(&daemon))
                .paths()
                .count(),
            0
        );
    }

    fn exe_at(dir: &TempDir, rel: &str) -> PathBuf {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"binary").unwrap();
        path
    }

    #[test]
    fn an_installed_daemon_ignores_the_dev_variable() {
        let dir = TempDir::new("tier-release");
        let evil = exe(&dir, "evil");
        let joined = std::env::join_paths([evil.clone()]).unwrap();
        for release in [
            exe_at(&dir, "Notchling.app/Contents/MacOS/notchling"),
            exe_at(&dir, "usr/local/bin/notchling"),
        ] {
            let list = Allowlist::for_daemon(&[], Some(&joined), Some(&release));
            assert_eq!(list.tier_of(Some(&evil)), Tier::Agent, "{release:?}");
            assert_eq!(list.paths().count(), 0);
        }
        // A daemon that can't name its own file is not a dev build either.
        let list = Allowlist::for_daemon(&[], Some(&joined), None);
        assert_eq!(list.tier_of(Some(&evil)), Tier::Agent);
        // The installed app's own path is listed whatever the variable says.
        let app = exe(&dir, "app");
        let release = dir.path().join("usr/local/bin/notchling");
        let list = Allowlist::for_daemon(std::slice::from_ref(&app), Some(&joined), Some(&release));
        assert_eq!(list.tier_of(Some(&app)), Tier::App);
    }
}
