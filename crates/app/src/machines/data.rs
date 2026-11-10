//! Where `notchling-machines` reads and writes, and the engine's parts for a run or a render.
//!
//! The data folder is Chat's (`DataRoot`: `--data-root`, `NOTCHLING_DATA_ROOT`, else the real
//! one). In it the Swift app's `machines.json` is only read; this tool's own copy of it and its
//! pending update offers are documents in `<data>/notchling.db` (the files they used to be, under
//! `<data>/daemon/machines/`, move in once), and its audit log, merge worktrees, scratch and cache
//! stay files there (`MachineRoots`). A render opens nothing for writing: it neither imports,
//! migrates nor saves, so a checked-in fixture folder gets no database. Every process goes
//! through the engine's runner:
//! `ProcessRunner` only with `--live`, else `Refusing`, which answers each call with the off
//! message and starts nothing. The vault's values are read only with `--vault`; the list of names
//! (`<data>/daemon/vault.json`, no values) is read for the banners either way. A fixture root
//! turns both switches off whatever the flags say.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use notchling_engine::machines::actions::MorningActions;
use notchling_engine::machines::catalog;
use notchling_engine::machines::engine::{Deps, SetupEngine};
use notchling_engine::machines::github::{GitHubHttp, LiveGitHub, NoGitHub};
use notchling_engine::machines::mac_facts;
use notchling_engine::machines::merge::MergePlan;
use notchling_engine::machines::poller::MachinePoller;
use notchling_engine::machines::process::ProcessRunner;
use notchling_engine::machines::registry::RegistryFiles;
use notchling_engine::machines::roots::{MachineRoots, VaultSecrets};
use notchling_engine::machines::runner::{Call, Refusing, Runner, OFF};
use notchling_engine::machines::step::{NoSecrets, Opener, SecretSource};
use notchling_engine::vault::entry::VaultEntry;
use notchling_engine::vault::store::{system_keychain, Unsupported, VaultStore};
use notchling_engine::vault::url::renew_url;
use notchling_engine::vault::VaultRequest;

use super::model::{Parts, VaultList, VaultStamp};
use crate::chat::data::DataRoot;

/// What the tool runs over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    /// Notchling's data folder (the Swift app's `machines.json` is in it).
    pub data_dir: PathBuf,
    /// The user's home (the plugin versions to put on a machine are read under it).
    pub home: PathBuf,
    /// ssh, `gh` and `open` may run.
    pub live: bool,
    /// The Secret Vault's values may be read.
    pub vault: bool,
    /// A fixture root: never a process, never the vault.
    pub fixture: bool,
    /// A render: it imports, migrates and saves nothing (a fixture folder is checked in, and the
    /// database would land in it).
    pub read_only: bool,
}

