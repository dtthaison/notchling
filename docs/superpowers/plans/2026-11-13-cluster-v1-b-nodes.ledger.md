# SDD ledger — plan: docs/superpowers/plans/2026-11-13-cluster-v1-b-nodes.md

machines: lane "machines", branch agent/cluster-v1-b-machines, base origin/agent-run-oss 70cda366. Task 15 only.
machines: Ruling R1: Task 15 consumes only Tasks 10/11 (cluster.status, cluster.join.token; CLI --version, cluster status --json, cluster join --token-stdin, service install), all merged; nothing from Tasks 12-14 (daemon lane) is needed.
machines: Ruling R2: the Linux unit is notchlingd.service (service.rs installs that), not dev.notchling.daemon as the plan's text says; probes use the installed names — costs a string if wrong.
machines: Ruling R3: the service Fix runs a fixed load command Notchling composes (service::load_command's text), never a command printed by the remote machine — the safe side; linger is enabled by an admin Fix on Linux.
machines: Ruling R4: dist file notchling-<os>-<arch>, os linux|macos, arch from uname -m with arm64→aarch64 (matches M2's notchling-linux-x86_64); hash computed on the Mac, checked on the machine before the move.
machines: Ruling R5: the join token goes only on stdin and into the Fix's masks; NODE_JOINED fails plainly when this Mac has no cluster.
machines: Task 15: dispatched (sonnet high).
machines: Task 15: implemented 72a26b34 (engine machines:: 633 pass incl. 18 node-step tests; app machines:: 71 pass incl. 4 node tests; clippy clean except a large_enum_variant on app Sheet that this change does not touch; fmt clean; guard lint 0 new; DONE_WITH_CONCERNS: tests were written after the code, not before)
machines: Correction: the clippy large_enum_variant on Sheet was introduced by Detected.node (Sheet::Add 544→568 bytes); origin/agent-run-oss is clippy-clean.
machines: Task 15 review (opus high): Needs fixes — I1 clippy large_enum_variant from Detected.node, I2 version-only binary check never sees an outdated build, I3 service Fix fails on a loaded service and never restarts after a new binary, I4 scp target breaks on IPv6 hosts; minors: placeholder Fix displays, duplicate cluster.status calls, name fallback when the node id is known.
machines: Ruling R6: Sheet::Add boxes AddForm — costs nothing if wrong.
machines: Ruling R7: the binary step compares the installed binary's SHA-256 with the Mac's dist file when one exists, else the version (workspace version is fixed at 0.1.0, so the version alone can't tell builds apart) — a stricter reading of "equals the Mac's build".
machines: Ruling R8 (extends R3): the service Fix enables, then kickstarts -k a loaded label or bootstraps an unloaded one (macOS), or daemon-reload, enable, restart (Linux); binary_fix restarts a loaded service after the move (Linux guarded by is-active --quiet instead of try-restart, which errors without a user bus on a machine whose service isn't set up yet). Still fixed text Notchling writes.
machines: Ruling R9: scp targets an IPv6 host as user@[host]:path.
machines: Note: the Linux service Fix always asks for admin, so a Linux machine set to Sudo is refused. That follows the existing Linux rule (root over SSH by default) and is left as is.
machines: Task 15 fix round 1: e02506ef (engine machines:: 646 pass incl. 31 node tests; app machines:: 72 pass; clippy -D warnings clean; fmt clean; guard lint 0 new).
machines: Task 15 re-review (opus high): Approved (646 engine, 72 app; clippy clean).
machines: Final review (fable high): Needs fixes — I1 safety pins skip node steps, I2 node steps gate the worker's Running switch, I3 fresh machine's log folder missing, I4 Mac with the app never passes the service step, I5 restart ends running work unannounced, I6 daemon down reads as no cluster; minors.
machines: Ruling R10: node-* steps are skipped by turn_on_blocker, so the Cluster Node switch never gates the worker's Running switch — costs a worker turned on while the node steps still fail, if wrong.
machines: Ruling R11: the service Fix makes the daemon's log folder first (Linux $XDG_DATA_HOME/notchling when absolute, else ~/.local/share/notchling; macOS ~/Library/Application Support/Notchling) with fixed text derived from the daemon's data_dir — costs a changed daemon default leaving the folder missing again, if wrong. notchlingd is untouched.
machines: Ruling R12: on macOS the service check runs the read-only `notchling status` and passes whenever a daemon answers, whoever started it, with no Fix offered then. Whether a Mac that runs the app should skip the BINARY and SERVICE steps entirely is for the maintainer — costs a Mac showing a step it never needs, if left.
machines: Ruling R13: the binary and service Fix sheets begin with "Work running on this machine stops while the service restarts." and the unit is unchanged — costs a restart that still ends running work, if wrong.
machines: Ruling R14: ClusterTokens::cluster_id returns Result<Option<String>, String> (a deviation from the plan's Option<String>); an Err reads as can't tell "Notchling isn't running on this Mac", never as no cluster. DaemonTokens caches the answer for 3 seconds — costs one stale answer for 3 seconds, if wrong.
machines: Note: a pending join can't be told apart on the remote status (`cluster` is null while joining), so it still reads as not in a cluster; behaviour kept.
machines: Note: turning Cluster Node off removes only the steps; the binary, service and join stay on the machine.
machines: Note for the maintainer: the Linux service Fix keeps its admin step; whether `loginctl enable-linger` works without sudo under the VPS's polkit is yours to check.
machines: Final fix round: 77a6764d (engine machines:: 658 pass; app machines:: 74 pass; clippy -D warnings clean; fmt clean; guard lint 0 new).
