use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "notchling-task-output-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("folder");
    dir
}

fn put(tmp: &Path, user: &str, project: &str, session: &str, id: &str) -> PathBuf {
    let tasks = tmp.join(user).join(project).join(session).join("tasks");
    fs::create_dir_all(&tasks).expect("tasks folder");
    let file = tasks.join(format!("{id}.output"));
    fs::write(&file, "").expect("output");
    file
}

#[test]
fn a_shells_output_file_is_found_under_any_user_project_and_session_folder() {
    let tmp = scratch("any");
    let one = put(&tmp, "claude-501", "-work-a", "s1", "bshell0001");
    let two = put(&tmp, "claude-501", "-work-b", "s2", "bshell0002");
    assert_eq!(find(&tmp, "bshell0001"), Found::At(one));
    assert_eq!(find(&tmp, "bshell0002"), Found::At(two));
    assert_eq!(find(&tmp, "bshell0003"), Found::Missing);
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn only_claude_user_folders_are_searched_and_without_one_nothing_can_be_said() {
    let tmp = scratch("users");
    put(&tmp, "other-501", "-work-a", "s1", "bshell0001");
    assert_eq!(find(&tmp, "bshell0001"), Found::Unsearchable);
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn an_id_that_is_not_plain_finds_nothing() {
    let tmp = scratch("plain");
    put(&tmp, "claude-501", "-work-a", "s1", "bshell0001");
    for id in ["", "../x", "a/b", "b shell", "bshell0001/../bshell0001"] {
        assert_eq!(find(&tmp, id), Found::Unsearchable, "{id:?}");
    }
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn a_missing_tmp_folder_finds_nothing() {
    assert_eq!(
        find(Path::new("/nonexistent-notchling-tmp"), "bshell0001"),
        Found::Unsearchable
    );
}