/// Seconds since 1970.
pub fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl Setup {
    /// For the window: the `--data-root` folder, else the environment's, else the real folders
    /// (`None` without a home). A fixture root never runs a process or opens the vault.
    pub fn for_run(data_root: Option<&Path>, live: bool, vault: bool) -> Option<Setup> {
        Some(Setup::of(DataRoot::for_run(data_root)?, live, vault))
    }

    /// For a render: the `--data-root` folder, else the environment's, else the fixture this
    /// crate carries (`fixtures/machines`). Never the real home, never a process.
    pub fn for_render(data_root: Option<&Path>) -> Setup {
        let root = match data_root {
            Some(dir) => DataRoot::fixture(dir),
            None => match std::env::var_os(crate::notch::source::DATA_ROOT_ENV) {
                Some(v) if !v.is_empty() => DataRoot::fixture(Path::new(&v)),
                _ => DataRoot::fixture(
                    &Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/machines"),
                ),
            },
        };
        Setup {
            read_only: true,
            ..Setup::of(root, false, false)
        }
    }

    fn of(root: DataRoot, live: bool, vault: bool) -> Setup {
        let fixture = root.fixture.is_some();
        Setup {
            data_dir: root.data_dir,
            home: root.home,
            live: live && !fixture,
            vault: vault && !fixture,
            fixture,
            read_only: false,
        }
    }

    pub fn roots(&self) -> MachineRoots {
        MachineRoots::in_data(&self.data_dir)
    }

    /// The daemon's own vault list (`<data>/daemon/vault.json`), never the Swift app's.
    pub fn vault_index(&self) -> PathBuf {
        self.data_dir.join("daemon").join("vault.json")
    }

    /// The engine, the poller, the morning actions, the registry and the vault list.
    pub fn parts(&self) -> Parts {
        let roots = self.roots();
        let registry = Arc::new(RegistryFiles {
            read_only: self.read_only,
            ..roots.registry()
        });
        let runner: Arc<dyn Runner> = if self.live {
            Arc::new(ProcessRunner)
        } else {
            Arc::new(Refusing(OFF))
        };
        let secrets: Arc<dyn SecretSource> = if self.vault {
            Arc::new(VaultSecrets::new(VaultStore::new(
                self.vault_index(),
                system_keychain(),
            )))
        } else {
            Arc::new(NoSecrets)
        };
        let http: Arc<dyn GitHubHttp> = if self.live {
            Arc::new(LiveGitHub)
        } else {
            Arc::new(NoGitHub)
        };
        let opener = Arc::new(UrlOpener {
            runner: runner.clone(),
        });
        let home = self.home.clone();
        let deps = Deps {
            runner: runner.clone(),
            secrets: secrets.clone(),
            http,
            opener,
            store: registry.clone(),
            mac: Arc::new(move || mac_facts::load(&home)),
            clock: Arc::new(now_secs),
            scratch: roots.scratch(),
            offers_data: (!self.read_only).then(|| self.data_dir.clone()),
            audit: Some(roots.audit_file()),
        };
        let engine = Arc::new(SetupEngine::new(deps, catalog::standard()));
        let for_poller = registry.clone();
        let poller = Arc::new(MachinePoller::new(
            runner.clone(),
            move || for_poller.all(),
            now_secs,
        ));
        let for_actions = registry.clone();
        let actions = Arc::new(
            MorningActions::new(
                runner.clone(),
                move || for_actions.all(),
                roots.own_dir.join("cache"),
            )
            .with_plan(MergePlan::from_process(&roots.merge_dir())),
        );
        let vault = Arc::new(VaultFiles {
            index: self.vault_index(),
            secrets,
            runner: runner.clone(),
        });
        Parts {
            engine,
            poller,
            actions,
            registry,
            vault,
            runner,
            clock: Arc::new(now_secs),
            off: (!self.live).then_some(OFF),
        }
    }
}

/// Opens a page in the browser through the runner, so an off tool opens nothing. The vault form
/// is the daemon's: a Fix that asks for it says so in its message instead.
struct UrlOpener {
    runner: Arc<dyn Runner>,
}

impl Opener for UrlOpener {
    fn open_page(&self, url: &str) {
        open_url(&*self.runner, url);
    }
    fn open_form(&self, _request: &VaultRequest) {}
}

/// `/usr/bin/open <url>` on macOS; nowhere else yet.
fn open_url(runner: &dyn Runner, url: &str) {
    if !cfg!(target_os = "macos") || !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    let _ = runner.run(&Call::secs(
        vec!["/usr/bin/open".to_owned(), url.to_owned()],
        10,
    ));
}

/// The vault as the window lists it: the names from the index, renames and renewals through the
/// engine's secret source (which refuses while the vault is off).
struct VaultFiles {
    index: PathBuf,
    secrets: Arc<dyn SecretSource>,
    runner: Arc<dyn Runner>,
}

impl VaultList for VaultFiles {
    fn entries(&self) -> Vec<VaultEntry> {
        VaultStore::new(self.index.clone(), Box::new(Unsupported("list only")))
            .entries()
            .unwrap_or_default()
    }

    fn rename(&self, old: &str, new: &str) -> Result<(), String> {
        self.secrets.rename(old, new)
    }

