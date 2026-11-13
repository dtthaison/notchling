use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lane {
    pub name: String,
    pub path: String,
    #[serde(rename = "accountID")]
    pub account_id: String,
    pub email: String,
    #[serde(default)]
    pub folder: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roots {
    pub main_dir: PathBuf,
    pub lanes_file: PathBuf,
}

impl Roots {
    pub fn new(home: &Path, data_dir: &Path) -> Self {
        Roots {
            main_dir: home.join(".claude"),
            lanes_file: data_dir.join("lanes.json"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigDir {
    pub dir: PathBuf,
    pub lane: Option<Lane>,
}

pub fn read_lanes(file: &Path) -> Vec<Lane> {
    std::fs::read(file)
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

pub fn config_dirs(roots: &Roots) -> Vec<ConfigDir> {
    let mut dirs = vec![ConfigDir {
        dir: roots.main_dir.clone(),
        lane: None,
    }];
    for lane in read_lanes(&roots.lanes_file) {
        dirs.push(ConfigDir {
            dir: PathBuf::from(&lane.path),
            lane: Some(lane),
        });
    }
    dirs
}

pub fn lane_at_path(roots: &Roots, path: &str) -> Option<Lane> {
    let wanted = standardize(path);
    read_lanes(&roots.lanes_file)
        .into_iter()
        .find(|l| standardize(&l.path) == wanted)
}

pub(crate) fn standardize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    let joined = parts.join("/");
    if path.starts_with('/') {
        format!("/{joined}")
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::TempDir;
    use super::*;

    #[test]
    fn main_dir_comes_first_then_lanes_from_lanes_json() {
        let tmp = TempDir::new("lanes");
        let lanes = r#"[{"name":"b","path":"/lanes/b","accountID":"acc-1","email":"b@example.com","folder":"/work"},
                        {"name":"c","path":"/lanes/c","accountID":"acc-2","email":"c@example.com"}]"#;
        tmp.write("data/lanes.json", lanes);
        let roots = Roots::new(&tmp.path("home"), &tmp.path("data"));
        let dirs = config_dirs(&roots);
        assert_eq!(dirs.len(), 3);
        assert_eq!(dirs[0].dir, tmp.path("home/.claude"));
        assert!(dirs[0].lane.is_none());
        assert_eq!(dirs[1].dir, PathBuf::from("/lanes/b"));
        assert_eq!(
            dirs[1].lane.as_ref().map(|l| l.account_id.as_str()),
            Some("acc-1")
        );
        assert_eq!(
            dirs[1].lane.as_ref().and_then(|l| l.folder.as_deref()),
            Some("/work")
        );
        assert_eq!(dirs[2].lane.as_ref().and_then(|l| l.folder.clone()), None);
    }

    #[test]
    fn missing_or_broken_lanes_file_means_main_only() {
        let tmp = TempDir::new("lanes-none");
        let roots = Roots::new(&tmp.path("home"), &tmp.path("data"));
        assert_eq!(config_dirs(&roots).len(), 1);
        tmp.write("data/lanes.json", "{not json");
        assert_eq!(config_dirs(&roots).len(), 1);
    }

    #[test]
    fn lane_lookup_ignores_trailing_slash_and_dot_parts() {
        let tmp = TempDir::new("lanes-find");
        tmp.write(
            "data/lanes.json",
            r#"[{"name":"b","path":"/lanes/b","accountID":"a","email":"e"}]"#,
        );
        let roots = Roots::new(&tmp.path("home"), &tmp.path("data"));
        assert!(lane_at_path(&roots, "/lanes/./b/").is_some());
        assert!(lane_at_path(&roots, "/lanes/other").is_none());
    }
}
