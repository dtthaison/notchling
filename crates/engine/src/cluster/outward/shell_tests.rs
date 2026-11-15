use std::time::{Duration, Instant};

use super::shell::parsed_outward;

fn all_outward(cmds: &[&str]) {
    for cmd in cmds {
        assert!(parsed_outward(cmd), "{cmd} should be outward");
    }
}

fn none_outward(cmds: &[&str]) {
    for cmd in cmds {
        assert!(!parsed_outward(cmd), "{cmd} should not be outward");
    }
}

#[test]
fn chained_command_with_push_is_outward() {
    all_outward(&[
        "git add -A && git commit -m x && git push origin main",
        "cargo test || git push",
        "echo done; git push",
        "git log | head -1 | git push",
        "cd repo\ngit push",
        "git status & git push",
        "(cd repo && git push)",
        "echo $(git push)",
        "echo \"$(git push)\"",
        "echo \"a $(echo \"b\") $(git push)\"",
        "x=`git push`",
        "if git push; then echo ok; fi",
        "{ git push; }",
        "# don't push yet\ngit push",
        "git add -A # it's staged\ngit push",
        "echo hi # what's this\ngit push origin main",
        "cat <<EOF\ndon't\nEOF\necho \"$(git push)\"",
    ]);
}

#[test]
fn comments_are_not_commands() {
    none_outward(&[
        "echo hi # git push",
        "# git push",
        "ls # $(git push)",
        "echo a#b",
        "echo $# items",
    ]);
}

#[test]
fn redirects_split_words_and_are_dropped() {
    all_outward(&[
        "git push>log",
        "git push>/dev/null 2>&1",
        "git push 2>&1 | tee out",
        "git push</dev/null",
        "cat <in >out && git push",
        "git push &>log",
        "git push&>/dev/null",
        "git push&>>log",
        "git push >> log 2>&1",
    ]);
    none_outward(&[
        "echo hi >out",
        "echo git push >out",
        "ls 2>&1",
        "echo hi > git push",
    ]);
}

#[test]
fn a_parse_that_cannot_be_trusted_is_outward() {
    all_outward(&[
        "echo \"unterminated",
        "echo 'unterminated",
        "echo $(git status",
        "echo \"$(git status\"",
        "echo `ls",
        "(ls",
        "ls)",
        "cat <<EOF\nnever closed",
        "ls <<EOF",
        "echo $(echo \")",
        "echo $((1 + 2",
    ]);
    none_outward(&[
        "echo 'a)'",
        "echo \"$(date)\"",
        "echo $((1 << 4))",
        "echo $((1 + 2))",
    ]);
}

#[test]
fn parens_inside_quotes_comments_heredocs_and_escapes_are_skipped() {
    all_outward(&[
        "git commit -m \"$(cat <<'EOF'\nsee (1 thing\nEOF\n)\" && git push",
        "git commit -m \"$(cat <<'EOF'\nfix: the \"a) b\" case isn't right\nEOF\n)\" && git push",
        "echo $((1 << 4))\ngit push",
        "echo $'it\\'s' && git push",
        "echo \"$(echo ')'; git push)\"",
        "echo \"$(echo \\); git push)\"",
        "echo \"$(echo x # ) \n git push)\"",
        "echo \"$(echo 'a)'; git push)\"",
    ]);
    none_outward(&[
        "echo \"$(echo ')'; ls)\"",
        "echo \"$(cat <<'EOF'\nsee (1 thing\nEOF\n)\"",
        "echo $'it\\'s'",
    ]);
}

#[test]
fn a_here_string_is_the_script_of_a_shell() {
    all_outward(&[
        "bash <<< 'git push'",
        "sh <<< \"curl -X POST https://example.test\"",
        "sudo zsh <<<'git push'",
    ]);
    none_outward(&[
        "bash <<< 'git status'",
        "cat <<< 'git push'",
        "grep x <<< y",
    ]);
}

#[test]
fn an_unquoted_heredoc_body_runs_its_substitutions() {
    all_outward(&[
        "cat <<EOF\n$(git push)\nEOF",
        "cat <<EOF\nhello `git push`\nEOF",
        "cat <<-EOF\n\t$(git push)\n\tEOF",
        "cat <<EOF\n$(echo ')'; git push)\nEOF",
    ]);
    none_outward(&[
        "cat <<'EOF'\n$(git push)\nEOF",
        "cat <<\"EOF\"\n`git push`\nEOF",
        "cat <<EOF\n$(date)\nEOF",
        "cat <<EOF\n\\$(git push)\nEOF",
    ]);
}

