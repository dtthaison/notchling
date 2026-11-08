//! Which processes are an app: the pure matching behind [`crate::apps`].
//!
//! The daemon has no main run loop, so the system's running-application list can be stale. The
//! process table can't be: a process either runs or it doesn't. [`app_pids`] picks an app's
//! processes out of a snapshot of it, by the file each one runs.

/// The Data volume's firmlink prefix; some tools report paths with it, `/Applications/X` without.
const DATA_VOLUME: &str = "/System/Volumes/Data";

/// The pids, sorted, whose executable is one of `executables` (the file inside each installed copy
/// of the app, `<bundle>/Contents/MacOS/<CFBundleExecutable>`).
///
/// A process counts when its path is exactly an executable, compared whole and without regard to
/// ASCII case (a default Mac volume ignores case, so the kernel's spelling can differ from the
/// bundle's). A helper in the bundle (`<bundle>/Contents/Frameworks/X Helper.app/...`) is another
/// executable and does not count: it has its own bundle id, as it did when the system's list was
/// asked, and quitting the app takes it away. A path that only starts the same way
/// (`Codex.app.bak`, `CodexBeta.app`) is another app. A translocated copy (Gatekeeper runs a
/// downloaded app from `.../AppTranslocation/<uuid>/d/<App>.app/...`, hiding where it came from)
/// counts when the path after `/d/` is the end of an executable, whole components only.
pub(crate) fn app_pids(processes: &[(i32, String)], executables: &[String]) -> Vec<i32> {
    let wanted: Vec<String> = executables
        .iter()
        .filter(|e| !e.is_empty())
        .map(|e| fold(e))
        .collect();
    let mut pids: Vec<i32> = processes
        .iter()
        .filter(|(_, path)| wanted.iter().any(|exe| is_executable(&fold(path), exe)))
        .map(|(pid, _)| *pid)
        .collect();
    pids.sort_unstable();
    pids.dedup();
    pids
}

/// A path as compared: no firmlink prefix, ASCII lowercase.
fn fold(path: &str) -> String {
    let path = match path.strip_prefix(DATA_VOLUME) {
        Some(rest) if rest.starts_with('/') => rest,
        _ => path,
    };
    path.to_ascii_lowercase()
}

/// Whether a process path (folded) is the executable `exe` (folded).
fn is_executable(process: &str, exe: &str) -> bool {
    if process == exe {
        return true;
    }
    match translocated_rest(process) {
        Some(rest) => exe
            .strip_suffix(rest)
            .is_some_and(|before| before.ends_with('/')),
        None => false,
    }
}

