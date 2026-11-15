# SDD ledger — plan: docs/superpowers/plans/2026-11-13-cluster-v1-b-nodes.md

machines: lane "machines", branch agent/cluster-v1-b-machines, base origin/agent-run-oss 70cda366. Task 15 only.
machines: Ruling R1: Task 15 consumes only Tasks 10/11 (cluster.status, cluster.join.token; CLI --version, cluster status --json, cluster join --token-stdin, service install), all merged; nothing from Tasks 12-14 (daemon lane) is needed.
machines: Ruling R2: the Linux unit is notchlingd.service (service.rs installs that), not dev.notchling.daemon as the plan's text says; probes use the installed names — costs a string if wrong.
machines: Ruling R3: the service Fix runs a fixed load command Notchling composes (service::load_command's text), never a command printed by the remote machine — the safe side; linger is enabled by an admin Fix on Linux.
machines: Ruling R4: dist file notchling-<os>-<arch>, os linux|macos, arch from uname -m with arm64→aarch64 (matches M2's notchling-linux-x86_64); hash computed on the Mac, checked on the machine before the move.
machines: Ruling R5: the join token goes only on stdin and into the Fix's masks; NODE_JOINED fails plainly when this Mac has no cluster.
machines: Task 15: dispatched (sonnet high).
machines: Task 15: implemented 72a26b34 (engine machines:: 633 pass incl. 18 node-step tests; app machines:: 71 pass incl. 4 node tests; clippy clean except a large_enum_variant on app Sheet that this change does not touch; fmt clean; guard lint 0 new; DONE_WITH_CONCERNS: tests were written after the code, not before)
