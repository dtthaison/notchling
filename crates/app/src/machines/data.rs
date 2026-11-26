use std::path::{Path, PathBuf};
use std::sync::Arc;

use notchling_engine::machines::actions::MorningActions;
use notchling_engine::machines::admin_secret::{AdminSecrets, FromSource, NoAdminSecrets};
use notchling_engine::machines::catalog;
use notchling_engine::machines::engine::{Deps, SetupEngine};
use notchling_engine::machines::github::{GitHubHttp, LiveGitHub, NoGitHub};
use notchling_engine::machines::mac_facts;
use notchling_engine::machines::merge::MergePlan;
use notchling_engine::machines::poller::MachinePoller;
use notchling_engine::machines::process::ProcessRunner;
use notchling_engine::machines::reach::Router;
use notchling_engine::machines::registry::RegistryFiles;
use notchling_engine::machines::release_download::CurlDownloads;
use notchling_engine::machines::roots::{MachineRoots, VaultSecrets};
use notchling_engine::machines::runner::{Call, Refusing, Runner, OFF};
use notchling_engine::machines::step::{ClusterTokens, NoCluster, NoSecrets, Opener, SecretSource};
use notchling_engine::machines::steps_node::dist_dir;
use notchling_engine::machines::tailscale::{default_cli, Tailscale};
use notchling_engine::vault::entry::VaultEntry;
use notchling_engine::vault::list::VaultIndex;
use notchling_engine::vault::store::{system_keychain, Unsupported, VaultStore};
use notchling_engine::vault::url::renew_url;
use notchling_engine::vault::VaultRequest;
use notchling_local_api::{ops, Client, ClientKind, ConnectOptions, SOCKET_NAME};

use super::admin_feed::{failure_text, DaemonAdminSecrets};
use super::cluster_feed::{DaemonTokens, NOT_RUNNING};
use super::model::{Parts, VaultList, VaultStamp};
use super::versions_feed::{fixed_mac, DaemonVersions, FixedVersions, MacVersions};
use crate::chat::data::DataRoot;
use crate::switches::Switches;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    pub data_dir: PathBuf,
    pub home: PathBuf,
    pub live: bool,
    pub vault: bool,
    pub fixture: bool,
    pub read_only: bool,
}

pub fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl Setup {
    pub fn for_run(data_root: Option<&Path>, live: bool, vault: bool) -> Option<Setup> {
        Some(Setup::of(DataRoot::for_run(data_root)?, live, vault))
    }

    pub fn for_root(root: DataRoot, live: bool, vault: bool) -> Setup {
        Setup::of(root, live, vault)
    }

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

    pub fn vault_index(&self) -> VaultIndex {
        VaultIndex::in_data(&self.data_dir)
    }

    pub fn versions(&self) -> Arc<dyn MacVersions> {
        if self.fixture {
            Arc::new(FixedVersions(Ok(fixed_mac())))
        } else {
            Arc::new(DaemonVersions {
                socket: self.data_dir.join(SOCKET_NAME),
            })
        }
    }

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
        let router: Arc<Router> = if self.live {
            Arc::new(Router::new(
                runner.clone(),
                Tailscale::new(runner.clone(), &default_cli()),
                Arc::new(now_secs),
            ))
        } else {
            Arc::new(Router::off())
        };
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
        let cluster: Arc<dyn ClusterTokens> = if self.fixture {
            Arc::new(NoCluster)
        } else {
            Arc::new(DaemonTokens::new(self.data_dir.join(SOCKET_NAME)))
        };
        let mac_daemon = (!self.fixture)
            .then(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| Some(exe.parent()?.join("notchling")))
            })
            .flatten()
            .filter(|path| path.is_file());
        let admin_secrets: Arc<dyn AdminSecrets> = if self.fixture {
            Arc::new(NoAdminSecrets)
        } else if self.vault {
            Arc::new(FromSource(secrets.clone()))
        } else {
            Arc::new(DaemonAdminSecrets::new(self.data_dir.join(SOCKET_NAME)))
        };
        let engine = Arc::new(
            SetupEngine::new(deps, catalog::standard())
                .with_router(router.clone())
                .with_admin_secrets(admin_secrets)
                .with_cluster(cluster.clone(), Some(dist_dir(&self.data_dir)))
                .with_mac_daemon(mac_daemon)
                .with_downloads(Arc::new(CurlDownloads::new(runner.clone()))),
        );
        let for_poller = registry.clone();
        let poller = Arc::new(
            MachinePoller::new(runner.clone(), move || for_poller.all(), now_secs)
                .with_router(router.clone()),
        );
        let for_actions = registry.clone();
        let actions = Arc::new(
            MorningActions::new(
                runner.clone(),
                move || for_actions.all(),
                roots.own_dir.join("cache"),
            )
            .with_plan(MergePlan::from_process(&roots.merge_dir()))
            .with_router(router.clone()),
        );
        let vault = Arc::new(VaultFiles {
            socket: (!self.fixture && !self.vault).then(|| self.data_dir.join(SOCKET_NAME)),
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
            versions: self.versions(),
            cluster,
        }
    }
}