/// What follows `.../AppTranslocation/<uuid>/d/` in a translocated path, when it starts with the
/// app's own folder (`<App>.app/...`): the part of the original path that survives translocation.
fn translocated_rest(path: &str) -> Option<&str> {
    let after = path.split_once("/apptranslocation/")?.1;
    let (_uuid, tail) = after.split_once('/')?;
    let rest = tail.strip_prefix("d/")?;
    let (folder, _) = rest.split_once('/')?;
    folder.ends_with(".app").then_some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODEX: &str = "/Applications/Codex.app/Contents/MacOS/Codex";

    fn table(rows: &[(i32, &str)]) -> Vec<(i32, String)> {
        rows.iter().map(|(p, s)| (*p, (*s).to_string())).collect()
    }

    fn pids(rows: &[(i32, &str)], exes: &[&str]) -> Vec<i32> {
        let exes: Vec<String> = exes.iter().map(|e| (*e).to_string()).collect();
        app_pids(&table(rows), &exes)
    }

    #[test]
    fn the_process_running_the_executable_is_the_app() {
        let rows = [(1, "/sbin/launchd"), (4242, CODEX), (7, "/bin/zsh")];
        assert_eq!(pids(&rows, &[CODEX]), vec![4242]);
    }

    #[test]
    fn nothing_matches_when_the_app_is_not_in_the_table() {
        let rows = [(1, "/sbin/launchd"), (7, "/bin/zsh")];
        assert_eq!(pids(&rows, &[CODEX]), Vec::<i32>::new());
    }

    #[test]
    fn a_path_with_spaces_is_matched_whole() {
        let exe = "/Users/me/My Apps/Claude Desktop.app/Contents/MacOS/Claude Desktop";
        let rows = [
            (10, exe),
            (
                11,
                "/Users/me/My Apps/Claude Desktop.app/Contents/MacOS/Claude",
            ),
            (12, "/Users/me/My"),
        ];
        assert_eq!(pids(&rows, &[exe]), vec![10]);
    }

    #[test]
    fn another_app_whose_path_shares_a_prefix_is_not_the_app() {
        let rows = [
            (1, "/Applications/Codex.app.bak/Contents/MacOS/Codex"),
            (2, "/Applications/CodexBeta.app/Contents/MacOS/Codex"),
            (3, "/Applications/Codex.app/Contents/MacOS/Codex Helper"),
            (4, "/Applications/Codex.app/Contents/MacOS/Codex/extra"),
            (5, "/Users/me/Applications/Codex.app/Contents/MacOS/Codex"),
            (6, "/Applications/Codex.app/Contents/MacOS"),
        ];
        assert_eq!(pids(&rows, &[CODEX]), Vec::<i32>::new());
    }

    #[test]
    fn a_helper_inside_the_bundle_is_not_the_app() {
        let helper = "/Applications/Codex.app/Contents/Frameworks/Codex Helper (Renderer).app/Contents/MacOS/Codex Helper (Renderer)";
        let rows = [(20, helper), (21, CODEX), (22, helper)];
        assert_eq!(pids(&rows, &[CODEX]), vec![21]);
    }

    #[test]
    fn every_copy_of_the_app_is_found_and_the_pids_are_sorted() {
        let other = "/Users/me/Downloads/Codex.app/Contents/MacOS/Codex";
        let rows = [(90, other), (30, CODEX), (60, CODEX), (30, CODEX)];
        assert_eq!(pids(&rows, &[CODEX, other]), vec![30, 60, 90]);
    }

    #[test]
    fn the_kernels_spelling_of_the_case_still_matches() {
        let rows = [(5, "/Applications/codex.app/Contents/MacOS/codex")];
        assert_eq!(pids(&rows, &[CODEX]), vec![5]);
    }

    #[test]
    fn the_data_volume_prefix_is_ignored_on_either_side() {
        let on_data = format!("/System/Volumes/Data{CODEX}");
        let rows = [(5, on_data.as_str())];
        assert_eq!(pids(&rows, &[CODEX]), vec![5]);
        let rows = [(6, CODEX)];
        assert_eq!(pids(&rows, &[on_data.as_str()]), vec![6]);
        // Only a whole leading folder is the prefix.
        let rows = [(
            7,
            "/System/Volumes/DataX/Applications/Codex.app/Contents/MacOS/Codex",
        )];
        assert_eq!(pids(&rows, &[CODEX]), Vec::<i32>::new());
    }

    #[test]
    fn a_translocated_copy_is_the_app() {
        let moved = "/private/var/folders/ab/cdef/T/AppTranslocation/6F1C2B7E-0000-4000-8000-123456789ABC/d/Codex.app/Contents/MacOS/Codex";
        let downloaded = "/Users/me/Downloads/Codex.app/Contents/MacOS/Codex";
        let rows = [(8, moved), (9, "/sbin/launchd")];
        assert_eq!(pids(&rows, &[downloaded]), vec![8]);
        assert_eq!(pids(&rows, &[CODEX]), vec![8]);
    }

    #[test]
    fn a_translocated_copy_of_another_app_is_not_the_app() {
        let base = "/private/var/folders/ab/cdef/T/AppTranslocation/6F1C2B7E-0000-4000-8000-123456789ABC/d";
        let beta = format!("{base}/CodexBeta.app/Contents/MacOS/Codex");
        let bak = format!("{base}/Codex.app.bak/Contents/MacOS/Codex");
        let tail_only = format!("{base}/Contents/MacOS/Codex");
        let helper = format!("{base}/Codex.app/Contents/Frameworks/H.app/Contents/MacOS/H");
        let rows = [
            (1, beta.as_str()),
            (2, bak.as_str()),
            (3, tail_only.as_str()),
            (4, helper.as_str()),
        ];
        assert_eq!(pids(&rows, &[CODEX]), Vec::<i32>::new());
    }

    #[test]
    fn a_translocation_path_with_no_app_after_it_matches_nothing() {
        let rows = [
            (1, "/x/AppTranslocation/uuid/d/"),
            (2, "/x/AppTranslocation/uuid"),
            (
                3,
                "/x/AppTranslocation/uuid/other/Codex.app/Contents/MacOS/Codex",
            ),
        ];
        assert_eq!(pids(&rows, &[CODEX]), Vec::<i32>::new());
    }

    #[test]
    fn no_executable_or_an_empty_one_matches_nothing() {
        let rows = [(1, CODEX), (2, "")];
        assert_eq!(pids(&rows, &[]), Vec::<i32>::new());
        assert_eq!(pids(&rows, &[""]), Vec::<i32>::new());
    }
}
