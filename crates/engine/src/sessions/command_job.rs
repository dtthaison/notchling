//! Template "exec" jobs: a shell command, not a conversation.

/// The command without its long `env PATH=… HOME=…` prefix.
pub fn readable(_command: &str) -> String {
    todo!()
}

/// The file the command's output goes to: the target of its last `>` or `>>` (not `2>`, not `>&`).
pub fn output_path(_command: &str, _cwd: &str, _home: &str) -> Option<String> {
    todo!()
}

/// The last `max_lines` lines of a file, read from its last 64 KB.
pub fn tail(_path: &str, _max_lines: usize) -> Option<String> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::super::testutil::TempDir;
    use super::*;

    // CommandJobTests.testOutputPath
    #[test]
    fn output_path_finds_the_redirect() {
        let sd = "cd /p && env HOME=/h PATH=/a:/b /venv/python3 /x/sd.py autopilot > /repo/.git/run.log 2>&1";
        let at = |c: &str| output_path(c, "/p", "/home/me");
        assert_eq!(at(sd).as_deref(), Some("/repo/.git/run.log"));
        assert_eq!(at("make test >> build.log").as_deref(), Some("/p/build.log"));
        assert_eq!(at("run > 'my out.log' 2>&1").as_deref(), Some("/p/my out.log"));
        assert_eq!(at("run 2>&1"), None);
        assert_eq!(at("run > /dev/null"), None);
        assert_eq!(at("npm test"), None);
    }

    #[test]
    fn output_path_edge_cases() {
        let at = |c: &str| output_path(c, "/p", "/home/me");
        assert_eq!(at("run 1> out.log").as_deref(), Some("/p/out.log"));
        assert_eq!(at("run > ~/out.log").as_deref(), Some("/home/me/out.log"));
        assert_eq!(at("run > \"a b.log\"").as_deref(), Some("/p/a b.log"));
        assert_eq!(at("a > first.log; b > second.log").as_deref(), Some("/p/second.log"));
        assert_eq!(at(">out.log run").as_deref(), Some("/p/out.log"));
        assert_eq!(at("run >&2"), None);
        assert_eq!(at("run 2> err.log"), None);
    }

    // CommandJobTests.testReadable
    #[test]
    fn readable_shortens_the_env_prefix() {
        assert_eq!(readable("cd /p && env HOME=/h PATH=/a:/b python3 sd.py"), "cd /p && env … python3 sd.py");
        assert_eq!(readable("env A='x y' B=\"z w\" run"), "env … run");
        assert_eq!(readable("make test"), "make test");
        assert_eq!(readable("env run"), "env run");
    }

    // CommandJobTests.testTail
    #[test]
    fn tail_keeps_the_last_lines() {
        let tmp = TempDir::new("tail");
        let text: String = (1..=300).map(|n| format!("line {n}\n")).collect();
        let p = tmp.write("x.log", &text);
        assert_eq!(tail(&p.display().to_string(), 3).as_deref(), Some("line 298\nline 299\nline 300"));
        assert_eq!(tail("/nonexistent/file.log", 3), None);
    }

    #[test]
    fn tail_drops_a_partial_first_line_of_a_big_file() {
        let tmp = TempDir::new("tail-big");
        let text: String = (1..=20000).map(|n| format!("line number {n}\n")).collect();
        let p = tmp.write("big.log", &text);
        let out = tail(&p.display().to_string(), 100_000).unwrap_or_default();
        assert!(out.starts_with("line number "), "{}", &out[..20]);
        assert!(out.ends_with("line number 20000"));
        assert!(out.len() <= 65_536);
    }
}
