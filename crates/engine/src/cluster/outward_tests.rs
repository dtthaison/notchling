use serde_json::{json, Value};

use super::outward::{cut_steps, is_outward, CutStep};
use crate::transcript::conversation;
use crate::transcript::model::{ChatItem, Conversation, ItemKind, ToolCall, ToolKind};

fn bash(cmd: &str) -> bool {
    is_outward("Bash", &json!({ "command": cmd }))
}

fn all_outward(cmds: &[&str]) {
    for cmd in cmds {
        assert!(bash(cmd), "{cmd} should be outward");
    }
}

fn none_outward(cmds: &[&str]) {
    for cmd in cmds {
        assert!(!bash(cmd), "{cmd} should not be outward");
    }
}

fn fixture() -> Conversation {
    conversation::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/cluster/cut-push.jsonl"
    ))
}

fn item(kind: ItemKind) -> ChatItem {
    ChatItem {
        id: "i".into(),
        kind,
        date: None,
    }
}

fn call(name: &str, kind: ToolKind, input: &str, finished: bool) -> ToolCall {
    let mut call = ToolCall::new("t", name, kind, format!("Used {name}"), input.into());
    call.finished = finished;
    call
}

fn conv(items: Vec<ItemKind>) -> Conversation {
    Conversation {
        items: items.into_iter().map(item).collect(),
        ..Conversation::default()
    }
}

#[test]
fn cut_git_push_is_outward() {
    let steps = cut_steps(&fixture());
    assert_eq!(
        steps,
        vec![CutStep {
            tool: "Bash".into(),
            summary: "Push the notes branch".into(),
            outward: true,
        }]
    );
}

#[test]
fn git_status_is_not() {
    none_outward(&[
        "git status",
        "git log --oneline",
        "git diff HEAD",
        "git fetch origin",
        "git commit -m 'git push later'",
        "git remote -v",
    ]);
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
        "x=`git push`",
        "if git push; then echo ok; fi",
        "{ git push; }",
    ]);
}

#[test]
fn curl_get_is_not_curl_post_is() {
    none_outward(&[
        "curl https://example.test/a",
        "curl -s -o out.json https://example.test/a",
        "curl -X GET https://example.test/a",
        "curl -XHEAD https://example.test/a",
        "curl -I https://example.test/a",
        "curl --request get https://example.test/a",
        "curl -G -d q=1 https://example.test/a",
        "wget https://example.test/a.tgz",
        "wget --method=GET https://example.test/a",
    ]);
    all_outward(&[
        "curl -X POST https://example.test/a",
        "curl -XPOST https://example.test/a",
        "curl --request DELETE https://example.test/a",
        "curl --request=PUT https://example.test/a",
        "curl -d a=1 https://example.test/a",
        "curl -sd a=1 https://example.test/a",
        "curl --data-binary @f https://example.test/a",
        "curl --data-raw '{}' https://example.test/a",
        "curl -F file=@f https://example.test/a",
        "curl --form a=b https://example.test/a",
        "curl -T f https://example.test/a",
        "curl --upload-file f https://example.test/a",
        "curl --json '{}' https://example.test/a",
        "curl -X GET -F a=b https://example.test/a",
        "wget --post-data=a=1 https://example.test/a",
        "wget --post-file f https://example.test/a",
        "wget --method POST https://example.test/a",
        "wget --method=PUT https://example.test/a",
    ]);
}

#[test]
fn mcp_send_is_outward() {
    let input = json!({});
    for name in [
        "mcp__slack__send_message",
        "mcp__github__create_issue",
        "mcp__github__update_issue",
        "mcp__jira__delete_ticket",
        "mcp__blog__publish",
        "mcp__vercel__deploy",
        "mcp__github__merge_pull_request",
        "mcp__notes__write_note",
        "mcp__mail__reply",
        "mcp__chat__post_message",
        "mcp__Slack__SEND_MESSAGE",
    ] {
        assert!(is_outward(name, &input), "{name} should be outward");
    }
    for name in [
        "mcp__github__list_issues",
        "mcp__github__get_file_contents",
        "mcp__slack__search",
        "mcp__docs__read",
    ] {
        assert!(!is_outward(name, &input), "{name} should not be");
    }
}

#[test]
fn artifact_publish_is_outward() {
    assert!(is_outward(
        "Artifact",
        &json!({"action": "publish", "file_path": "/tmp/a.html"})
    ));
    assert!(is_outward("Artifact", &json!({"file_path": "/tmp/a.html"})));
    assert!(is_outward("Artifact", &json!({})));
    assert!(is_outward("Artifact", &json!({"mode": "delete"})));
    assert!(is_outward("Artifact", &Value::Null));
    for action in ["read", "get", "list", "view", "READ"] {
        assert!(!is_outward("Artifact", &json!({ "action": action })));
        assert!(!is_outward("Artifact", &json!({ "mode": action })));
    }
}

