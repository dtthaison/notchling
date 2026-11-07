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

The worktrees it works in are readable by `notchling-push` (the wrapper only reads them): mode 0750 on the worktree root, with `notchling-push` in a shared group, or 0755. The wrapper also reads the worktree's index and the clone's objects, so the clone's `.git` folder (which a linked worktree shares) must be readable by `notchling-push` too.

The agent's clone must hold its own objects. Make it with a plain `git clone` (or `git worktree add` from such a clone), never `--shared` or `--reference`: git follows `objects/info/alternates` to any folder, so the wrapper refuses a repository that has one (or `http-alternates`), and one whose `objects` folder is a symlink.

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

Install the scripts where neither the agent nor sd can change them (on Linux, use group `root` wherever this says `wheel`):

```sh
sudo install -d -o root -g wheel -m 0755 /opt/notchling/agents
sudo cp tools/agents/preflight.py tools/agents/push_wrapper.py /opt/notchling/agents/
sudo install -d -o root -g wheel -m 0755 /opt/notchling/guard
sudo cp -R tools/guard/guard /opt/notchling/guard/guard             # preflight imports MAINTAINERS from it
sudo chown -R root:wheel /opt/notchling && sudo chmod -R go-w /opt/notchling
```

`push_wrapper.py` imports `preflight.py` from its own folder, and `preflight.py` finds `guard` at `../guard` from that folder, so keep the two scripts together and `/opt/notchling/agents` and `/opt/notchling/guard` side by side. Update both from a merged `main` after a change, and never from an agent branch.

The wrapper needs Python 3.9 or newer, git 2.31 or newer (it uses `rev-parse --path-format`) and `openssl`. It never searches the caller's `PATH`: it takes `git` and `openssl` from the first of `/usr/bin`, `/bin`, `/usr/local/bin`, `/opt/homebrew/bin` that has them, by absolute path, and says "not found" (exit 2) if none does. On macOS `/usr/bin/python3` and `/usr/bin/git` are Command Line Tools stubs; check that they run as `notchling-push` (`sudo -u notchling-push /usr/bin/python3 -I -c pass`). If they don't, use a root-owned Python and git in one of those folders and change the sudoers line to match.

## 3. The App key and the wrapper's config

