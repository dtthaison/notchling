use super::*;
use std::path::Path;

const SESSION: &str = "5e5510aa-0000-4000-8000-0000000000bb";

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!("nl-shells-guard-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    Dir(dir)
}

fn put(root: &Path, uid: u32, session: &str, id: &str) -> PathBuf {
    let tasks = root
        .join(format!("claude-{uid}"))
        .join("-work")
        .join(session)
        .join("tasks");
    std::fs::create_dir_all(&tasks).expect("tasks");
    let file = tasks.join(format!("{id}.output"));
    std::fs::write(&file, "").expect("file");
    file
}

fn check(root: &Path, path: &Path, id: &str) -> Option<PathBuf> {
    shell_output(
        path.to_str().expect("utf8"),
        SESSION,
        id,
        &[root.to_path_buf()],
        501,
    )
}

#[test]
fn the_sessions_own_tasks_file_is_accepted() {
    let dir = scratch("ok");
    let file = put(&dir.0, 501, SESSION, "bshell0001");
    assert_eq!(check(&dir.0, &file, "bshell0001"), Some(file));
}

#[test]
fn another_users_session_or_id_is_refused() {
    let dir = scratch("else");
    let other_user = put(&dir.0, 502, SESSION, "bshell0001");
    assert_eq!(check(&dir.0, &other_user, "bshell0001"), None);
    let other_session = put(&dir.0, 501, "another-session", "bshell0001");
    assert_eq!(check(&dir.0, &other_session, "bshell0001"), None);
    let file = put(&dir.0, 501, SESSION, "bshell0002");
    assert_eq!(check(&dir.0, &file, "bshell0001"), None);
    assert_eq!(check(&dir.0, &file, "../bshell0002"), None);
}

#[test]
fn a_path_of_the_wrong_shape_is_refused() {
    let dir = scratch("shape");
    let file = put(&dir.0, 501, SESSION, "bshell0001");
    let elsewhere = dir.0.join("bshell0001.output");
    std::fs::write(&elsewhere, "").expect("file");
    assert_eq!(check(&dir.0, &elsewhere, "bshell0001"), None);
    let dotted = dir
        .0
        .join("claude-501/-work")
        .join(SESSION)
        .join("tasks/../tasks/bshell0001.output");
    assert_eq!(check(&dir.0, &dotted, "bshell0001"), None);
    let tail = format!("{}/x", file.display());
    assert_eq!(check(&dir.0, Path::new(&tail), "bshell0001"), None);
    assert_eq!(check(&dir.0, Path::new(""), "bshell0001"), None);
    assert_eq!(
        check(Path::new("/nonexistent-root"), &file, "bshell0001"),
        None
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_file_or_folder_is_refused() {
    let dir = scratch("link");
    let file = put(&dir.0, 501, SESSION, "bshell0001");
    let real = dir.0.join("target.txt");
    std::fs::write(&real, "x").expect("target");
    std::fs::remove_file(&file).expect("remove");
    std::os::unix::fs::symlink(&real, &file).expect("link");
    assert_eq!(check(&dir.0, &file, "bshell0001"), None);

    let other = scratch("link-dir");
    let planted = put(&other.0, 501, "sess-link", "bshell0001");
    let session_dir = dir.0.join("claude-501/-work").join("sess-link");
    std::os::unix::fs::symlink(planted.parent().expect("tasks"), &session_dir).expect("dir link");
    let through = session_dir.join("bshell0001.output");
    let got = shell_output(
        through.to_str().expect("utf8"),
        "sess-link",
        "bshell0001",
        std::slice::from_ref(&dir.0),
        501,
    );
    assert_eq!(got, None);
}

#[test]
fn a_folder_in_place_of_the_file_is_refused() {
    let dir = scratch("folder");
    let file = put(&dir.0, 501, SESSION, "bshell0001");
    std::fs::remove_file(&file).expect("remove");
    std::fs::create_dir(&file).expect("folder");
    assert_eq!(check(&dir.0, &file, "bshell0001"), None);
}

fn proc(pid: i32, started: &str, zombie: bool) -> Proc {
    Proc {
        pid,
        ppid: 1,
        pgid: pid,
        started: started.to_string(),
        zombie,
        command: "/bin/claude".to_string(),
    }
}

fn session_file(dir: &Path, pid: u32, session: &str, started: &str) {
    let sessions = dir.join(".claude/sessions");
    std::fs::create_dir_all(&sessions).expect("sessions");
    let body = serde_json::json!({"pid": pid, "sessionId": session, "procStart": started});
    std::fs::write(sessions.join(format!("{pid}.json")), body.to_string()).expect("file");
}

#[test]
fn the_owner_is_the_one_live_process_with_the_sessions_file() {
    let dir = scratch("owner");
    let roots = Roots::new(&dir.0, &dir.0.join("data"));
    let began = "Fri Nov 20 23:53:37 2026";
    session_file(&dir.0, 4001, SESSION, began);
    let table = vec![proc(4001, began, false)];
    assert_eq!(session_owner(&roots, SESSION, &table), Some(4001));
    assert_eq!(session_owner(&roots, "another", &table), None);
    assert_eq!(session_owner(&roots, "../x", &table), None);
}

#[test]
fn a_gone_zombie_or_reused_pid_is_no_owner() {
    let dir = scratch("stale");
    let roots = Roots::new(&dir.0, &dir.0.join("data"));
    session_file(&dir.0, 4002, SESSION, "Fri Nov 20 23:53:37 2026");
    assert_eq!(session_owner(&roots, SESSION, &[]), None);
    let zombie = vec![proc(4002, "Fri Nov 20 23:53:37 2026", true)];
    assert_eq!(session_owner(&roots, SESSION, &zombie), None);
    let reused = vec![proc(4002, "Sat Nov 21 09:00:00 2026", false)];
    assert_eq!(session_owner(&roots, SESSION, &reused), None);
}

#[test]
fn two_live_processes_for_one_session_are_no_owner() {
    let dir = scratch("two");
    let roots = Roots::new(&dir.0, &dir.0.join("data"));
    let began = "Fri Nov 20 23:53:37 2026";
    session_file(&dir.0, 4003, SESSION, began);
    session_file(&dir.0, 4004, SESSION, began);
    let table = vec![proc(4003, began, false), proc(4004, began, false)];
    assert_eq!(session_owner(&roots, SESSION, &table), None);
}