#[test]
fn cut_artifact_with_unparseable_input_is_outward() {
    let cut = "{\n  \"action\" : \"read\",\n  \"content\" : \"aaaa\n…";
    let c = conv(vec![
        ItemKind::user("go"),
        ItemKind::Tools(vec![call("Artifact", ToolKind::Other, cut, false)]),
    ]);
    let steps = cut_steps(&c);
    assert_eq!(steps.len(), 1);
    assert!(steps[0].outward);
    let read = "{\n  \"action\" : \"read\",\n  \"url\" : \"https://example.test/a\"\n}";
    let c = conv(vec![
        ItemKind::user("go"),
        ItemKind::Tools(vec![call("Artifact", ToolKind::Other, read, false)]),
    ]);
    assert!(!cut_steps(&c)[0].outward);
}

#[test]
fn read_and_edit_are_not() {
    for tool in [
        "Read",
        "Edit",
        "Write",
        "MultiEdit",
        "Grep",
        "Glob",
        "WebFetch",
        "WebSearch",
        "Agent",
        "TodoWrite",
        "NotebookEdit",
    ] {
        assert!(!is_outward(tool, &json!({"file_path": "/w/a.rs"})));
        assert!(!is_outward(tool, &json!({"command": "git push"})));
    }
    assert!(!is_outward("Bash", &json!({})));
    assert!(!is_outward("Bash", &json!({"command": ""})));
    assert!(!is_outward("Bash", &json!({"command": 3})));
}

#[test]
fn cut_steps_reads_unanswered_calls() {
    let c = fixture();
    let steps = cut_steps(&c);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].tool, "Bash");
    let last = c.items.last().expect("last item");
    let ItemKind::Tools(calls) = &last.kind else {
        panic!("last item is not tools");
    };
    let cut = calls.last().expect("a call");
    assert!(!cut.finished);
    assert_eq!(cut.input, "git push origin notes");
    assert!(calls.iter().filter(|c| !c.finished).count() == 1);
}

#[test]
fn cut_steps_only_looks_at_the_last_turn() {
    let c = conv(vec![
        ItemKind::user("one"),
        ItemKind::Tools(vec![call("Bash", ToolKind::Shell, "git push", false)]),
        ItemKind::user("two"),
        ItemKind::Tools(vec![
            call("Bash", ToolKind::Shell, "git push", true),
            call("Read", ToolKind::Read, "/w/a.rs", false),
            call("Bash", ToolKind::Shell, "ls", false),
        ]),
    ]);
    let steps = cut_steps(&c);
    let tools: Vec<(&str, bool)> = steps.iter().map(|s| (s.tool.as_str(), s.outward)).collect();
    assert_eq!(tools, vec![("Read", false), ("Bash", false)]);
}

#[test]
fn no_cut_steps_for_a_finished_or_empty_turn() {
    assert!(cut_steps(&Conversation::default()).is_empty());
    let c = conv(vec![
        ItemKind::user("go"),
        ItemKind::Tools(vec![call("Bash", ToolKind::Shell, "git push", true)]),
    ]);
    assert!(cut_steps(&c).is_empty());
    let c = conv(vec![
        ItemKind::Tools(vec![call("Bash", ToolKind::Shell, "git push", false)]),
        ItemKind::user("go"),
    ]);
    assert!(cut_steps(&c).is_empty());
}

#[test]
fn cut_steps_without_a_user_item_reads_every_item() {
    let c = conv(vec![ItemKind::Tools(vec![call(
        "Bash",
        ToolKind::Shell,
        "git push",
        false,
    )])]);
    assert_eq!(cut_steps(&c).len(), 1);
}

#[test]
fn cut_mcp_call_takes_its_name_from_the_call() {
    let c = conv(vec![
        ItemKind::user("go"),
        ItemKind::Tools(vec![call(
            "mcp__slack__send_message",
            ToolKind::Other,
            "{\n  \"text\" : \"hi\"\n}",
            false,
        )]),
    ]);
    let steps = cut_steps(&c);
    assert_eq!(steps[0].tool, "mcp__slack__send_message");
    assert_eq!(steps[0].summary, "Used mcp__slack__send_message");
    assert!(steps[0].outward);
}

#[test]
fn git_family_table() {
    all_outward(&[
        "git push",
        "git push --force-with-lease origin main",
        "git -C /w/repo push",
        "git -c user.name=x push",
        "git send-email --to a@example.test patch",
        "/usr/bin/git push",
    ]);
    none_outward(&["git pull", "git checkout push", "git stash push"]);
}

