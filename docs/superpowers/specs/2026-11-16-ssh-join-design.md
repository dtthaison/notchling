# Add Machine over SSH: a machine joins the cluster from its address

- **Date:** 2026-11-16
- **Status:** Design, waiting for the maintainer. Nothing here is built. The questions only the maintainer can answer are at the end, in [Needs the maintainer](#needs-the-maintainer), each with a recommendation.
- **Source:** the maintainer's words: "it should install Notchling on this machine, I just need to add the SSH address". Amended the same day by the maintainer: machines added over SSH get **no app**, on Macs too. They run only the headless `notchling` daemon as a service, and everything about them is seen and controlled from the Primary's app (the Cluster sheet and Machines).
- **Base:** `origin/agent-run-oss` 0a32aaca. The build starts after lane E wave 3 (`agent/cluster-v1-e-wave3`, E Tasks 10, 12, 13, 14) has merged, since both change the same files ([19](#19-order-with-other-work)).
- **Touches:** decisions 0004 (Machines is a separate tool, not in the daemon), 0007 (tiers: Machines fixes are app tier), 0010 (releases), 0011 (one owner per resource), `XXXX-cluster-v1-nodes` (joins need the maintainer's Add), the [cluster design](2026-11-13-notchling-cluster-design.md), the [cluster v1 overview](../plans/2026-11-13-cluster-v1-overview.md) (the manual steps M1 to M15) and the [version sync design](2026-11-13-version-sync-design.md) §16 (Notchling on workers). It needs a decision record of its own (plan Task 1).
- **Gaps** the source left open are settled here and marked **Builder's call**, collected in [Builder's calls](#builders-calls).

## In short

1. **One entry point.** The Cluster sheet gets **Add Machine…**: type `user@host` (or pick a machine Machines already knows, like m1 or vps-sing-1, and press **Join Cluster**). Nothing else to type.
2. **Check first.** Notchling checks the machine over ssh, read-only: system and chip, Tailscale and its tailnet, an existing `notchling` and its build, Claude Code, Codex and Antigravity, disk encryption, and on a Mac its sleep settings. Each line is a Machines step: a read-only Check plus a Fix.
3. **One plan, one click.** The sheet lists what will run, with the exact commands behind **Show Commands**, the admin password field when a step needs it, and two choices (Can Be Master, Acting). One **Install and Join** click runs it. Nothing runs before that click.
4. **The daemon only.** A Mac gets this Mac's own `notchling` binary, copied and checked by SHA-256, in a bundle-shaped folder, run by a **LaunchDaemon**. A Linux machine gets a prebuilt static `notchling` for its chip, run by a **systemd user unit with linger** (as today). No app, no GPUI, no pty host.
5. **Joining without a second click.** The app asks its daemon for an **invite**: a join token bound to the new machine's node id, which the app read over the same ssh. The token goes over ssh stdin to `notchling cluster join --token-stdin`. The primary adds that one node without the Inbox's Add, because the person started it there; any other node with that token is refused. Only the app can make an invite; an agent's token still needs Add.
6. **Seen at once.** The new member shows in the Cluster sheet as it joins, online, with its role.
7. **Updates.** When this Mac's Notchling changes, the Cluster sheet shows **Update N Machines** for members whose build differs, and runs the same copy-check-restart step on each, one at a time.
8. **Builds for Linux.** No Linux build exists in any release or CI job today. v1 adds `tools/build-node.sh` (static musl builds for x86_64 and aarch64, into the cluster dist folder with a manifest) and a CI job that keeps the musl build compiling. Signed release assets (version sync §16) replace it later.
9. **What stays manual.** Signing in to Tailscale, Codex and Antigravity, making a Linux account, trusting a new ssh host key and turning on disk encryption stay how-tos. Notchling never renews a token, never touches Keychain on either machine, and sends secrets only over ssh stdin.

## Builder's calls

| # | Call | Section |
| --- | --- | --- |
| B1 | The add flow runs in the app's process (the Cluster sheet) on the engine's Machines library. Not in the `notchling-machines` binary (agent tier, can't make invites) and not in the daemon (0004) | [4](#4-approaches) |
| B2 | A machine added here is an ordinary registry machine with `node` set and no jobs: a **node-only** machine. Its catalog drops the sd worker's steps (Base tools, Xcode, the sd Claude login) and adds a small **Git** step | [8](#8-the-steps) |
| B3 | On a Mac the binary lives at `~/.local/share/notchling/Notchling.app/Contents/MacOS/notchling`, with `~/.local/bin/notchling` a symlink to it. The bundle shape is required: on macOS a client trusts a daemon only when its executable is inside `*.app/Contents` (`local-api/src/spawn.rs::allowed_binary`, checked by `client.rs::server_trusted`), so today's Mac path `~/.local/bin/notchling` would be refused by its own `notchling status` | [9.2](#92-where-it-lives) |
| B4 | A Mac's build is this Mac's own bundled `notchling` when the chips match; otherwise the dist file `notchling-macos-<arch>` | [9.1](#91-which-build) |
| B5 | A Mac's service is a LaunchDaemon `dev.notchling.node` (system domain, `UserName` = the node user), installed once with the admin password. It replaces the LaunchAgent the code writes today. Maintainer to confirm ([Needs the maintainer](#needs-the-maintainer) 2) | [9.3](#93-the-service) |
| B6 | `cluster.invite {node, name, canBeMaster?, acting?}` is a new app-tier op. A client's call is forwarded to the primary at App origin, allowed for `CLIENT` only (a client or the home, the rule `cluster.record` got in e1da98aa) | [10](#10-joining-without-a-second-click) |
| B7 | An invite is one use, 10 minutes, at most 4 open, and bound to one node id. Used by any other node, it is refused and spent | [10](#10-joining-without-a-second-click) |
| B8 | The join step re-reads the machine's node id in the same Fix, right before it asks for the invite, rather than trusting the last refresh | [10](#10-joining-without-a-second-click) |
| B9 | A build id: the commit, embedded by the build scripts as `NOTCHLING_BUILD`, printed by `notchling --version` and reported as `Versions.build` (additive). A dev build without it compares by SHA-256, as the binary step does today | [12](#12-updates) |
| B10 | Update offers are computed from `cluster.status` each time, not stored: a member whose `versions.build` differs from this Mac's | [12](#12-updates) |
| B11 | **Reaches the primary** is `notchling cluster reach <addr> <port>`: a plain TCP connect with a 3 s limit, no TLS, run on the new machine | [8](#8-the-steps) |
| B12 | The Tailscale step also checks the machine is in the same tailnet as this Mac (`CurrentTailnet.Name`), when this Mac's can be read | [8](#8-the-steps) |
| B13 | The node account is the address's user. `root@…` is refused before any ssh. On Linux the account is never an admin (today's rule); admin steps use root over ssh | [7.2](#72-the-address) |
| B14 | Notchling never accepts a new ssh host key. `Host key verification failed` stops at a how-to: run `ssh user@host` once in Terminal | [7.3](#73-checking) |
| B15 | A run keeps going when the sheet closes; the Cluster sheet shows "Adding m1… (step 4 of 9)" and reopening shows it. Quitting the app ends its ssh processes (the runner's process groups); every step checks first, so the next run picks up where it stopped | [7.5](#75-running) |
| B16 | The member gets the plan's Can Be Master and Acting choices from the invite; Bridge and Install stay off | [7.4](#74-the-plan) |
| B17 | On a Mac that has `/Applications/Notchling.app`, the app may stay. The service is installed anyway; the join needs the app closed there (the daemon refuses an agent's join while an app is attached). When the app opens later it uses the service's daemon | [9.4](#94-a-mac-that-has-the-app) |
| B18 | An update restart never kills work: Linux keeps `KillMode=process`; the Mac plist sets `AbandonProcessGroup`. A machine with running shell jobs or a move in progress is skipped, with Update Anyway on its row | [12](#12-updates) |

## Contents

1. [Why](#1-why)
2. [Goals, non-goals, anti-goals](#2-goals-non-goals-anti-goals)
3. [Constraints](#3-constraints)
4. [Approaches](#4-approaches)
5. [What exists today](#5-what-exists-today)
6. [Terms](#6-terms)
7. [The flow](#7-the-flow)
8. [The steps](#8-the-steps)
9. [Installing Notchling](#9-installing-notchling)
10. [Joining without a second click](#10-joining-without-a-second-click)
11. [Machines already set up](#11-machines-already-set-up)
12. [Updates](#12-updates)
13. [Builds for other machines](#13-builds-for-other-machines)
14. [Ops, events, words](#14-ops-events-words)
15. [Failure cases](#15-failure-cases)
16. [Tests, rule by rule](#16-tests-rule-by-rule)
17. [Risks](#17-risks)
18. [Where the code goes](#18-where-the-code-goes)
19. [Order with other work](#19-order-with-other-work)
20. [Needs the maintainer](#needs-the-maintainer)

## 1. Why

Today a machine reaches the cluster in two separate places, with many hand steps between them:

- **Machines** (`crates/app/src/machines/`, the `notchling-machines` binary) adds a machine by Name, Host and User, and for sd's overnight worker runs its Checks and Fixes. Its **Cluster Node** switch adds four node steps (Tailscale, Notchling installed, Notchling service, Joined). But Machines is a separate binary at the agent tier, not in the shipped bundle, and runs ssh only with `--live`.
- **The Cluster sheet** (Chat > Cluster) has Create Cluster, Copy Join Token and Join. A join from another machine needs Add in the Inbox on the primary.

Between them sit the overview plan's manual steps: M1 Tailscale, M2 a Linux build copied by hand into the dist folder, M3 the node user and linger, M4 Claude Code on Linux, M7 the worker logins, M11 a Mac Standby's prerequisites, M13 installs on each Mac, M14 disk encryption. A new machine takes an hour of terminal work and two windows. The maintainer wants to type an address.

## 2. Goals, non-goals, anti-goals

**Goals**

- G1. From the Cluster sheet, an SSH address is all the maintainer types to add a Mac or a Linux machine to the cluster.
- G2. Before anything runs, the sheet shows what is there and what will change, with the exact commands, in the Machines style (Check, then a Fix only on a click).
- G3. Notchling installs only its daemon on the machine and runs it as a service that survives logout and restart, with no app.
- G4. The machine joins without a second approval, when and only when the person started the add on a Mac with the app in the cluster.
- G5. Machines already in the registry (m1, vps-sing-1) join with one click.
- G6. When this Mac's Notchling changes, the other machines can be brought to the same build with one click.
- G7. Every manual step of M1 to M15 that can be done safely over ssh is a Fix; the rest are Checks with plain how-tos.

**Non-goals**

- Windows machines.
- Installing the app, a GUI, or the pty host on a machine (the daemon doesn't use the pty host today; `crates/ptyhost/AGENTS.md`: never installed until its handover).
- Signing in to any tool, or to Tailscale, on the machine's behalf (except the optional Linux Tailscale sign-in link, [Needs the maintainer](#needs-the-maintainer) 5).
- Making the machine's account, its ssh keys, or accepting its host key.
- Turning on disk encryption.
- Leaving another cluster, or uninstalling Notchling from a machine (the commands go in `docs/notes/cluster.md` as a how-to).
- Ssh ports and options other than what `~/.ssh/config` already says for the host.
- Moving the whole Machines window into the app (its own handover, later).
- Signed release downloads on the machine (version sync §16, later).

**Anti-goals** (things this must never do)

- Run anything on a machine before the person pressed the plan's one Run button.
- Let an agent start an install, make an invite, or approve a join without the Inbox.
- Put a token, password or other secret on a command line, in an environment that outlives its call, in a log, the Audit, the Activity or a Fix log.
- Accept a new ssh host key, or open a port on a machine to the internet.
- Renew any token, or read or write Keychain, on this Mac or the machine.
- Turn on a daemon switch from a file, or ship a service file that does.
- Kill a running session or shell job on a machine to install or update.
- Run Notchling as root, or make the Linux node account an admin.

## 3. Constraints

- **Tiers (0007).** Machines fixes are app tier. Only the app may start an install; no local API op runs ssh. The `notchling-machines` binary is the agent tier (`crates/app/AGENTS.md`, `cluster_feed.rs::DaemonTokens`), so it can't make invites.
- **Switches.** Every switch is off at every daemon start. A node joined through `notchling cluster join` with no app attached is headless: the join turns its `cluster` switch on (audited, `cluster_join`) and so does each restart (`cluster_wiring.rs::rejoin_on_start`); `acting`, `bridge` and `install` come only from the primary's stored choice (`node.switch.set`). Nothing here changes that.
- **Local API additive.** New op and fields only; `cluster.join.token` and `cluster.join` keep their shapes and tiers.
- **AGENTS.md hard rules.** Never refresh tokens; Keychain only through `/usr/bin/security` (this design touches none); secrets only on ssh stdin into 0600 files or a process's stdin; no secrets in logs; tests never ssh, never `gh`, never the real `~/.claude`.
- **Machines rules** (`crates/engine/AGENTS.md`): plan, confirm, run with displays compared (`engine_plan.rs::SetupEngine::run`); one ssh per refresh for cheap probes; anything a machine reports is untrusted (`shell::is_safe_machine_path`); admin scripts use `/usr/bin/sudo -S` under `env -i` with a fixed PATH; on Linux the worker account is never the admin.
- **Cluster rules.** Tailscale addresses only (`cluster-net/src/listen.rs`); node ids are the first 16 hex of the certificate's SHA-256, made once per data folder (`identity.rs::load_or_make`); the node tier is the closed table `cluster::policy::RULES`.
- **Files.** Under 1,000 lines (600 warns). `app/src/cluster/view.rs` is 744 and `model.rs` 666: new code goes in new files. No comments in code; the why goes in the crates' `AGENTS.md`.
- **Lane E wave 3** edits `wire.rs`, `cluster_ops.rs`, `inbox/model.rs`, `bridge_backend.rs`, `app/src/cluster/model.rs`, `view.rs`, `renders.rs`, `docs/local-api-ops.md`, the notes, `feature-map.md` and `CHANGELOG.md`. This build waits for it ([19](#19-order-with-other-work)).

## 4. Approaches

**A. In the app, on the Machines library (chosen).** The Cluster sheet hosts the flow; the engine's `machines` module (registry, steps, `SetupEngine`, `ProcessRunner`) runs in the app's process, as ARCHITECTURE.md already lists Machines among the app's parts. The app is app tier, so it can make invites. The registry is shared, so the machine shows in Machines too. Cost: the shipped app runs ssh for the first time ([Needs the maintainer](#needs-the-maintainer) 3).

**B. In the `notchling-machines` binary.** The Cluster sheet would open Machines at an Add sheet. Machines is agent tier, so every join would still need Add in the Inbox, and the binary isn't in the bundle. Giving it the app tier changes 0007's allowlist.

**C. In the daemon.** A `machines.add` op on the daemon would put ssh, scp and the admin password in the always-on process and next to the vault, against 0004 ("Machines stays a separate tool, not inside the daemon").

**D. Let the machine download its own build.** `curl` a release on the machine. No Linux or daemon release asset exists ([13](#13-builds-for-other-machines)); this is version sync §16's later path, which this design leaves room for.

A keeps every Machines rule, gives the person one window, and is the only one that can imply the approval without widening a tier.

## 5. What exists today

| Part | Where | What this design does with it |
| --- | --- | --- |
| Machine registry, `node: Option<NodeSettings>` | `engine/src/machines/registry.rs` | Reused; a node-only machine has `node` set and no jobs |
| Machine steps: Reachable (os, arch, home), Worker user, Stays awake (pmset, admin Fix), FileVault (read-only), Base tools, Claude Code (install script Fix), Xcode | `steps_machine.rs` | Reused; Base tools and Xcode left out for node-only machines |
| Node steps: Tailscale (how-to), Notchling installed (scp, hash, move, restart), Notchling service (systemd user + linger; Mac LaunchAgent), Joined (token on stdin) | `steps_node.rs`, `steps_node_binary.rs`, `steps_node_service.rs` | Reused on Linux; the Mac binary path and service change; Joined uses an invite when it can |
| Codex and Antigravity signed in (read-only, how-to) | `steps_node_logins.rs` | Reused |
| Can Be Master checks after the join | `steps_master.rs` | Reused |
| Tool Versions, Self-Update Off | `steps_versions.rs`, `steps_selfupdate.rs` | Reused |
| "A Mac with the app: binary and service Not needed; join is a how-to" | `steps_node_binary.rs::parse_binary`, `steps_node.rs::joined` | Replaced: a Mac node always runs the service ([9.4](#94-a-mac-that-has-the-app)) |
| `ClusterTokens` (join token, cluster id, Can Be Master) | `step.rs`, app `machines/cluster_feed.rs::DaemonTokens` | Gains `invite`; in the app the same code is app tier |
| `notchling service install` (LaunchAgent, systemd unit, Task Scheduler) | `notchlingd/src/service.rs` | Unchanged; still writes the Linux unit on the machine. A Mac node's LaunchDaemon plist is rendered on this Mac (`service_files.rs`), since root writes it |
| `notchling cluster status|token|join --token-stdin` | `notchlingd/src/cluster_cli.rs` | Gains `reach` |
| Join tokens, one use, 10 minutes | `engine/src/cluster/join.rs::Tokens`, `core/join.rs::on_join` | Gains invites |
| Join approval: Inbox item `node-join:<node>:<ms>`, Add or Deny | `notchlingd/src/cluster_join.rs` | Unchanged for tokens; an invite adds a notice instead |
| Cluster sheet: Create, Copy Join Token, Join, member rows and menus | `app/src/cluster/` | Gains Add Machine…, Not in the Cluster, Update N Machines |
| Dist folder `<data>/cluster/dist/notchling-<os>-<arch>` | `steps_node.rs::dist_dir` | Gains `manifest.json` from `tools/build-node.sh` |
| Release workflows | `.github/workflows/beta.yml`, `release.yml` | Build the Swift app only; nothing for Linux or the daemon |

## 6. Terms

- **Node-only machine**: a registry machine with `node` set and no sd worker job.
- **Node user**: the account `notchling` runs as on the machine, the user in the address.
- **Invite**: a join token the app asked for, bound to one node id, which the primary adds without the Inbox's Add.
- **Primary**: as in the cluster plans; in code `home`.
- **Build id**: the commit a `notchling` was built from, `None` for a dev build.
- **Dist folder**: `<data>/cluster/dist/` on this Mac, the builds for other machines.

## 7. The flow

### 7.1 Where it starts

- The Cluster sheet's header gets **Add Machine…** beside Copy Join Token, shown while this Mac is in a cluster. With no cluster it says "Make a cluster first" over the Create form; with the `cluster` switch off it shows Turn On first.
- A section **Not in the Cluster** lists registry machines whose Joined step isn't a pass, each with name, host, system and **Join Cluster…** (m1 and vps-sing-1 today). [11](#11-machines-already-set-up).
- In the Machines window, each card gets **Join Cluster** (turns on Cluster Node and plans Set Up for the node steps; since Machines is agent tier, its join still waits for Add in the Inbox; the card says so).

### 7.2 The address

One field, **SSH Address**, placeholder `user@host`. Below it, **Name**, filled from the host's first label (lowercase, digits and hyphens; `m1.tail1234.ts.net` gives `m1`) and editable. A bare `host` takes this Mac's user name and shows it. The existing `forms::machine_problems` rules apply (name, host, user alphabets; unique name); `root` is refused ("Notchling runs as its own account; give that account, for example agent@vps-sing-1"). **Check** is the only button.

### 7.3 Checking

**Check** runs one batched probe over ssh (the existing Reachable script and the node probes, read-only, BatchMode, 8 s connect timeout) and shows each step as it lands: ✓, ✗ with the reason, or Can't Tell. Only once Reachable passes is the machine saved to the registry with `node` set (Machines' rule: Add only after a Check of the same name, host and user), so Machines sees it too.

Stops before a plan:

- **Can't reach it**: ssh's own first line ("Permission denied (publickey)", "Could not resolve hostname"). How-to: add this Mac's key with `ssh-copy-id user@host` in Terminal, then Check again.
- **Host key not known**: "Host key verification failed". How-to: `ssh user@host` once in Terminal and compare the key (*B14*).
- **Unsupported system**: not macOS 13 or newer, or not Linux on x86_64 or aarch64.
- **In another cluster**: Joined says so; leave it there first.

### 7.4 The plan

The sheet lists every step that isn't ✓, in order, each with its title, why it's needed in one line and **Show Commands** (the Fix's display, as Machines shows it). Steps that are only how-tos (Tailscale on a Mac, logins) show their how-to and don't block the Run unless the join needs them (Tailscale does). Under the list:

- **Admin password** (secure field) when a step needs admin on a Mac; on Linux, the root-over-ssh warning Machines shows today.
- **Can Be Master** (Mac only): on when FileVault is on and the user and home match the primary's; else off with the reason ("FileVault is off"). Linux: off, with "Needs an encrypted disk; most VPSes can't" (M14).
- **Let it run sessions and jobs** (Acting): on (*B16*; [Needs the maintainer](#needs-the-maintainer) 6).
- The line "It joins this cluster without asking again, because you started it here."

One button, **Install and Join**. It is the plan's single confirm (`SetupEngine::confirm` once per PlanId); the run rebuilds the plan from the machine as it is and refuses if any display changed, as Machines does.

### 7.5 Running

Steps run one at a time, each re-checked after its Fix. The sheet streams the masked Fix log under the running step. The run stops at the first step that still fails, with its reason and **Check Again** (which re-plans). The sheet can close; the run goes on and the Cluster sheet shows its row (*B15*).

### 7.6 Joining

The last Fix is the join ([10](#10-joining-without-a-second-click)). The sheet then waits (up to 60 s) for `cluster.status` to list the node as active and online.

### 7.7 Done

The sheet closes onto the Cluster sheet with the new row: name, Worker or Standby, Online, its tools and load. The Machines registry entry stays, and its card shows the node steps as ✓.

## 8. The steps

For a node-only machine, in order. "New" marks what this design adds; the rest exist.

| Step | Check (read-only) | Fix | Admin | Mac | Linux | Automates |
| --- | --- | --- | --- | --- | --- | --- |
| Reachable | os, arch, home, version, disk (`steps_machine.rs`) | how-to | | ✓ | ✓ | |
| Worker user | not root; on Linux not in a sudo group | how-to | | ✓ | ✓ | M3 (check) |
| Stays awake | `pmset -g`: sleep 0, disksleep 0, autorestart 1 | `pmset -a …` | ✓ | ✓ | | M11 |
| FileVault | `fdesetup status` | how-to only | | ✓ | | M14 (check) |
| Git (new) | `git --version` | Linux: `apt-get install -y git` or `dnf install -y git`; Mac: how-to (`xcode-select --install`) | Linux | ✓ | ✓ | |
| Tailscale connected | running, a 100.x address, and (new) the same tailnet as this Mac (*B12*) | how-to; optional Linux Fixes ([Needs the maintainer](#needs-the-maintainer) 5) | | ✓ | ✓ | M1 |
| Notchling installed | the binary's build and SHA-256 against this Mac's build for that system and chip | copy with `scp -O`, check the SHA-256 on the machine, move into place, restart a running service | | ✓ (changed, [9](#9-installing-notchling)) | ✓ | M2, M13 |
| Notchling service | Mac (new): the LaunchDaemon is loaded and `notchling status` answers; Linux: unit active, `KillMode=process`, linger on | Mac: write the plist as root, bootstrap or kickstart; Linux: `service install`, enable linger, load | ✓ | ✓ (changed) | ✓ | M3 linger |
| Reaches the primary (new) | `notchling cluster reach <primary 100.x> 47823` (*B11*) | how-to: the tailnet's ACL must allow `tcp:47823` between your machines | | ✓ | ✓ | M1 ACL |
| Joined the cluster | `cluster status --json`: in this cluster | the invite join ([10](#10-joining-without-a-second-click)) | | ✓ | ✓ | |
| Can Be Master checks | after the join, from `cluster.status` (`steps_master.rs`) | how-tos | | ✓ | ✓ | M11, M14 |
| Claude Code | `claude --version` | Claude Code's install script | | ✓ | ✓ | M4 (install; the proof stays M4) |
| Codex, Antigravity signed in | read-only (`steps_node_logins.rs`) | how-to | | ✓ | ✓ | M7 (check) |
| Tool Versions, Self-Update Off | as today | as today | | ✓ | ✓ | |

Left out for a node-only machine: Base tools (python3, jq, gh are sd's), Xcode, and the sd Claude login (`steps_secret::claude_login`): a cluster job gets `CLAUDE_CODE_OAUTH_TOKEN` per job from the vault node (plan C), nothing on disk (*B2*). A machine that also has an sd job keeps every step it has today.

## 9. Installing Notchling

### 9.1 Which build

- **Mac, same chip as this Mac** (all Apple Silicon today): this Mac's own `notchling` from the running app's bundle (`Contents/MacOS/notchling`, beside `notchling-app`). It is signed with the identifier `dev.notchling.daemon` like every bundle build, and scp sets no quarantine flag, so it runs (*B4*).
- **Mac, another chip**: the dist file `notchling-macos-x86_64`; missing, the step says "Build Notchling for Intel Macs first: tools/build-node.sh --mac-intel".
- **Linux**: the dist file `notchling-linux-x86_64` or `notchling-linux-aarch64`, a static musl build ([13](#13-builds-for-other-machines)); missing or from another commit than this Mac's, the step says "Build the Linux nodes first: tools/build-node.sh".

The check compares SHA-256: the installed binary's against the chosen source's, as `parse_binary` does now (arm64 is aarch64; the arch word is checked before it enters a path).

### 9.2 Where it lives

- **Linux:** `~/.local/bin/notchling`, as today.
- **Mac:** `~/.local/share/notchling/Notchling.app/Contents/MacOS/notchling`, and `~/.local/bin/notchling` a symlink to it for the command line (*B3*). The folder is bundle-shaped only so that `server_trusted` accepts the daemon: on macOS a client refuses a daemon whose executable isn't `notchling` inside `*.app/Contents` or a cargo target. No Info.plist and no app: launching it from Finder does nothing useful, and the dot folder keeps it out of Spotlight. The copy lands as `notchling.new` beside it and is moved only after the hash matches, as today.

The service file points at the real path, never the symlink. When version sync §16 lands, `versions/<v>/` folders and a moved symlink fit under the same folder.

### 9.3 The service

**Linux** (exists): `~/.config/systemd/user/notchlingd.service` from `notchling service install`, `KillMode=process`, `Restart=on-failure`; linger enabled as root over ssh; `daemon-reload`, `enable`, `restart`. Probes set `XDG_RUNTIME_DIR`.

**Mac** (new, *B5*): a LaunchDaemon, `/Library/LaunchDaemons/dev.notchling.node.plist`, root-owned, mode 644:

- `Label` `dev.notchling.node`; `ProgramArguments` `[<real path>, "daemon"]`; `UserName` the node user; `EnvironmentVariables` `HOME` (the probed home, safe-path checked) and `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin` plus the user's `~/.local/bin` and `/opt/homebrew/bin`, so `claude`, `codex` and `git` are found); `RunAtLoad`; `KeepAlive {SuccessfulExit: false}` and `ThrottleInterval` 30, so a daemon that exits because the data folder is locked by another one tries again; `AbandonProcessGroup` true, so a restart leaves shell jobs running, like Linux's `KillMode=process` (*B18*); `ProcessType` Background; logs to `~/Library/Application Support/Notchling/notchlingd.log`, made by the node user first so launchd doesn't create it as root.
- Rendered on this Mac by `engine/src/machines/service_files.rs`, which already renders the sd worker's plist and checks every user and home value before it enters a unit (XML-escaped, comments stripped); sent inside the admin script (`/usr/bin/sudo -S`, the password the first stdin line, `env -i`, fixed PATH), written with `install -m 644 -o root -g wheel`, then `launchctl enable system/dev.notchling.node` and `bootstrap system <plist>`, or `kickstart -k` when it is loaded.
- An old per-login `dev.notchling.daemon` LaunchAgent from today's code is booted out and its file removed in the same Fix, shown in the sheet.

Why a LaunchDaemon: a Standby or Worker Mac often sits with nobody logged in. A LaunchAgent runs only while the node user is logged in at the screen: after a power cut with `autorestart 1` the Mac boots to the login window, after `fdesetup authrestart` too, and after a logout the agent stops. A LaunchDaemon runs in all three. What it costs is in [17](#17-risks): no login Keychain (agy on a Mac keeps its login there), no privacy prompts (a repository under Desktop, Documents or Downloads is refused silently), and one admin password at install. With FileVault on, nothing runs after a cold boot until someone unlocks the disk, whichever form is used.

### 9.4 A Mac that has the app

Today a Mac with `/Applications/Notchling.app` passes the binary and service steps as "Not needed" and its join is a how-to. After this design (*B17*):

- The binary and service steps run as for any Mac node. The app is never removed or changed.
- A new check before the join, **Notchling app closed** (only when the probe printed `app=yes`): `pgrep -x notchling-app`. While it runs, the join fails (the daemon refuses an agent's join while an app is attached); the how-to says to quit it on that Mac.
- Only one daemon runs per data folder (`notchlingd.lock`). If the app started its own daemon first, the service's daemon exits and launchd retries every 30 s until it gets the lock; the step's Fix restarts it after the app's daemon is gone. When the app opens later it finds the service's daemon answering and uses it.

## 10. Joining without a second click

### 10.1 What happens

1. The join Fix reads the machine's node id over ssh: `notchling cluster status --json`, field `me` (16 hex characters; anything else is refused). The status answers with the `cluster` switch off (*B8*).
2. The app calls `cluster.invite {node, name, canBeMaster, acting}` on its own daemon. That daemon, or the primary it forwards to, records an invite and answers `{token}`.
3. The Fix sends the token as the one line of stdin to `notchling cluster join --token-stdin` over ssh (the existing `join_fix`: never on a command line, masked in a copy of the masks, never streamed to the Fix log).
4. The new node dials the primary over Tailscale with the token, as any join does. With no app attached there, it joins headless and rejoins by itself after a restart.
5. On the primary, `on_join` takes the token. It is an invite and the node's id is the invite's id: the primary proposes and approves the member in one step, with the invite's name, Can Be Master and Acting, and raises an Inbox notice "m1 joined the cluster. Added over SSH from m5." (no actions). The node gets `JoinOk` at once.
6. Any other node presenting that token: refused "this token was made for another machine", and the token is spent (*B7*).

### 10.2 Why this is safe enough (recommended; [Needs the maintainer](#needs-the-maintainer) 1)

The Add click exists because a join token can be made at the agent tier: Machines and the command line make them, so an agent with a shell could hand one to a machine it controls. An invite is different on each point that click protects:

- **Who asks.** Only the app tier can make one (`OpSpec::APP`). On a client, the call is forwarded to the primary at App origin and allowed for `CLIENT` only, so a Standby without an app can't make one (the hole the e3 ledger notes for `cluster.join.token` stays out of this op).
- **Who it lets in.** Exactly one node id, which the app read from the machine the person typed, over ssh that checked the host key. A leaked invite admits nobody else. Taking over that node id means holding that machine's certificate key, and then the machine is already the attacker's, which the person would have approved by hand too.
- **When.** One use, 10 minutes, at most 4 open per maker, in the primary's memory only (lost on a restart, which is the safe side).
- **What the person saw.** The plan sheet said in words that it joins without asking again, and the Run click is theirs.
- **Records.** A `cluster.step` event `invited` when made and `invite_joined` when used (node id, maker; never the token), and the Inbox notice, so a join that wasn't expected is visible.

The ordinary token and its Add stay exactly as they are, for the command line, the Copy Join Token button and the Machines window.

### 10.3 What doesn't change

- The node's switches: `cluster` on through the headless join (audited, as today); `acting` from the invite's `desired`, applied by `node.switch.set` from the primary; `bridge` and `install` off.
- `cluster.join`, its `--token-stdin` rule and its refusals.
- The join window on the primary's listener (`Pins::set_join_open`): an open invite keeps it open like an open token.

## 11. Machines already set up

m1 (user `sondoan`) and vps-sing-1 (user `agent`) are in the registry for sd's worker. **Join Cluster…** in the Not in the Cluster section:

- turns on `node` for that machine (the Machines Cluster Node switch, saved under the registry lock),
- runs Check for the node steps only (the sd steps' results are kept as they are),
- shows the same plan sheet and runs it.

The sd worker's steps, files and service are not touched. The node runs as the registry's user: `agent` on vps-sing-1 ([Needs the maintainer](#needs-the-maintainer) 7). On m1 the LaunchDaemon runs as `sondoan`.

In the Machines window, **Join Cluster** on the card does the same through Machines' own sheet. Machines is agent tier, so its join uses an ordinary token and waits for Add in the Inbox; the card says "Add it in the Inbox on the primary".

## 12. Updates

- **The build id** (*B9*). `tools/bundle-next.sh` and `tools/build-node.sh` set `NOTCHLING_BUILD` to the short commit (with `-dirty` when the tree has changes); `notchling` and `notchling-app` read it with `option_env!`. `notchling --version` prints `notchling 0.1.0 (0a32aaca)`. `Versions` gains `build: Option<String>` (`#[serde(default)]`, left out when unknown), so each member reports it in `cluster.status`. The dist folder's `manifest.json` names the build its files came from.
- **The offer** (*B10*). The Cluster sheet compares each active member's `versions.build` with this Mac's own. When any differ it shows "2 machines run another Notchling build: m1, vps-sing-1" with **Update 2 Machines…**. A member with no build id (a dev build) is compared by the binary step's hash instead, once Machines has checked it. Nothing is stored: the offer is there while the builds differ, and **Not Now** hides it until the next build. The Machines window shows the same list through its existing banner.
- **The run.** One sheet lists the machines and the binary step's commands for each; one Run click; then each machine in turn: copy, hash check, move, restart. A Linux machine whose dist build isn't this Mac's build is skipped with "Build the Linux nodes first". A machine with running shell jobs (`cluster.status` `jobs`) or while a move runs is skipped with **Update Anyway** on its row (*B18*). The first failure stops the run; the others keep their build.
- **Restart, not stop.** Claude Code's background sessions are their own processes and keep running through a daemon restart. Shell jobs keep running (Linux `KillMode=process`, Mac `AbandonProcessGroup`), but the restarted daemon no longer lists them (its job table is memory, as today), which is why such machines are skipped.

## 13. Builds for other machines

**What exists.** Nothing builds `notchling` for another machine. `tools/release/` (`appcast.py`, `version.py`, `fragments.py`) serves the Swift app's update feed; `beta.yml` and `release.yml` run `./build.sh` on macOS and publish `Notchling-<v>.dmg` and `.zip`; `rust.yml` tests on macOS, Linux and Windows but uploads nothing. The overview's M2 has the maintainer build on the VPS with rustup and copy the file into the dist folder by hand. Version sync §16 plans per-target release assets (`notchling-<v>-<target>.tar.gz`, `notchling-manifest.json` signed with the Sparkle ed25519 key) and is not built; it waits for the Developer ID.

**What v1 adds** (*recommended*, [Needs the maintainer](#needs-the-maintainer) 4):

- `tools/build-node.sh [--mac-intel]`: builds `-p notchling-notchlingd --bin notchling` with `cargo zigbuild --release --locked` for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` (static: no glibc version to match on the VPS), and with `--mac-intel` for `x86_64-apple-darwin`; sets `NOTCHLING_BUILD`; copies the results to `<data>/cluster/dist/notchling-linux-x86_64`, `notchling-linux-aarch64` (and `notchling-macos-x86_64`); writes `dist/manifest.json` `{build, files: {name: {sha256, size}}}`. The dependencies suit musl: rustls with ring, rusqlite with bundled SQLite, no OpenSSL. It respects `NOTCHLING_DATA_ROOT`.
- `tools/bundle-next.sh --nodes` runs it after the bundle, so an install leaves matching node builds.
- A `rust.yml` job `node-builds` on ubuntu: builds the two musl targets with `--locked`, no secrets, no upload. It keeps them compiling.
- Later, version sync §16's signed assets: the binary step downloads on the machine from the URL and SHA-256 this Mac sends, and the dist folder becomes a cache.

## 14. Ops, events, words

**Local API** (additive):

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `cluster.invite` | app | `{node, name, canBeMaster?, acting?}`; `node` 16 hex, `name` the machine's name (Machines' alphabet) | `{token: "nlj1.…", expiresMs}`; refused with no cluster, on a non-primary that can't reach the primary, or when 4 invites by this maker are open (the oldest is dropped instead, like tokens) |

- `Versions.build` (additive, in `cluster.status` members and the node handshake).
- `cluster.status` gains `invites: [{node, name, expiresMs}]` on the primary (never a token), so the app can show "Waiting for m1 to join".
- Node tier: `cluster.invite` from a client at App origin to the primary, `CLIENT` only; never relayed twice.
- The `notchling` command line gains `notchling cluster reach <addr> <port>`: exit 0 when a TCP connect succeeds within 3 s, else 1 with one line; the address must be a Tailscale IPv4 address.

**Events.** `cluster.step` gains `invited` and `invite_joined` (node and maker as `peer`; no token). The golden and `docs/log-kinds.md` change.

**Inbox.** A `notice` "m1 joined the cluster" with detail "Added over SSH from m5 · macOS 26.1 arm64 · <node id>" and no actions.

**Words.** `locales/en/mac/cluster.ftl`: Add Machine…, SSH Address, Name, Check, Install and Join, Show Commands, Join Cluster…, Not in the Cluster, Update N Machines…, Update Anyway, Not Now, and the step lines. Machines' step titles stay in the engine as today.

## 15. Failure cases

| Case | What the person sees | What is left on the machine |
| --- | --- | --- |
| No ssh key for the user | Reachable ✗ "Permission denied (publickey)", how-to `ssh-copy-id` | nothing |
| Host key unknown | "Host key verification failed", how-to | nothing |
| `root@` typed | refused before any ssh | nothing |
| macOS older than 13, or another chip or system | Reachable ✗ with the reason | nothing |
| Tailscale not installed, stopped, or another tailnet | Tailscale ✗; Install and Join is off until it passes | nothing |
| No dist build for that machine | Notchling installed ✗ "Build the Linux nodes first: tools/build-node.sh" | nothing |
| Copy fails or the hash differs | ✗ "the copy doesn't match this Mac's build; it was removed" | nothing new (`notchling.new` removed) |
| Wrong admin password | the admin step ✗ with sudo's line (masked) | the steps before it; Check Again resumes |
| Linux account can use sudo | Worker user ✗, how-to | nothing |
| Service fails to start | Notchling service ✗ with the log's last line | binary and service file; the next Fix rewrites them |
| ACL blocks 47823 | Reaches the primary ✗, how-to for the ACL | binary and service, not joined |
| App open on a Mac node | Notchling app closed ✗ | binary and service, not joined |
| Invite expired before the join (a slow run) | the join Fix asks for a new invite once, then ✗ | not joined |
| Node id changed between read and join (another data folder) | ✗ "this token was made for another machine" | not joined; the invite is spent |
| Already in another cluster | Joined ✗ "in another cluster, <name>"; no Fix | nothing changed |
| Primary unreachable from a client Mac | `cluster.invite` fails with the forward's reason | not joined |
| The app quits mid-run | the next open shows the machine's steps as they are | whatever finished; each step re-checks |
| Update while shell jobs run | skipped, Update Anyway | old build |

## 16. Tests, rule by rule

Tests never run ssh, scp, `gh`, `launchctl` or `systemctl`: `ScriptedRunner` and the Machines fakes, the core's simulator and test transports, `NOTCHLING_CLUSTER_LOOPBACK` e2e daemons over copied fixtures.

- **Nothing before the click.** Check and Refresh record no Fix call on `ScriptedRunner`; Install and Join with a changed display refuses.
- **Only the app invites.** An agent-tier `cluster.invite` is `forbidden`; a Standby without an app forwarding it at App origin is refused by `policy::check`; the `notchling-machines` path (agent tier) falls back to a token and the Inbox item appears.
- **Bound to one node.** Core: an invite used by its node approves without a `JoinRequest` effect; used by another node is refused and spent; expired is refused; a restart forgets it. Sim: no member becomes active without an Add or a matching invite (new invariant check).
- **Secrets.** The join token is only in `Call::stdin`, never argv, env or a Fix log (planted value); the admin password likewise; `cluster.status` `invites` never holds a token; events carry none.
- **Mac layout.** The plan writes to the bundle path; `allowed_binary` accepts the real path and refuses `~/.local/bin/notchling` (a test that fails on today's Mac path).
- **LaunchDaemon plist.** Golden render; a home or user outside the safe alphabet is refused before render; `AbandonProcessGroup`, `UserName`, `HOME`, no switch argument.
- **Service.** Mac Fix boots out an old LaunchAgent, writes as root, bootstraps or kickstarts; Linux unchanged tests still pass.
- **Builds.** Mac source is the app's sibling `notchling` for the same chip; a dist file for another chip; the manifest's build must equal this Mac's for Linux.
- **App closed.** `app=yes` and a running `notchling-app` fails the check before the join.
- **Reach.** `cluster reach` refuses a non-Tailscale address; connect and timeout over a local listener (`127.0.0.1` under the loopback flag).
- **Updates.** Members with another build are offered; a dev build compares by hash; machines with running shell jobs are skipped; the first failure stops the run.
- **App model.** Address parsing and name suggestion; the plan's choices (Can Be Master default by FileVault and home); the run's states; the Not in the Cluster list from the registry and `cluster.status`; renders `cluster-add`, `cluster-add-plan`, `cluster-add-running`, `cluster-not-joined`, `cluster-update`.
- **E2e.** Two daemons over loopback: an invite joins without an Inbox item and the member is active; a token without an invite still makes the item.

## 17. Risks

- **Claude Code under a LaunchDaemon on macOS is unproven.** With the per-job `CLAUDE_CODE_OAUTH_TOKEN` it shouldn't need the Keychain, but `claude --bg` outside a login session has never been run. The live check proves it (plan Task 17); if it fails, the LaunchAgent form is the fallback ([Needs the maintainer](#needs-the-maintainer) 2).
- **Antigravity on a Mac node** keeps its login in the Keychain, which a LaunchDaemon can't open while nobody is logged in. Its login step already says Can't Tell on a Mac; dispatching agy there may fail until someone logs in.
- **Privacy prompts** can't be answered on a headless Mac: work under Desktop, Documents or Downloads is refused. The plan says so.
- **musl builds** are new: a dependency that misbehaves on musl shows first in the CI job.
- **A stale dist folder** gives Linux machines an older build than the Mac; the manifest's build id catches it.
- **The shipped app now runs ssh.** The engine's runner is the same Machines has used for months, and every Fix still shows its commands first.
- **The invite trusts the machine's own report of its node id.** A machine that lies can only join as some other id it holds the key for; the person chose that machine.
- **Two daemons on a Mac with the app** fight for the data folder's lock; the loser retries every 30 s. Harmless, but the Activity shows the refusals.

## 18. Where the code goes

New:

- `crates/engine/src/machines/steps_node_mac.rs` (the Mac binary source, bundle path, LaunchDaemon service step, app-closed check), `steps_node_reach.rs` (Reaches the primary), `steps_git.rs`, `address.rs` (`user@host` and the name), `node_updates.rs` (which machines to update), `dist_manifest.rs` (reads the dist folder's `manifest.json`), and their `tests_*.rs`; `engine/src/cluster/build_id.rs` (the build id)
- `crates/engine/src/cluster/invite.rs` (`Invite`, `Invites`) and `invite_tests.rs`; `core/core_tests_invite.rs`; a sim invariant
- `crates/notchlingd/src/cluster_invite.rs` (`cluster.invite`, the notice) and tests; `cluster_reach.rs` (the CLI) and tests
- `crates/app/src/cluster/add.rs` (pure flow model), `add_parts.rs` (the engine's parts in the app, live), `add_runs.rs` (runs that outlive the sheet), `add_view.rs`, `not_joined.rs`, `not_joined_view.rs`, `updates.rs`, `updates_view.rs`, and their tests
- `tools/build-node.sh`, `docs/decisions/XXXX-ssh-join.md`, `changes/ssh-join.md`

Changed: `engine/src/machines/catalog.rs` (node-only profile), `steps_node.rs` (join via invite, `me` always detected), `steps_node_binary.rs` (sources, Mac path), `steps_node_service.rs` (Mac form moves out), `step.rs` (`ClusterTokens::invite`, `StepContext.mac_daemon`), `registry.rs` (nothing new in the file format); `engine/src/cluster/join.rs` (tokens carry an invite), `core/join.rs` (`on_join`), `core/mod.rs` (`Effect::InviteJoined`), `versions.rs` (`build`), `policy.rs` (rule); `crates/events/src/kinds/security.rs` (two `ClusterStep` values); `crates/local-api/src/wire.rs` (`CLUSTER_INVITE`); `engine/src/machines/service_files.rs` (the node LaunchDaemon plist); `notchlingd/src/cluster_cli.rs` (`reach`), `cluster_ops.rs`, `cluster_driver.rs`, `cluster_versions.rs`, `cli.rs` (version line), `op_kinds.rs`, `daemon_wiring.rs`; `app/src/cluster/view.rs` (two buttons and the sections, nothing else), `wire.rs`, `machines/cluster_feed.rs` (`invite`), `machines/cards.rs` (Join Cluster); `tools/bundle-next.sh` (`NOTCHLING_BUILD`, `--nodes`); `.github/workflows/rust.yml` (`node-builds`); `locales/en/mac/cluster.ftl`; `docs/local-api-ops.md`, `docs/notes/cluster.md`, the crates' `AGENTS.md`, `docs/feature-map.md`, `docs/log-kinds.md`, the overview plan's M table, `CHANGELOG.md`.

## 19. Order with other work

Lane E wave 3 (`agent/cluster-v1-e-wave3`, base 0a32aaca) is building E Tasks 10, 12, 13 and 14 and changes `crates/local-api/src/wire.rs`, `crates/notchlingd/src/cluster_ops.rs`, `crates/engine/src/inbox/model.rs`, `crates/notchlingd/src/bridge_backend.rs`, `crates/engine/src/cluster/policy.rs`, `crates/app/src/cluster/model.rs`, `view.rs`, `renders.rs`, `docs/local-api-ops.md`, `crates/AGENTS.md` and the crate notes, `docs/feature-map.md` and `CHANGELOG.md`. This build branches from `origin/agent-run-oss` after E wave 3 has merged. Tasks that touch none of those files (the build scripts, `service.rs`, the Machines engine steps, the address parser) may start earlier if the maintainer wants; the plan marks them.

## Needs the maintainer

1. **Imply the approval for a machine added over SSH?** *Recommend yes,* through an invite: app tier only, bound to the node id read over the same ssh, one use, 10 minutes, recorded as an event and an Inbox notice ([10](#10-joining-without-a-second-click)). Ordinary tokens keep Add. The alternative is to keep Add and show the pending item inside the add sheet (one more click, no new op).
2. **Mac service: LaunchDaemon or LaunchAgent?** *Recommend a LaunchDaemon* running as the node user: it runs after a power cut, an authenticated restart and a logout, which a LaunchAgent doesn't; it costs one admin password, no login Keychain and no privacy prompts ([9.3](#93-the-service), [17](#17-risks)). The live check proves `claude --bg` under it before Mac Workers rely on it; if that fails, fall back to the LaunchAgent.
3. **Let the shipped app run Machines fixes over ssh,** for node steps only? *Recommend yes:* it is what 0007 and ARCHITECTURE.md already say (Machines fixes are app tier), it is the only way to imply the approval without widening a tier, and every Fix still shows its commands first. The full Machines window stays the dev binary until its own handover.
4. **Linux builds: how?** *Recommend* `tools/build-node.sh` with cargo-zigbuild for static musl targets on this Mac, plus a CI job that only checks they compile; needs `brew install zig` and `cargo install cargo-zigbuild --locked` once, and the musl targets added with rustup. Alternatives: build on the VPS (M2 as today), or publish CI artifacts now (outward-facing; version sync §16 does it properly later).
5. **Automate Tailscale on Linux?** *Recommend yes, as two optional Fixes:* Install Tailscale (as root, Tailscale's own install script, fetched over HTTPS and shown in the sheet) and Sign In to Tailscale (runs `tailscale up` and shows the sign-in link in the sheet only, never in a log, for you to open on this Mac). No auth keys. Mac stays a how-to (the Tailscale app needs a person at the screen to allow its network extension).
6. **Acting on for a machine added this way?** *Recommend on* by default, as a visible choice in the plan: the point of adding a machine is to run work there. Bridge and Install stay off.
7. **The node account on vps-sing-1:** *Recommend* the existing `agent` (sd worker's account) in v1: it has Claude Code, git access and the worker logins already. A separate `notchling` account (the cluster design's call) keeps cluster jobs and the overnight worker apart but doubles every login; it can come later with a "make the account" Fix.
8. **Can Be Master default for an added Mac:** *Recommend on* only when FileVault is on and the user and home match the primary's (m1: `sondoan`), else off with the reason. Linux always off unless you turn it on.
9. **Decision record and zones.** Accept `XXXX-ssh-join` (number at merge). The Machines engine, cluster core, `policy.rs`, `wire.rs`, `local-api` and `notchlingd` changes are in your zone, and the policy tests gain the `cluster.invite` rule's cases.