Create the GitHub App first (the maintainer does this in GitHub; see the spec's "GitHub setup": contents write, pull requests write, issues write, checks read; never `workflows`, never admin; installed on this repository only). Download its private key.

The **Secret Vault** on the maintainer's Mac keeps the master copy (name it `notchling-app-key`, with a note on how to renew it). The wrapper reads a file, so each worker gets one copy of it, written once by the maintainer over ssh stdin and never through the agent's account:

```sh
sudo install -d -o notchling-push -g notchling-push -m 0700 /var/lib/notchling-push     # macOS: -g staff, and /Users/notchling-push/keys
sudo -u notchling-push sh -c 'umask 077; cat > /var/lib/notchling-push/app.pem'
# paste the key (or pipe it: ssh worker 'sudo -u notchling-push sh -c "umask 077; cat > /var/lib/notchling-push/app.pem"' < key.pem), then Ctrl-D
sudo chmod 0600 /var/lib/notchling-push/app.pem
```

The wrapper refuses a key that isn't a regular file (a symlink is refused too), isn't owned by the user running it, or gives group or others any access (0600 and 0400 pass).

The config, `.config/notchling/push-wrapper.json` in `notchling-push`'s home (mode 0600, owned by `notchling-push`):

```json
{"app_id": 123456, "key_path": "/var/lib/notchling-push/app.pem", "repo": "OWNER/REPO"}
```

`key_path` is used as written (`~` isn't expanded), so give the full path. The App key is never in CI, never in a repository secret and never in the agent's home.

Where the config is read from: `--config FILE` if given, else `.config/notchling/push-wrapper.json` in the home folder the account database lists for the user running the wrapper (not `$HOME`, which sudo may leave as the caller's, and not an environment variable). The file must be a plain file (a symlink or a second hard link is refused), owned by the user running the wrapper, and writable by neither group nor others (0600, 0640 and 0644 pass, 0660 and 0666 don't). So sd can't aim `--config` at a file of its own. sd passes no `--config`.

`api_url` and `remote_url` are not for production: the wrapper refuses any `api_url` but `https://api.github.com` and any `remote_url` but `https://github.com/OWNER/REPO.git` (or without `.git`), because the App's JWT and token are sent there. Leave them out. Only the wrapper's own tests set `NOTCHLING_PUSH_WRAPPER_TESTING=1` to point them at a local fake; the script scrubs its environment when it starts, so a command line can't turn that on.

## 4. The sudoers line

Let sd (user `sd`; use your own account name) run the wrapper as `notchling-push`, and nothing else. `visudo -f /etc/sudoers.d/notchling-push`:

```
Defaults:sd env_reset, !setenv
sd ALL=(notchling-push) NOPASSWD: /usr/bin/python3 -I /opt/notchling/agents/push_wrapper.py *
```

`env_reset` hands the wrapper a clean environment and `!setenv` stops sd from adding variables on the command line. `-I` is Python's isolated mode (no `PYTHON*` variables, no user site folder); it is part of the line, so sd can't drop it. The script path is pinned too, and it sits in a folder neither sd nor the agent can write.

sd then calls it with the body on stdin:

```sh
sudo -u notchling-push /usr/bin/python3 -I /opt/notchling/agents/push_wrapper.py \
  --worktree /Users/notchling-agent/work/issue-42 --issue 42 \
  --title "Fix the thing" --agent-model claude-opus-5-5 < "$BODY"
```

There is no `--config` (the default path in step 3 is used) and no body file option: a path would be opened with `notchling-push`'s rights, and an agent could swap the file at it for a link to the App key. On Linux the config is under `/home/notchling-push`.

What sd must do for `$BODY`: write it itself, from the agent's final message that sd captured, into a folder only sd can write (for example mode 0700 under sd's home). Never point `<` at a path inside the agent's worktree, `/tmp` or any other place the agent can write: sd's shell would follow a link there with sd's rights, and sd's own files would end up in the pull request.

The `*` lets the caller pass any arguments, and sudoers can't narrow them further, so the wrapper treats every one as untrusted:

- The worktree's branch (`agent/<issue>-<slug>`, matching `--issue`), a clean tree, a repository with no alternates, and no change under `.github/workflows/` (the App has no `workflows` permission; when the agent's clone has `origin/HEAD`, `origin/main`, `origin/master`, `main` or `master` to compare with, the wrapper says so and exits 1 before it mints a token; if not, GitHub refuses the push). A push the remote rejects is also exit 1, with the remote's reason.
- The title (one plain line), the model id, and the body (UTF-8 text, at most 60,000 bytes). A body with `PRIVATE KEY-----` or the installation token is refused. `@name` mentions and `closes/fixes/resolves #N` references to other issues are broken with a zero-width joiner, except a reference to the PR's own issue. The wrapper appends `Refs #<issue>` and, with `--agent-model`, an `Agent-Model:` line.
- Which `git` and `openssl` run (fixed folders, absolute paths), the working directory (its own scratch folder), the environment (rebuilt), the config file (owned by `notchling-push`) and the URLs (GitHub's), as described above.
- The installation token is revoked when the wrapper is done, best effort, so a leaked one is not good for an hour.

What this does not stop: anything running as `sd` can still ask the wrapper to push a clean `agent/<n>-<slug>` branch from any repository `notchling-push` can read, and to open a PR for it. The rulesets (only the App writes `agent/**`, `main` takes no direct pushes) and the maintainer's merge decide what that is worth. It also cannot tell whether a title or body is honest, only make it inert.

It prints the PR URL on exit 0; exit 1 is a refusal, exit 2 an error (including "the branch is pushed, but the pull request failed").

## 5. The pre-start check

Before sd starts an issue, it runs `preflight.py` as itself:

```sh
GITHUB_TOKEN="$(gh auth token)" python3 /opt/notchling/agents/preflight.py OWNER/REPO 42
```

Exit 0 means go. Exit 1 or 2 means do not start (2 is an API or usage error, which counts as refused). It checks that the latest `notchling:queued` label event was a maintainer's (`MAINTAINERS` in `guard/gate.py`), that the label wasn't removed since, that the issue wasn't edited at or after that event, and that the issue is open, isn't a pull request and isn't `notchling:stuck`. It also refuses an issue whose edit history is longer than it reads (100 edits).

A renamed title after the label refuses too (the `renamed` timeline event and GraphQL `lastEditedAt`).

On exit 0, stdout is one JSON line: `ok`, `repo`, `issue`, `title`, `body`, `title_sha256`, `body_sha256` and `sha256` (of title, a newline, then body, as UTF-8). Stdout is empty on exit 1 or 2, and stderr never holds issue text. The worker hands the agent exactly that `title` and `body`, as data, and never re-reads the issue or passes its comments on. A worker that must read the issue itself reads it first and passes its own hash with `--expect-sha256 <hex>`; a different text refuses with exit 1. Parse the line as JSON and never paste a field into a shell.

The body-edit check uses GitHub's GraphQL `userContentEdits`, and GraphQL needs a token even on a public repository. The token only has to read issues. Use the maintainer's own `gh` login in sd's account, which the agent user can't see, or a fine-grained read-only token. Do not give it to the agent.

## 6. Network allowlist

The agent's builds and tests may reach package registries and nothing else. On the worker, block outbound traffic for `notchling-agent` except the registries you need (crates.io, static.crates.io, index.crates.io, pub.dev, Swift package hosts, your Cargo and pub mirrors). Do this in the firewall (macOS: `pf` with a `user` rule; Linux: `nftables` `meta skuid`), not in the agent's own environment. The agent must not reach `api.github.com` or `github.com`; only `notchling-push` and sd do. The model API is reached by sd's process, not by the agent user, unless the agent tool itself needs it (then allow only that host). Untried: write the actual rules when the first worker is set up and paste them here.

## 7. Smoke test

1. `preflight.py` on an issue the maintainer queued: exit 0. On the same issue after editing its body: exit 1.
2. As `notchling-agent`, make a branch `agent/<issue>-test` with one commit in a scratch clone. Try `git push`: it must fail.
3. Run the wrapper on it: a PR opens, authored by the App, and `agent/<issue>-test` exists on GitHub. Once the rulesets are applied, this needs the App in the `agent-branches` ruleset's bypass list.
4. Run the wrapper on a branch called `main`, then on one with uncommitted files: both refuse with exit 1 before any network call.
   Then on a branch that adds `.github/workflows/x.yml`: exit 1, naming the missing `workflows` permission, with no token minted. And with a body that holds `-----BEGIN PRIVATE KEY-----`: exit 1, nothing sent.
5. `ps auxww | grep -E 'ghs_|x-access-token'` while a push runs: nothing. The token is in no argument list; it reaches git only through the environment of the one `git push` (`GIT_CONFIG_*`).

## 8. What changes in Machines later

Machines' "Push is refused" step (Swift, protected code, the maintainer's call) today checks that the worker account can't push with the old issues token, `gh` login, key, keychain password or `~/.git-credentials`. After this setup it becomes "Agent user can't push; wrapper can push only `agent/**`":

- The "can't push" half runs as `notchling-agent`, and is the same probe as today but for that user.
- The "wrapper can push only `agent/**`" half is the rulesets' job on GitHub's side (`agent/**` only the App; `main` no direct pushes), plus the wrapper's own refusal of any other ref.
- The personal token `~/.config/agents/github-token` (Issues write) is retired once the App exists, and its Machines row goes with it.

Until then that step stays as it is.