#[test]
fn gh_family_table() {
    all_outward(&[
        "gh pr create --fill",
        "gh pr merge 12",
        "gh pr close 12",
        "gh pr comment 12 -b hi",
        "gh -R o/r pr merge 3",
        "gh release create v1",
        "gh release upload v1 a.tgz",
        "gh api -X POST repos/o/r/issues",
        "gh api --method DELETE repos/o/r/issues/1",
        "gh api -XPATCH repos/o/r/issues/1",
        "gh api repos/o/r/issues -f title=x",
        "gh api repos/o/r/issues --input body.json",
    ]);
    none_outward(&[
        "gh pr view 12",
        "gh pr list",
        "gh pr checkout 12",
        "gh issue list",
        "gh api repos/o/r/issues",
        "gh api -X GET repos/o/r/issues",
        "gh api --method GET repos/o/r/issues -f state=open",
    ]);
}

#[test]
fn package_publish_table() {
    all_outward(&[
        "npm publish",
        "npm publish --access public",
        "npm --prefix web publish",
        "pnpm publish",
        "pnpm -r publish",
        "yarn publish",
        "yarn npm publish",
        "cargo publish",
        "cargo +nightly publish -p x",
        "docker push registry.test/app:1",
        "docker image push registry.test/app:1",
        "docker -H tcp://h:2375 push app",
    ]);
    none_outward(&[
        "npm install",
        "npm run publish-docs",
        "npm run publish",
        "pnpm install",
        "yarn add left-pad",
        "cargo build",
        "cargo test publish",
        "docker build -t app .",
        "docker pull app",
    ]);
}

#[test]
fn infra_family_table() {
    all_outward(&[
        "kubectl apply -f x.yaml",
        "kubectl delete pod p",
        "kubectl rollout restart deploy/d",
        "kubectl -n prod apply -f x.yaml",
        "kubectl --context c1 delete pod p",
        "helm install r chart",
        "helm upgrade r chart",
        "terraform apply",
        "terraform -chdir=infra apply -auto-approve",
        "terraform destroy",
        "tofu apply",
        "fly deploy",
        "flyctl deploy --remote-only",
        "firebase deploy --only hosting",
        "vercel --prod",
        "vercel deploy --prod",
        "vercel --prod --yes",
    ]);
    none_outward(&[
        "kubectl get pods",
        "kubectl describe pod p",
        "kubectl -n prod get pods",
        "kubectl logs p",
        "helm list",
        "helm template r chart",
        "terraform plan",
        "terraform init",
        "fly status",
        "firebase login",
        "vercel",
        "vercel dev",
        "vercel ls",
    ]);
}

#[test]
fn remote_shell_table() {
    all_outward(&[
        "ssh host ls",
        "ssh -p 22 user@host",
        "scp a.txt host:/tmp/",
        "scp host:/tmp/a.txt .",
        "rsync -a src/ host:/dst",
        "rsync -a src/ user@host:/dst",
        "rsync -a -e 'ssh -p 22' src/ host:dst",
        "rsync -a src/ rsync://host/mod",
        "rsync -a src/ host::mod",
        "rsync -a a b host:dst",
    ]);
    none_outward(&[
        "rsync -a src/ dst/",
        "rsync -a ./a:b dst/",
        "rsync -a host:/src ./local",
        "rsync -a --exclude a:b src/ dst/",
        "rsync --list-only src/",
        "rsync -a src/ dst/ > rsync.log",
    ]);
}

#[test]
fn cloud_cli_table() {
    all_outward(&[
        "aws s3 cp a s3://b/a",
        "aws s3 rm s3://b/a",
        "aws s3 sync . s3://b",
        "aws ec2 run-instances --image-id x",
        "aws --profile p lambda update-function-code --function-name f",
        "aws s3 cp list.txt s3://b/",
        "gcloud compute instances delete i",
        "gcloud run deploy s",
        "gcloud --project p app deploy",
        "az vm delete -n v",
        "az group create -n g",
    ]);
    none_outward(&[
        "aws s3 ls",
        "aws s3 ls s3://b/path",
        "aws ec2 describe-instances",
        "aws iam list-users",
        "aws sts get-caller-identity",
        "aws s3api list-buckets",
        "aws --profile p --region eu-west-1 s3 ls",
        "aws --version",
        "gcloud compute instances list",
        "gcloud compute instances describe i",
        "gcloud storage ls",
        "gcloud config get project",
        "az vm list",
        "az group show -n g",
        "az account show",
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
        "command git push",
        "nohup git push",
        "time git push",
        "exec git push",
        "nice -n 5 git push",
        "timeout 30 git push",
        "timeout -s KILL 30 git push",
        "xargs -n 1 git push",
        "sudo env FOO=1 git push",
        "bash -c 'git push'",
        "sh -c \"cd r && git push\"",
        "bash -lc 'git push'",
        "zsh -c 'echo; git push'",
        "eval 'git push'",
        "! git push",
    ]);
    none_outward(&[
        "sudo ls",
        "env FOO=1 ls",
        "bash -c 'git status'",
        "bash script.sh",
        "timeout 30 ls",
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
    assert!(bash(&cmd));
    for _ in 0..12 {
        cmd = format!("bash -c '{}'", cmd.replace('\'', "'\\''"));
    }
    assert!(bash(&cmd));
}
