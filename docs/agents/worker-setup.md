# Worker setup: agent user, push wrapper, pre-start check

Status: **untried.** Nothing here has run on a real worker yet. It waits for the GitHub App, the agent OS user and the maintainer. Each command below is the intended one. Check it on one machine before the rest, and fix this page where it's wrong.

The design is in `docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md` ("The flow" items 2 and 3, "GitHub setup"). The code is `tools/agents/preflight.py` and `tools/agents/push_wrapper.py`.

## What runs as whom

| Who | Does | Holds |
| --- | --- | --- |
| `sd` user (the maintainer's account on the worker) | runs `preflight.py`, starts the agent, calls the wrapper through sudo | a read token for `preflight.py` (see Step 5) |
| `notchling-agent` | the agent: edits, builds, tests, commits in its worktree | **no GitHub credential of any kind** |
| `notchling-push` | `push_wrapper.py`: mints the App token, pushes `agent/*`, opens the PR | the App's private key file, mode 0600 |

The agent never pushes. It leaves a clean worktree on `agent/<issue>-<slug>`, and sd runs the wrapper.

## 1. The agent user

macOS:

```sh
sudo sysadminctl -addUser notchling-agent -fullName "Notchling Agent" -password - -home /Users/notchling-agent
```

Linux:

```sh
sudo useradd --create-home --shell /bin/bash notchling-agent
```

It gets no `gh` login, no ssh key, no `~/.git-credentials`, no `GITHUB_TOKEN` or `GH_TOKEN` in its environment, and no Keychain item for GitHub. Check as that user:

```sh
sudo -u notchling-agent -H sh -c 'gh auth status; ls ~/.ssh ~/.git-credentials ~/.config/gh; env | grep -i -E "github|gh_token"'
sudo -u notchling-agent -H git ls-remote https://github.com/OWNER/REPO.git agent/none   # must ask for a login or fail
```

The worktrees it works in are readable by `notchling-push` (the wrapper only reads them): mode 0750 on the worktree root, with `notchling-push` in a shared group, or 0755.

## 2. The wrapper user

macOS:

```sh
sudo sysadminctl -addUser notchling-push -fullName "Notchling Push" -password - -home /Users/notchling-push
```

Linux:

```sh
sudo useradd --create-home --shell /usr/sbin/nologin notchling-push
```

It must not be able to log in over ssh and must not be in the admin group.

Install the scripts where neither the agent nor sd can change them:

```sh
sudo install -d -o root -g wheel -m 0755 /opt/notchling/agents      # Linux: -g root
sudo cp tools/agents/preflight.py tools/agents/push_wrapper.py /opt/notchling/agents/
sudo install -d -o root -g wheel -m 0755 /opt/notchling/guard
sudo cp -R tools/guard/guard /opt/notchling/guard/guard             # preflight imports MAINTAINERS from it
sudo chown -R root:wheel /opt/notchling && sudo chmod -R go-w /opt/notchling
```

`preflight.py` finds `guard` at `../guard` from its own folder, so keep `/opt/notchling/agents` and `/opt/notchling/guard` side by side. Update both from a merged `main` after a change, and never from an agent branch.

## 3. The App key and the wrapper's config

Create the GitHub App first (the maintainer does this in GitHub; see the spec's "GitHub setup": contents write, pull requests write, issues write, checks read; never `workflows`, never admin; installed on this repository only). Download its private key.

The **Secret Vault** on the maintainer's Mac keeps the master copy (name it `notchling-app-key`, with a note on how to renew it). The wrapper reads a file, so each worker gets one copy of it, written once by the maintainer over ssh stdin and never through the agent's account:

```sh
sudo install -d -o notchling-push -g notchling-push -m 0700 /var/lib/notchling-push     # macOS: /Users/notchling-push/keys
sudo -u notchling-push sh -c 'umask 077; cat > /var/lib/notchling-push/app.pem'
# paste the key (or pipe it: ssh worker 'sudo -u notchling-push sh -c "umask 077; cat > /var/lib/notchling-push/app.pem"' < key.pem), then Ctrl-D
sudo chmod 0600 /var/lib/notchling-push/app.pem
```

The wrapper refuses a key that isn't a regular file, isn't owned by its user, or is readable by group or others.

The config, `~notchling-push/.config/notchling/push-wrapper.json` (mode 0600):

```json
{"app_id": 123456, "key_path": "/var/lib/notchling-push/app.pem", "repo": "OWNER/REPO"}
```

`api_url` and `remote_url` default to GitHub's and are there for tests. The App key is never in CI, never in a repository secret and never in the agent's home.

## 4. The sudoers line

Let sd (user `sd`; use your own account name) run the wrapper as `notchling-push`, and nothing else. `visudo -f /etc/sudoers.d/notchling-push`:

```
sd ALL=(notchling-push) NOPASSWD: /usr/bin/python3 /opt/notchling/agents/push_wrapper.py *
```

sd then calls:

```sh
sudo -u notchling-push /usr/bin/python3 /opt/notchling/agents/push_wrapper.py \
  --worktree /Users/notchling-agent/work/issue-42 --issue 42 \
  --title "Fix the thing" --body-file /tmp/body-42.md --agent-model claude-opus-5-5
```

The `*` is safe only because the script validates every argument (branch name, issue number, title, model, body size). It prints the PR URL on exit 0; exit 1 is a refusal, exit 2 an error.

## 5. The pre-start check

Before sd starts an issue, it runs `preflight.py` as itself:

```sh
GITHUB_TOKEN="$(gh auth token)" python3 /opt/notchling/agents/preflight.py OWNER/REPO 42
```

Exit 0 means go. Exit 1 or 2 means do not start (2 is an API error, which counts as refused). It checks that a maintainer added `notchling:queued`, that it wasn't removed since, that the body wasn't edited after the label, and that the issue is open, isn't a pull request and isn't `notchling:stuck`.

The body-edit check uses GitHub's GraphQL `userContentEdits`, and GraphQL needs a token even on a public repository. The token only has to read issues. Use the maintainer's own `gh` login in sd's account, which the agent user can't see, or a fine-grained read-only token. Do not give it to the agent.

## 6. Network allowlist

The agent's builds and tests may reach package registries and nothing else. On the worker, block outbound traffic for `notchling-agent` except the registries you need (crates.io, static.crates.io, index.crates.io, pub.dev, Swift package hosts, your Cargo and pub mirrors). Do this in the firewall (macOS: `pf` with a `user` rule; Linux: `nftables` `meta skuid`), not in the agent's own environment. The agent must not reach `api.github.com` or `github.com`; only `notchling-push` and sd do. The model API is reached by sd's process, not by the agent user, unless the agent tool itself needs it (then allow only that host). Untried: write the actual rules when the first worker is set up and paste them here.

## 7. Smoke test

1. `preflight.py` on an issue the maintainer queued: exit 0. On the same issue after editing its body: exit 1.
2. As `notchling-agent`, make a branch `agent/<issue>-test` with one commit in a scratch clone. Try `git push`: it must fail.
3. Run the wrapper on it: a PR opens, authored by the App, and `agent/<issue>-test` exists on GitHub.
4. Run the wrapper on a branch called `main`, then on one with uncommitted files: both refuse with exit 1 before any network call.
5. `ps aux | grep -i x-access-token` while a push runs: nothing. The token is only in the environment of the one `git push`.

## 8. What changes in Machines later

Machines' "Push is refused" step (Swift, protected code, the maintainer's call) today checks that the worker account can't push with the old issues token, `gh` login, key, keychain password or `~/.git-credentials`. After this setup it becomes "Agent user can't push; wrapper can push only `agent/**`":

- The "can't push" half runs as `notchling-agent`, and is the same probe as today but for that user.
- The "wrapper can push only `agent/**`" half is the rulesets' job on GitHub's side (`agent/**` only the App; `main` no direct pushes), plus the wrapper's own refusal of any other ref.
- The personal token `~/.config/agents/github-token` (Issues write) is retired once the App exists, and its Machines row goes with it.

Until then that step stays as it is.
