use super::job_tests::{rig, Rig};
use crate::identity::{Identifier, Launches};
use crate::late_emit::LateSalt;
use crate::vault_caller::Processes;
use notchling_engine::organizations::Organizations;
use notchling_engine::sessions::Roots;
use notchling_events::kinds::choices::AgentTool;
use notchling_events::Salt;
use notchling_local_api::AgentIdentity;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

struct Started {
    cwd: Option<PathBuf>,
}

impl Processes for Started {
    fn parent(&self, pid: u32) -> Option<u32> {
        match pid {
            900 => Some(700),
            700 => Some(1),
            _ => None,
        }
    }
    fn name(&self, _: u32) -> Option<String> {
        None
    }
    fn cwd(&self, pid: u32) -> Option<PathBuf> {
        self.cwd.clone().filter(|_| pid == 700)
    }
    fn resolve(&self, path: &Path) -> Option<PathBuf> {
        Some(path.to_path_buf())
    }
    fn start_time(&self, pid: u32) -> Option<u64> {
        (pid == 700).then_some(5_000)
    }
}

fn salt() -> LateSalt {
    let cell = OnceLock::new();
    let _ = cell.set(Salt([7; 32]));
    Arc::new(cell)
}

fn identify(r: &Rig, launches: Launches, rows: Value, cwd: Option<PathBuf>) -> AgentIdentity {
    let home = &r.home.0;
    Identifier::new(
        Roots::new(home, &home.join("data")),
        Arc::new(Started { cwd }),
        Arc::new(Mutex::new(rows)),
        salt(),
        launches,
    )
    .with_own_exe(PathBuf::from("/Applications/Notchling.app/notchling"))
    .with_organizations(Organizations::new(&home.join("data")).with_home(home))
    .identify(900)
}

#[test]
fn a_launched_session_takes_the_folder_it_was_launched_in() {
    let r = rig("launch-folder", json!({}));
    let in_acme = r.work.join("app").to_string_lossy().into_owned();
    let in_beta = r.other.join("app");
    let launches = Launches::default();
    launches.register(700, 5_000, AgentTool::Codex, "codex-sess", Some(&in_acme));
    let rows = json!([{"id": "codex-sess", "cwd": in_beta.to_string_lossy()}]);
    let who = identify(&r, launches, rows, Some(in_beta));
    assert!(who.verified);
    assert_eq!(who.session.as_deref(), Some("codex-sess"));
    assert_eq!(who.folder.as_deref(), Some(in_acme.as_str()));
    assert_eq!(who.organization.as_deref(), Some(r.acme.as_str()));
}

#[test]
fn a_launched_session_with_no_launch_folder_reads_its_live_folder_or_has_none() {
    let r = rig("launch-live", json!({}));
    let in_acme = r.work.join("app").to_string_lossy().into_owned();
    let in_beta = r.other.join("app");
    let launches = Launches::default();
    launches.register(700, 5_000, AgentTool::Codex, "codex-sess", None);
    let rows = json!([{"id": "codex-sess", "cwd": in_acme}]);
    let who = identify(&r, launches.clone(), rows.clone(), Some(in_beta.clone()));
    assert_eq!(
        who.folder.as_deref(),
        Some(in_beta.to_string_lossy().as_ref())
    );
    assert_eq!(who.organization.as_deref(), Some(r.beta.as_str()));
    let blind = identify(&r, launches, rows, None);
    assert!(blind.verified);
    assert!(blind.folder.is_none());
    assert!(blind.organization.is_none());
}
