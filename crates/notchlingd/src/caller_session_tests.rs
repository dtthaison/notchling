use crate::caller_session::CallerSession;
use crate::vault_caller::Processes;
use notchling_engine::sessions::Roots;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SESSION: &str = "6f1c2a9e-3b7d-4e21-9a0c-5d8e7f6a1b2c";

struct Tree(HashMap<u32, u32>);

impl Processes for Tree {
    fn parent(&self, pid: u32) -> Option<u32> {
        self.0.get(&pid).copied()
    }
    fn name(&self, _pid: u32) -> Option<String> {
        None
    }
    fn cwd(&self, _pid: u32) -> Option<PathBuf> {
        None
    }
    fn resolve(&self, _path: &Path) -> Option<PathBuf> {
        None
    }
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Scratch {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "notchlingd-caller-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    Scratch(dir)
}

fn lane_with_session(dir: &Path, name: &str, pid: u32) -> PathBuf {
    let lane = dir.join(name);
    std::fs::create_dir_all(lane.join("sessions")).unwrap();
    std::fs::write(
        lane.join("sessions").join(format!("{pid}.json")),
        format!("{{\"sessionId\":\"{SESSION}\"}}"),
    )
    .unwrap();
    lane
}

fn roots(main: &Path, lanes: &[(&str, &Path)], dir: &Path) -> Roots {
    let list: Vec<_> = lanes
        .iter()
        .map(|(n, p)| {
            serde_json::json!({
                "name": n, "path": p.to_string_lossy(), "accountID": "acc", "email": "a@b.c"
            })
        })
        .collect();
    let lanes_file = dir.join("lanes.json");
    std::fs::write(&lanes_file, serde_json::to_vec(&list).unwrap()).unwrap();
    Roots {
        main_dir: main.to_path_buf(),
        lanes_file,
    }
}

#[test]
fn session_hit_names_the_lane_and_chain() {
    let s = scratch("hit");
    let main = s.0.join("main");
    std::fs::create_dir_all(main.join("sessions")).unwrap();
    let lane = lane_with_session(&s.0, "lane-b", 300);
    let roots = roots(&main, &[("lane-b", &lane)], &s.0);
    let procs = Tree(HashMap::from([
        (500, 400),
        (400, 300),
        (300, 200),
        (200, 1),
    ]));
    let found = CallerSession::new(roots, Arc::new(procs)).session_of(500);
    let hit = found.unwrap();
    assert_eq!(hit.session_id, SESSION);
    assert_eq!(hit.claude_pid, 300);
    assert_eq!(hit.config_dir, lane);
    assert_eq!(hit.chain, vec![400, 300]);
}

#[test]
fn the_main_folder_is_a_lane_too() {
    let s = scratch("main");
    let main = lane_with_session(&s.0, "main", 300);
    let roots = roots(&main, &[], &s.0);
    let procs = Tree(HashMap::from([(500, 300), (300, 1)]));
    let hit = CallerSession::new(roots, Arc::new(procs))
        .session_of(500)
        .unwrap();
    assert_eq!(hit.config_dir, main);
    assert_eq!(hit.chain, vec![300]);
}

#[test]
fn no_session_past_sixteen_parents() {
    let s = scratch("deep");
    let main = lane_with_session(&s.0, "main", 117);
    let roots = roots(&main, &[], &s.0);
    let tree: HashMap<u32, u32> = (100..=130).map(|p| (p, p + 1)).collect();
    let call =
        |pid| CallerSession::new(roots.clone(), Arc::new(Tree(tree.clone()))).session_of(pid);
    assert!(call(100).is_none(), "117 is the 17th parent of 100");
    assert_eq!(call(101).unwrap().chain.len(), 16);
}

#[test]
fn nothing_outside_a_session() {
    let s = scratch("none");
    let main = s.0.join("main");
    let roots = roots(&main, &[], &s.0);
    let procs = Tree(HashMap::from([(500, 400), (400, 1)]));
    let c = CallerSession::new(roots, Arc::new(procs));
    assert!(c.session_of(500).is_none());
    assert!(c.session_of(1).is_none());
    assert!(c.session_of(31_337).is_none());
}