    fn stamp(&self) -> Option<VaultStamp> {
        let meta = std::fs::metadata(&self.index).ok()?;
        Some((meta.modified().ok()?, meta.len()))
    }

    fn renew(&self, name: &str, url: Option<&str>) -> Result<(), String> {
        let page = renew_url(url).ok_or(format!("{name} has no page to renew it at"))?;
        let r = self
            .runner
            .run(&Call::secs(vec!["/usr/bin/open".to_owned(), page], 10));
        if r.code == 0 {
            Ok(())
        } else {
            Err(r
                .err
                .lines()
                .next()
                .unwrap_or("couldn't open the page")
                .to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::machines::runner::ScriptedRunner;

    #[test]
    fn a_fixture_root_is_never_live_and_reads_its_own_machines_file() {
        let s = Setup::for_run(Some(Path::new("/f")), true, true).unwrap();
        assert!(!s.live && !s.vault && s.fixture);
        assert_eq!(s.data_dir, PathBuf::from("/f/notchling"));
        assert_eq!(
            s.roots().registry().swift.as_deref(),
            Some(Path::new("/f/notchling/machines.json"))
        );
        assert_eq!(
            s.roots().registry().old_own,
            PathBuf::from("/f/notchling/daemon/machines/machines.json")
        );
        assert_eq!(s.roots().registry().data_dir, PathBuf::from("/f/notchling"));
        assert!(!s.read_only, "a run saves");
        assert_eq!(
            s.vault_index(),
            PathBuf::from("/f/notchling/daemon/vault.json")
        );
        let render = Setup::for_render(None);
        assert!(render.fixture && !render.live && render.read_only);
        assert!(render.data_dir.ends_with("fixtures/machines/notchling"));
    }

    #[test]
    fn the_fixture_parts_list_the_machines_and_refuse_every_process() {
        let parts = Setup::for_render(None).parts();
        let names: Vec<String> = parts.registry.all().into_iter().map(|m| m.name).collect();
        assert_eq!(names, ["m1", "vps-sing-1"]);
        assert_eq!(parts.off, Some(OFF));
        let r = parts
            .runner
            .run(&Call::secs(vec!["/usr/bin/ssh".into()], 1));
        assert_eq!(r.code, notchling_engine::machines::runner::REFUSED);
        assert!(parts.vault.entries().is_empty());
        assert!(parts.vault.rename("a", "b").is_err());
    }

    #[test]
    fn a_merge_makes_its_own_worktree_under_the_data_root() {
        use notchling_engine::machines::merge::MergeTarget;
        use notchling_engine::machines::registry::MachineRepo;
        let parts = Setup::for_render(Some(Path::new("/f"))).parts();
        let repo = MachineRepo {
            github: "acme/app".into(),
            local_path: "/Users/me/code/app".into(),
            check_command: "just check".into(),
            machine_path: None,
            verify_skill: None,
        };
        assert_eq!(
            parts.actions.plan().target(&repo, ""),
            MergeTarget::Own(
                PathBuf::from("/f/notchling/daemon/machines/merge/acme-app")
                    .to_string_lossy()
                    .into_owned()
            )
        );
    }

    #[test]
    fn renew_opens_only_a_web_page_and_says_when_there_is_none() {
        let runner = Arc::new(ScriptedRunner::new());
        let files = VaultFiles {
            index: PathBuf::from("/nonexistent/vault.json"),
            secrets: Arc::new(NoSecrets),
            runner: runner.clone(),
        };
        assert!(files.renew("x", None).is_err());
        assert!(files.renew("gh", Some("https://api.github.com")).is_ok());
        let calls = runner.calls();
        assert_eq!(
            calls[0].argv,
            [
                "/usr/bin/open",
                "https://github.com/settings/personal-access-tokens"
            ]
        );
        open_url(&*runner, "file:///etc/passwd");
        assert_eq!(runner.calls().len(), 1, "only http(s) pages open");
    }
}
