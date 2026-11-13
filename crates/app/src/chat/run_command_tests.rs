use super::run_command::{available, command, how, message, RunHow};
use crate::chat::data::Tool;

fn run(language: &str, text: &str) -> Option<String> {
    command(language, text)
}

#[test]
fn the_screenshots_block_runs_without_its_bang() {
    let text = "! git worktree remove --force … && git branch -D …";
    assert_eq!(
        run("", text).as_deref(),
        Some("git worktree remove --force … && git branch -D …")
    );
}

#[test]
fn a_dollar_prompt_is_stripped() {
    assert_eq!(run("", "$ cargo test").as_deref(), Some("cargo test"));
    assert_eq!(run("", "$ cargo test\n").as_deref(), Some("cargo test"));
}

#[test]
fn shell_languages_run_whatever_their_first_word() {
    for lang in ["sh", "bash", "zsh", "shell", "console", "terminal", "Bash"] {
        assert_eq!(
            run(lang, "./build.sh --install").as_deref(),
            Some("./build.sh --install"),
            "{lang}"
        );
    }
    assert_eq!(
        run("bash title=x", "echo hi").as_deref(),
        Some("echo hi"),
        "an info string keeps its first word"
    );
    assert_eq!(
        run("sh", "frobnicate --now").as_deref(),
        Some("frobnicate --now")
    );
}

#[test]
fn other_shells_and_languages_never_run() {
    assert_eq!(run("fish", "git status"), None);
    assert_eq!(run("powershell", "git status"), None);
    assert_eq!(run("rust", "cargo test"), None);
    assert_eq!(run("python", "git status"), None);
    assert_eq!(run("json", "{\"a\": 1}"), None);
    assert_eq!(run("diff", "- git status\n+ git log"), None);
}

#[test]
fn two_commands_join_with_and() {
    assert_eq!(
        run("bash", "cd crates\ncargo test --workspace").as_deref(),
        Some("cd crates && cargo test --workspace")
    );
    assert_eq!(
        run("", "$ git add .\n$ git commit -m wip").as_deref(),
        Some("git add . && git commit -m wip")
    );
}

#[test]
fn continuations_collapse_into_one_line() {
    let text = "cargo test \\\n  --workspace \\\n  --locked";
    assert_eq!(
        run("bash", text).as_deref(),
        Some("cargo test --workspace --locked")
    );
    let text = "$ docker run \\\n    -p 80:80 nginx\n$ docker ps";
    assert_eq!(
        run("", text).as_deref(),
        Some("docker run -p 80:80 nginx && docker ps")
    );
}

#[test]
fn a_trailing_or_leading_operator_joins_with_a_space() {
    assert_eq!(
        run("sh", "make build &&\nmake test").as_deref(),
        Some("make build && make test")
    );
    assert_eq!(
        run("sh", "cat log |\n  grep error").as_deref(),
        Some("cat log | grep error")
    );
    assert_eq!(
        run("sh", "make build\n  && make test").as_deref(),
        Some("make build && make test")
    );
}

#[test]
fn comments_and_blank_lines_are_dropped_in_shell_blocks() {
    assert_eq!(
        run("bash", "# build it\ncargo build\n\n# then test\ncargo test").as_deref(),
        Some("cargo build && cargo test")
    );
    assert_eq!(run("bash", "# only a comment"), None);
    assert_eq!(run("bash", ""), None);
}

#[test]
fn a_console_block_keeps_the_prompted_lines_and_skips_the_output() {
    let text = "$ cargo test\nrunning 3 tests\ntest result: ok\n$ git status\nOn branch main";
    assert_eq!(
        run("console", text).as_deref(),
        Some("cargo test && git status")
    );
    // A continuation after a prompted line is part of it, not output.
    let text = "$ cargo test \\\n  --locked\nok";
    assert_eq!(run("console", text).as_deref(), Some("cargo test --locked"));
}

#[test]
fn an_untagged_block_needs_every_line_to_be_prompted_or_a_command() {
    assert_eq!(run("", "! ls -la\n$ pwd").as_deref(), Some("ls -la && pwd"));
    assert_eq!(run("text", "git status").as_deref(), Some("git status"));
    assert_eq!(run("code", "git status").as_deref(), Some("git status"));
    // Some lines prompted, one not: output mixed in, so not a command block.
    assert_eq!(run("", "$ cargo test\nrunning 3 tests"), None);
}

#[test]
fn untagged_command_words() {
    for text in [
        "git log --oneline -5",
        "cargo clippy --workspace -- -D warnings",
        "npm run build",
        "pnpm install",
        "yarn test",
        "swift test",
        "make test",
        "ls -la",
        "cd ~/go/src",
        "cat Cargo.toml",
        "grep -rn foo crates",
        "rg 'TODO' src",
        "open -a Notchling",
        "brew install ripgrep",
        "python3 tools/run.py",
        "node build.js",
        "tools/bundle-next.sh",
        "./build.sh --dev",
        "claude --version",
        "codex login",
        "notchling status --json",
        "gh pr list",
        "docker ps -a",
        "kubectl get pods",
    ] {
        assert_eq!(run("", text).as_deref(), Some(text), "{text}");
    }
}

