use crate::snap_files::stage_set;
use crate::snap_files_tests::{meta, node, put};
use crate::snap_repo_tests::{launcher, read_in, unsandboxed, Rig};
use crate::snap_restore::{restore, run_step, DbHooks, Report, Request};
use notchling_engine::cluster::restore::{plan_restore, PathState, RestoreCtx, RestoreStep};
use notchling_engine::cluster::snapshot::{movable, staged_path, Manifest, SessionEntry};
use notchling_engine::cluster::snapshot::{SetPlan, Trigger};
use notchling_engine::cluster::standby_store::StandbyStore;
use notchling_engine::cluster::Tool;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(crate) const S1: &str = "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa";
pub(crate) const CODEX: &str = "bbbbbbbb-1111-4111-8111-bbbbbbbbbbbb";

pub(crate) struct World {
    pub(crate) rig: Rig,
    pub(crate) home: PathBuf,
    pub(crate) cluster: PathBuf,
    pub(crate) repo: PathBuf,
}

pub(crate) fn ok_hooks() -> DbHooks<'static> {
    DbHooks {
        close: &|| Ok(()),
        reopen: &|| Ok(()),
    }
}

impl World {
    pub(crate) fn new() -> World {
        let rig = Rig::pushed();
        let home = std::fs::canonicalize(rig.t.root()).unwrap();
        let cluster = home.join("data/cluster");
        let repo = rig.root();
        assert_eq!(repo, home.join("work"));
        let origin = std::fs::canonicalize(&rig.origin).unwrap();
        rig.w(&["remote", "set-url", "origin", &origin.to_string_lossy()]);
        World {
            rig,
            home,
            cluster,
            repo,
        }
    }

    pub(crate) fn at(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }

    pub(crate) fn entry(
        &self,
        id: &str,
        repo: &Path,
        transcripts: &[&Path],
        files: &[&Path],
    ) -> SessionEntry {
        let facts = read_in(&self.rig.t, repo).expect("a repository");
        let cwd = repo.to_string_lossy().into_owned();
        SessionEntry {
            id: id.to_owned(),
            tool: Tool::Claude,
            cwd: cwd.clone(),
            movable: movable(Some(&facts), &cwd),
            repo: Some(facts),
            transcripts: transcripts.iter().map(|p| p.to_path_buf()).collect(),
            files: files.iter().map(|p| p.to_path_buf()).collect(),
        }
    }

    pub(crate) fn transcript(&self, id: &str, bytes: &[u8]) -> PathBuf {
        let path = self.at(&format!(".claude/projects/p/{id}.jsonl"));
        put(&path, bytes);
        path
    }

    pub(crate) fn stage(
        &self,
        sessions: Vec<SessionEntry>,
        files: &[&Path],
        dbs: &[&Path],
    ) -> Manifest {
        let tails = sessions
            .iter()
            .flat_map(|s| s.transcripts.iter().map(|p| (p.clone(), 0)))
            .collect();
        let mut sidecars: Vec<PathBuf> = files.iter().map(|p| p.to_path_buf()).collect();
        for entry in &sessions {
            sidecars.extend(entry.files.iter().cloned());
        }
        let plan = SetPlan {
            id: "s-1".to_owned(),
            trigger: Trigger::Timer,
            sessions,
            tails,
            files: sidecars,
            dbs: dbs.iter().map(|p| p.to_path_buf()).collect(),
        };
        let out = self.at("stage-out");
        let manifest = stage_set(&plan, &out, &meta()).expect("stage");
        let store = StandbyStore::new(&self.cluster, &node());
        store.begin(&manifest).unwrap();
        for part in &manifest.parts {
            let from = staged_path(&out.join(&manifest.set), &part.path).unwrap();
            store.part(&manifest.set, part, &from).unwrap();
        }
        store.commit(&manifest.set).unwrap();
        manifest
    }

    pub(crate) fn run(&self, ids: &[&str], aside: bool, shared: bool) -> Result<Report, String> {
        self.run_with(ids, aside, shared, &ok_hooks())
    }

    pub(crate) fn run_with(
        &self,
        ids: &[&str],
        aside: bool,
        shared: bool,
        db: &DbHooks,
    ) -> Result<Report, String> {
        let sessions: BTreeSet<String> = ids.iter().map(|s| (*s).to_owned()).collect();
        let read = launcher(&self.rig.t);
        let git = unsandboxed(&self.rig.t);
        restore(&Request {
            cluster: &self.cluster,
            primary: &node(),
            home: &self.home,
            node: "m1",
            sessions: &sessions,
            aside,
            shared,
            git: &git,
            read: &read,
            db,
        })
    }

    pub(crate) fn step(&self, step: &RestoreStep) -> Result<(), String> {
        let sessions = BTreeSet::new();
        let read = launcher(&self.rig.t);
        let git = unsandboxed(&self.rig.t);
        run_step(
            step,
            &Request {
                cluster: &self.cluster,
                primary: &node(),
                home: &self.home,
                node: "m1",
                sessions: &sessions,
                aside: false,
                shared: false,
                git: &git,
                read: &read,
                db: &ok_hooks(),
            },
        )
    }

    pub(crate) fn plan(
        &self,
        ids: &[&str],
        aside: bool,
        shared: bool,
    ) -> Result<Vec<RestoreStep>, String> {
        let manifest = StandbyStore::new(&self.cluster, &node()).current().unwrap();
        let sessions: BTreeSet<String> = ids.iter().map(|s| (*s).to_owned()).collect();
        let ctx = RestoreCtx {
            cluster: &self.cluster,
            home: &self.home,
            node: "m1",
            aside,
            shared,
            path_dirs: &[],
        };
        let git = launcher(&self.rig.t);
        plan_restore(&manifest, &ctx, &sessions, &|p: &Path| {
            crate::snap_restore::probe_state(&git, &self.home, p)
        })
    }

    pub(crate) fn state(&self, path: &Path) -> PathState {
        crate::snap_restore::probe_state(&launcher(&self.rig.t), &self.home, path)
    }
}
