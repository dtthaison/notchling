use super::plan::{Command, KeyStep, PtyScript, Quiet};

pub const TRUST_PROMPT: &str = "Yes, I trust this folder";
pub const DOWN: &str = "\u{1b}[B";
pub const ENTER: &str = "\r";
pub const CTRL_C: &str = "\u{03}";

pub fn plan_trust_folder(folder: &str, config_dir: Option<&str>) -> PtyScript {
    let command = Command::new(&["claude"])
        .in_dir(folder)
        .with_env("TERM", Some("xterm-256color"))
        .with_env("NO_COLOR", None)
        .for_config_dir(config_dir);
    let step = |keys: &str, pause_ms: u64, then_quiet: Option<Quiet>| KeyStep {
        keys: keys.to_string(),
        pause_ms,
        then_quiet,
    };
    PtyScript {
        command,
        rows: 50,
        cols: 200,
        wait_for: TRUST_PROMPT.to_string(),
        wait_timeout_secs: 20,
        keys: vec![
            step(DOWN, 300, None),
            step(
                ENTER,
                0,
                Some(Quiet {
                    quiet_ms: 1000,
                    timeout_ms: 8000,
                }),
            ),
            step(CTRL_C, 300, None),
            step(
                CTRL_C,
                300,
                Some(Quiet {
                    quiet_ms: 500,
                    timeout_ms: 3000,
                }),
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_then_enter_then_quit() {
        let script = plan_trust_folder("/work/new", None);
        let keys: Vec<&str> = script.keys.iter().map(|k| k.keys.as_str()).collect();
        assert_eq!(keys, ["\u{1b}[B", "\r", "\u{03}", "\u{03}"]);
        assert_eq!(script.wait_for, "Yes, I trust this folder");
        assert_eq!((script.rows, script.cols), (50, 200));
        assert_eq!(script.keys[0].pause_ms, 300);
    }

    #[test]
    fn the_hidden_terminal_gets_color_and_the_lane_folder() {
        let script = plan_trust_folder("/work/new", Some("/lanes/a"));
        assert_eq!(script.command.cwd.as_deref(), Some("/work/new"));
        assert_eq!(script.command.argv, ["claude"]);
        assert_eq!(
            script.command.env.get("TERM"),
            Some(&Some("xterm-256color".to_string()))
        );
        assert_eq!(script.command.env.get("NO_COLOR"), Some(&None));
        assert_eq!(
            script.command.env.get("CLAUDE_CONFIG_DIR"),
            Some(&Some("/lanes/a".to_string()))
        );
        let main = plan_trust_folder("/w", None);
        assert_eq!(main.command.env.get("CLAUDE_CONFIG_DIR"), Some(&None));
    }
}
