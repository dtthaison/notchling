use notchling_engine::cluster::snapshot::repo_state::RepoFacts;
use notchling_engine::control::Launcher;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aside {
    Moved,
    AlreadyThere,
    Nothing,
}

fn present(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

pub fn aside(from: &Path, to: &Path) -> Result<Aside, String> {
    if !present(from) {
        return Ok(if present(to) {
            Aside::AlreadyThere
        } else {
            Aside::Nothing
        });
    }
    if present(to) {
        return Err(format!(
            "{} already holds a copy, so {} was not moved",
            to.display(),
            from.display()
        ));
    }
    if let Some(folder) = to.parent() {
        notchling_platform::files::ensure_private_dir(folder)
            .map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    std::fs::rename(from, to)
        .map_err(|e| format!("Couldn't move {} aside: {e}", from.display()))?;
    Ok(Aside::Moved)
}

pub struct GitRepo(pub Arc<dyn Launcher>);

fn same(a: &RepoFacts, b: &RepoFacts) -> bool {
    a.head == b.head
        && a.branch == b.branch
        && a.upstream == b.upstream
        && a.unpushed == b.unpushed
        && a.dirty == b.dirty
        && a.untracked == b.untracked
}

impl crate::cluster_returns::Repo for GitRepo {
    fn changed(&self, cwd: &str, was: Option<&RepoFacts>) -> bool {
        let Some(was) = was else {
            return true;
        };
        match crate::snap_repo::read_repo(Path::new(cwd), self.0.as_ref()) {
            Ok(Some(now)) => !same(was, &now),
            _ => true,
        }
    }
}