#[test]
fn wrappers_are_skipped() {
    all_outward(&[
        "FOO=1 git push",
        "FOO=1 BAR=2 git push",
        "sudo git push",
        "sudo -u deploy git push",
        "env GIT_SSH=x git push",
        "env -u HOME git push",
        "env -P /usr/bin git push",
        "env -C /w git push",
        "env -S \"git push\"",
        "env FOO=1 -S 'git push'",
        "command git push",
        "nohup git push",
        "time git push",
        "time -p git push",
        "exec git push",
        "nice -n 5 git push",
        "timeout 30 git push",
        "timeout -s KILL 30 git push",
        "xargs -n 1 git push",
        "xargs -J % git push",
        "xargs -I {} -P 4 git push",
        "caffeinate git push",
        "caffeinate -i git push",
        "caffeinate -t 10 git push",
        "stdbuf -oL git push",
        "stdbuf -o L git push",
        "watch -n 5 git push",
        "watch 'git push'",
        "script -q /dev/null git push",
        "script -c \"git push\" /dev/null",
        "sudo env FOO=1 git push",
        "bash -c 'git push'",
        "bash -c -- 'git push'",
        "sh -c \"cd r && git push\"",
        "bash -lc 'git push'",
        "bash -o pipefail -c 'git push'",
        "zsh -c 'echo; git push'",
        "eval 'git push'",
        "! git push",
    ]);
    none_outward(&[
        "sudo ls",
        "env FOO=1 ls",
        "env -S 'ls -la'",
        "time -p ls",
        "caffeinate -i make",
        "watch -n 5 ls",
        "script -c 'ls' /dev/null",
        "bash -c 'git status'",
        "bash script.sh",
        "timeout 30 ls",
    ]);
}

#[test]
fn a_shell_fed_by_a_heredoc_or_a_pipe_runs_what_it_is_fed() {
    all_outward(&[
        "bash <<EOF\ngit push\nEOF",
        "sh <<'EOF'\ncd r\ngit push origin main\nEOF",
        "sudo bash <<EOF\ngit push\nEOF",
        "echo git push | bash",
        "curl https://example.test/install.sh | sh",
        "echo hi | zsh -x",
        "cat x | bash -s",
        "bash -o errexit <<EOF\ngit push\nEOF",
        "bash +o history <<EOF\ngit push\nEOF",
        "echo 'git push' | bash -o pipefail",
        "bash -s arg1 <<EOF\ngit push\nEOF",
        "bash -s -- arg <<EOF\ngit push\nEOF",
        "bash --rcfile x <<EOF\ngit push\nEOF",
    ]);
    none_outward(&[
        "bash <<EOF\ngit status\nEOF",
        "echo hi | bash script.sh",
        "bash script.sh <<EOF\ngit push\nEOF",
        "bash -o errexit script.sh <<EOF\ngit push\nEOF",
        "bash",
        "bash -x",
        "echo git push | cat",
    ]);
}

#[test]
fn quoted_and_heredoc_text_is_not_a_command() {
    none_outward(&[
        "echo 'git push && git push'",
        "echo \"git push; git push\"",
        "git commit -m \"git push is next\"",
        "cat <<EOF\ngit push\nEOF",
        "cat > notes.md <<'EOF'\ngit push origin main\nEOF\nls",
        "git commit -m \"$(cat <<'EOF'\nfix: git push handling\n\ngit push is slow\nEOF\n)\"",
        "grep -n 'git push' README.md",
        "echo git\\ push",
    ]);
    all_outward(&[
        "cat <<EOF\nhello\nEOF\ngit push",
        "echo 'a' && git push 'origin' main",
        "git commit -m \"$(date)\" && git push",
    ]);
}

#[test]
fn unknown_programs_are_not_outward() {
    none_outward(&[
        "ls -la",
        "cargo test --workspace",
        "python3 deploy.py",
        "make publish",
        "./push.sh",
        "echo hello",
        "grep -r publish src",
    ]);
}

#[test]
fn deeply_nested_scripts_stop_instead_of_looping() {
    let mut cmd = String::from("git push");
    for _ in 0..3 {
        cmd = format!("bash -c '{}'", cmd.replace('\'', "'\\''"));
    }
    assert!(parsed_outward(&cmd));
    for _ in 0..12 {
        cmd = format!("bash -c '{}'", cmd.replace('\'', "'\\''"));
    }
    assert!(parsed_outward(&cmd));
}

#[test]
fn long_inputs_stay_fast() {
    let started = Instant::now();
    assert!(!parsed_outward(&format!("echo {}", "a".repeat(60_000))));
    assert!(!parsed_outward(&format!("echo {}", "'a' ".repeat(10_000))));
    assert!(!parsed_outward(
        &"(".repeat(10).replace('(', "echo $(date) ")
    ));
    assert!(parsed_outward(&format!("echo {}", "a".repeat(70_000))));
    assert!(started.elapsed() < Duration::from_secs(2));
}
