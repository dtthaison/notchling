use crate::cluster_redact::TailMasks;
use crate::grant_roots::{GrantMarks, JobRoots};
use crate::shell_jobs::ShellGrants;
use notchling_engine::cluster::grant_files::GrantStore;
use std::path::Path;
use std::sync::Arc;

pub type Parts = (Arc<TailMasks>, Arc<GrantMarks>, Arc<JobRoots>);

pub struct GrantParts {
    pub store: Arc<GrantStore>,
    pub masks: Arc<TailMasks>,
    pub marks: Arc<GrantMarks>,
    pub roots: Arc<JobRoots>,
}

impl GrantParts {
    pub fn new(cluster_dir: &Path) -> GrantParts {
        let masks = Arc::new(TailMasks::new());
        let marks = Arc::new(GrantMarks::new(cluster_dir));
        for job in marks.list() {
            masks.mark_granted(&job);
        }
        GrantParts {
            store: Arc::new(GrantStore::new(cluster_dir)),
            masks,
            marks,
            roots: Arc::new(JobRoots::new()),
        }
    }

    pub fn parts(&self) -> Parts {
        (self.masks.clone(), self.marks.clone(), self.roots.clone())
    }

    pub fn shell(&self) -> ShellGrants {
        let (store, roots) = (self.store.clone(), self.roots.clone());
        let masks = self.masks.clone();
        ShellGrants {
            masks: self.masks.clone(),
            roots: self.roots.clone(),
            reaped: Arc::new(move |job: &str| {
                let _gate = roots.gate();
                roots.end(job);
                let _ = store.remove_job(job);
            }),
            ended: Arc::new(move |job: &str| masks.end_job(job)),
        }
    }
}