#[test]
fn untagged_prose_and_code_never_qualify() {
    for text in [
        "This is a plain sentence about git.",
        "make sure you test it first",
        "open the settings and pick a lane",
        "fn main() {\n    println!(\"hi\");\n}",
        "use std::fs;\nlet x = 1;",
        "{\n  \"git\": true\n}",
        "[1, 2, 3]",
        "- git status\n+ git log",
        "diff --git a/x b/x\n--- a/x\n+++ b/x",
        "import Foundation\nlet x = 1",
        "error: could not compile `foo`",
        "Cargo.toml",
        "",
        "   \n  ",
    ] {
        assert_eq!(run("", text), None, "{text}");
    }
}

#[test]
fn a_long_untagged_block_is_not_a_command() {
    let text = "git add a\ngit add b\ngit add c\ngit add d\ngit add e\ngit add f\ngit add g";
    assert_eq!(run("", text), None);
}

#[test]
fn scripts_cannot_run_as_one_line() {
    assert_eq!(run("bash", "#!/bin/sh\necho hi"), None);
    assert_eq!(run("bash", "for f in *.rs; do\n  echo $f\ndone"), None);
    assert_eq!(run("bash", "if [ -f x ]; then\n  echo y\nfi"), None);
    assert_eq!(run("bash", "cat <<EOF\nhello\nEOF"), None);
    assert_eq!(run("bash", "git commit -m \"title\n\nbody\""), None);
    assert_eq!(run("bash", "greet() {\n  echo hi\n}"), None);
}

#[test]
fn a_shell_block_over_ten_lines_is_a_script() {
    let text = (0..11).map(|i| format!("echo {i}\n")).collect::<String>();
    assert_eq!(run("bash", &text), None);
}

#[test]
fn a_dollar_variable_is_not_a_prompt() {
    assert_eq!(
        run("bash", "$HOME/bin/tool --go").as_deref(),
        Some("$HOME/bin/tool --go")
    );
}

#[test]
fn the_message_starts_shell_mode() {
    assert_eq!(message("cargo test"), "! cargo test");
}

#[test]
fn run_shows_only_where_the_reply_box_takes_a_bang() {
    assert!(available(Tool::Claude, true, false));
    // Read-only, an exited session, acting off.
    assert!(!available(Tool::Claude, false, false));
    // A subagent's transcript is only read.
    assert!(!available(Tool::Claude, true, true));
    // `!` is Claude Code's shell mode: Codex and Antigravity have none.
    assert!(!available(Tool::Codex, true, false));
    assert!(!available(Tool::Antigravity, true, false));
}

const SSH_SECRET: &str = r#"ssh -t deploy@100.69.89.73 'read -rsp "Google client secret: " s; echo; umask 077; printf %s "$s" > /opt/…/google-secret; bash /opt/…/setup.sh | tail -2'"#;

#[test]
fn the_ssh_secret_block_is_a_command_that_needs_a_terminal() {
    let found = command("", SSH_SECRET).expect("a block with no language that starts with ssh");
    assert_eq!(found, SSH_SECRET);
    assert_eq!(how(&found), RunHow::Terminal);
}

#[test]
fn a_remote_command_without_a_tty_is_sent() {
    for text in [
        "ssh host uptime",
        "ssh deploy@host uptime",
        "ssh -i ~/.ssh/key host ls",
        "ssh -p 2222 host uptime",
        "ssh -o BatchMode=yes host 'df -h'",
    ] {
        let found = command("", text).unwrap_or_else(|| panic!("{text}"));
        assert_eq!(how(&found), RunHow::Send, "{text}");
    }
}

#[test]
fn a_tty_or_a_login_shell_needs_a_terminal() {
    for text in [
        "ssh -t host sudo reboot",
        "ssh -tt host",
        "ssh deploy@host",
        "ssh -i key -p 22 host",
        "sudo make install",
        "docker run -it ubuntu bash",
        "docker exec -it web sh",
        "vim notes.md",
        "cargo test 2>&1 | less",
        "read -s TOKEN && echo ok",
        "passwd",
        "gh auth login",
        "git rebase -i HEAD~3",
        "FOO=1 sudo ls",
    ] {
        assert_eq!(how(text), RunHow::Terminal, "{text}");
    }
    // A program started with no arguments opens a session of its own.
    for text in ["claude", "codex", "python3", "node"] {
        assert_eq!(how(text), RunHow::Terminal, "{text}");
        assert_eq!(command("bash", text).as_deref(), Some(text));
    }
}

#[test]
fn plain_commands_are_sent() {
    for text in [
        "git status",
        "cargo test --workspace",
        "docker ps -a",
        "docker run ubuntu echo hi",
        "claude --version",
        "codex login",
        "curl -s https://example.com",
        "rsync -av a/ host:b/",
        "scp a host:b",
        "bash ./setup.sh",
        "git add .",
        "echo reading -s",
    ] {
        assert_eq!(how(text), RunHow::Send, "{text}");
    }
}

#[test]
fn more_untagged_command_words() {
    for text in [
        "scp build.tar host:/tmp",
        "rsync -av a/ host:b/",
        "curl -s https://example.com/x.json",
        "sudo launchctl list",
        "bash ./setup.sh",
        "bash /opt/app/setup.sh",
        "sh tools/run.sh",
        "zsh ~/bin/x.sh",
    ] {
        assert_eq!(command("", text).as_deref(), Some(text), "{text}");
    }
    // A shell's name starts prose as easily as a command.
    for text in ["bash is the default shell", "sh", "bash", "zsh -l"] {
        assert_eq!(command("", text), None, "{text}");
    }
}
