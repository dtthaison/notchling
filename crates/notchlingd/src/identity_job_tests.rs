use crate::identity::{Identifier, Launches};
use crate::late_emit::LateSalt;
use crate::vault_caller::Processes;
use notchling_engine::organizations::Organizations;
use notchling_engine::sessions::Roots;
use notchling_events::Salt;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

pub(crate) const SESSION: &str = "52afae32-0000-4000-8000-000000000001";
pub(crate) const JOB_SESSION: &str = "be50d36a-0000-4000-8000-000000000002";

pub(crate) struct Procs {
    pub(crate) claude: PathBuf,
    pub(crate) cwd: Option<PathBuf>,
}

impl Processes for Procs {
    fn parent(&self, pid: u32) -> Option<u32> {
        match pid {
            900 => Some(800),
            800 => Some(600),
            600 => Some(1),
            _ => None,
        }
    }
    fn name(&self, _: u32) -> Option<String> {
        None
    }
    fn cwd(&self, pid: u32) -> Option<PathBuf> {
        self.cwd.clone().filter(|_| pid == 800)
    }
    fn resolve(&self, path: &Path) -> Option<PathBuf> {
        Some(path.to_path_buf())
    }
    fn exe(&self, pid: u32) -> Option<PathBuf> {
        (pid == 800).then(|| self.claude.clone())
    }
}

pub(crate) struct Home(pub(crate) PathBuf);

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn salt() -> LateSalt {
    let cell = OnceLock::new();
    let _ = cell.set(Salt([7; 32]));
    Arc::new(cell)
}

pub(crate) struct Rig {
    pub(crate) home: Home,
    pub(crate) acme: String,
    pub(crate) beta: String,
    pub(crate) work: PathBuf,
    pub(crate) other: PathBuf,
}

pub(crate) fn rig(tag: &str, session_file: Value) -> Rig {
    let dir = std::env::temp_dir().join(format!("nl-ident-job-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".claude/sessions")).unwrap();
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(
        dir.join(".claude/sessions/800.json"),
        session_file.to_string(),
    )
    .unwrap();
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let book = Organizations::new(&dir.join("data")).with_home(&dir);
    let acme = book.create("Acme", None, None).unwrap();
    book.set_folders(&acme, &[work.to_string_lossy().into_owned()])
        .unwrap();
    let other = dir.join("other");
    std::fs::create_dir_all(&other).unwrap();
    let beta = book.create("Beta", None, None).unwrap();
    book.set_folders(&beta, &[other.to_string_lossy().into_owned()])
        .unwrap();
    Rig {
        home: Home(dir),
        acme,
        beta,
        work,
        other,
    }
}

pub(crate) fn identify(rig: &Rig, rows: Value) -> notchling_local_api::AgentIdentity {
    identify_in(rig, rows, None)
}

pub(crate) fn identify_in(
    rig: &Rig,
    rows: Value,
    cwd: Option<PathBuf>,
) -> notchling_local_api::AgentIdentity {
    let home = &rig.home.0;
    let claude = home.join(".local/share/claude/versions/2.1.0/claude");
    Identifier::new(
        Roots::new(home, &home.join("data")),
        Arc::new(Procs { claude, cwd }),
        Arc::new(Mutex::new(rows)),
        salt(),
        Launches::default(),
    )
    .with_own_exe(PathBuf::from("/Applications/Notchling.app/notchling"))
    .with_organizations(Organizations::new(&home.join("data")).with_home(home))
    .identify(900)
}

#[test]
fn a_session_the_list_holds_under_its_job_id_keeps_its_folder_and_organization() {
    let r = rig(
        "job",
        json!({"sessionId": SESSION, "cwd": "/elsewhere", "jobId": "j1"}),
    );
    let inside = r.work.join("app").to_string_lossy().into_owned();
    let who = identify(
        &r,
        json!([{
            "id": JOB_SESSION, "cwd": inside, "resumeSessionId": SESSION,
            "target": {"job": "j1"}, "group": "working", "label": "Working",
        }]),
    );
    assert!(who.verified);
    assert_eq!(who.session.as_deref(), Some(SESSION));
    assert_eq!(who.folder.as_deref(), Some(inside.as_str()));
    assert_eq!(who.organization.as_deref(), Some(r.acme.as_str()));
}

#[test]
fn a_session_the_list_lacks_takes_its_folder_from_its_own_session_file() {
    let r = rig("file", json!({"sessionId": SESSION, "cwd": "/placeholder"}));
    let inside = r.work.join("app").to_string_lossy().into_owned();
    std::fs::write(
        r.home.0.join(".claude/sessions/800.json"),
        json!({"sessionId": SESSION, "cwd": inside}).to_string(),
    )
    .unwrap();
    let who = identify(&r, json!([]));
    assert_eq!(who.folder.as_deref(), Some(inside.as_str()));
    assert_eq!(who.organization.as_deref(), Some(r.acme.as_str()));
}

#[test]
fn a_verified_session_with_no_known_folder_has_no_organization() {
    let r = rig("none", json!({"sessionId": SESSION}));
    let who = identify(&r, json!([]));
    assert!(who.verified);
    assert!(who.folder.is_none());
    assert!(who.organization.is_none());
}