struct UrlOpener {
    runner: Arc<dyn Runner>,
}

impl Opener for UrlOpener {
    fn open_page(&self, url: &str) {
        open_url(&*self.runner, url);
    }
    fn open_form(&self, _request: &VaultRequest) {}
}

fn open_url(runner: &dyn Runner, url: &str) {
    if !cfg!(target_os = "macos") || !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    let _ = runner.run(&Call::secs(
        vec!["/usr/bin/open".to_owned(), url.to_owned()],
        10,
    ));
}

const SWITCH_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

pub fn vault_state(reply: &serde_json::Value) -> Result<(), String> {
    match crate::vault::pick::unavailable(&Switches::from_reply(reply)) {
        Some(why) => Err(why),
        None => Ok(()),
    }
}

fn daemon_vault_state(socket: &Path) -> Result<(), String> {
    let options = ConnectOptions {
        timeout: SWITCH_WAIT,
        ..ConnectOptions::default()
    };
    let mut client =
        Client::connect_with(socket, ClientKind::App, env!("CARGO_PKG_VERSION"), &options)
            .map_err(|_| NOT_RUNNING.to_owned())?;
    let reply = client
        .call(ops::SETTINGS_SWITCHES, serde_json::json!({}))
        .map_err(|e| failure_text(&e))?;
    vault_state(&reply)
}

struct VaultFiles {
    socket: Option<PathBuf>,
    index: VaultIndex,
    secrets: Arc<dyn SecretSource>,
    runner: Arc<dyn Runner>,
}

impl VaultList for VaultFiles {
    fn entries(&self) -> Vec<VaultEntry> {
        VaultStore::new(self.index.clone(), Box::new(Unsupported("list only")))
            .entries()
            .unwrap_or_default()
    }

    fn listing(&self) -> Result<Vec<VaultEntry>, String> {
        if let Some(socket) = &self.socket {
            daemon_vault_state(socket)?;
        }
        VaultStore::new(self.index.clone(), Box::new(Unsupported("list only")))
            .entries()
            .map_err(|e| e.to_string())
    }

    fn rename(&self, old: &str, new: &str) -> Result<(), String> {
        self.secrets.rename(old, new)
    }

    fn stamp(&self) -> Option<VaultStamp> {
        self.index.stamp()
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
            VaultIndex::new(
                Path::new("/f/notchling"),
                PathBuf::from("/f/notchling/daemon/vault.json")
            )
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

    #[cfg(unix)]
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
            socket: None,
            index: VaultIndex::in_data(Path::new("/nonexistent")),
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
    #[test]
    fn the_vault_picker_says_off_and_unreadable_instead_of_empty() {
        use serde_json::json;
        assert_eq!(vault_state(&json!({"vault": true})), Ok(()));
        assert_eq!(
            vault_state(&json!({"vault": false})),
            Err(notchling_locale::cluster::vault_off().to_owned())
        );
        let dir = std::env::temp_dir().join(format!("notchling-pick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("daemon")).unwrap();
        std::fs::write(dir.join("daemon").join("vault.json"), "{not json").unwrap();
        let broken = VaultFiles {
            socket: None,
            index: VaultIndex::in_data(&dir),
            secrets: Arc::new(NoSecrets),
            runner: Arc::new(ScriptedRunner::new()),
        };
        assert!(broken.listing().is_err());
        let nobody = VaultFiles {
            socket: Some(dir.join("no-daemon.sock")),
            ..broken
        };
        assert_eq!(nobody.listing().unwrap_err(), NOT_RUNNING);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
