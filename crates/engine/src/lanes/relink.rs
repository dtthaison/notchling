use super::{Lane, SHARED};
use std::fs;
use std::path::Path;
use std::time::SystemTime;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Relinked {
    pub linked: Vec<String>,
    pub took_lane_copy: Vec<String>,
    pub set_aside: Vec<String>,
}

fn modified(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

#[cfg(unix)]
fn make_link(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(not(unix))]
fn make_link(_target: &Path, _link: &Path) -> bool {
    false
}

pub fn relink(lane: &Lane, main_dir: &Path, now_secs: u64) -> Relinked {
    let dir = Path::new(&lane.path);
    let mut done = Relinked::default();
    for name in SHARED {
        let link = dir.join(name);
        let target = main_dir.join(name);
        match fs::symlink_metadata(&link) {
            Ok(meta) if meta.file_type().is_symlink() => {
                if fs::read_link(&link).is_ok_and(|dest| dest == target) {
                    continue;
                }
                let _ = fs::remove_file(&link);
            }
            Ok(meta) => {
                let mine = modified(&link);
                let theirs = modified(&target);
                if !meta.is_dir() && mine > theirs {
                    let backup = main_dir.join(format!("{name}.lane-backup"));
                    let _ = fs::remove_file(&backup);
                    let _ = fs::copy(&target, &backup);
                    let _ = fs::remove_file(&target);
                    if fs::rename(&link, &target).is_ok() {
                        done.took_lane_copy.push(name.into());
                    }
                } else {
                    let aside = dir.join(format!("{name}.lane-old-{now_secs}"));
                    if fs::rename(&link, &aside).is_ok() {
                        done.set_aside.push(name.into());
                    }
                }
            }
            Err(_) => {}
        }
        if fs::symlink_metadata(&link).is_err() && make_link(&target, &link) {
            done.linked.push(name.into());
        }
    }
    done
}
