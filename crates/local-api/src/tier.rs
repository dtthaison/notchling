use crate::peer::PeerInfo;
use crate::wire::Tier;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub const APP_TIER_PATHS_ENV: &str = "NOTCHLING_APP_TIER_PATHS";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Allowlist {
    paths: BTreeSet<PathBuf>,
}

impl Allowlist {
    pub fn empty() -> Allowlist {
        Allowlist::default()
    }

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

    pub fn tier_of(&self, exe: Option<&Path>) -> Tier {
        let Some(exe) = exe else {
            return Tier::Agent;
        };
        match std::fs::canonicalize(exe) {
            Ok(real) if self.paths.contains(&real) => Tier::App,
            _ => Tier::Agent,
        }
    }

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

pub fn installed_app_paths() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        [
            "/Applications/Notchling.app/Contents/MacOS/notchling-app",
            "/Applications/Notchling Next.app/Contents/MacOS/notchling-app",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
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
        assert_eq!(
            list.tier_for(&at_accept, Some(&peer(11, &app, 1))),
            Tier::Agent
        );
        assert_eq!(
            list.tier_for(&at_accept, Some(&peer(10, &app, 2))),
            Tier::Agent
        );
        let other = exe(&dir, "other");
        assert_eq!(
            list.tier_for(&peer(10, &other, 1), Some(&peer(10, &app, 1))),
            Tier::Agent
        );
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
        assert_eq!(Allowlist::new([&app]).tier_of(Some(&link)), Tier::App);
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

    #[test]
    #[cfg(target_os = "macos")]
    fn the_side_by_side_bundle_is_the_app_too() {
        let paths = installed_app_paths();
        for want in [
            "/Applications/Notchling.app/Contents/MacOS/notchling-app",
            "/Applications/Notchling Next.app/Contents/MacOS/notchling-app",
        ] {
            assert!(paths.contains(&PathBuf::from(want)), "{want}");
        }
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
        let list = Allowlist::for_daemon(&[], Some(&joined), None);
        assert_eq!(list.tier_of(Some(&evil)), Tier::Agent);
        let app = exe(&dir, "app");
        let release = dir.path().join("usr/local/bin/notchling");
        let list = Allowlist::for_daemon(std::slice::from_ref(&app), Some(&joined), Some(&release));
        assert_eq!(list.tier_of(Some(&app)), Tier::App);
    }
}
