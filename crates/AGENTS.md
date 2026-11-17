# The Rust app: notes for agents

The Cargo workspace in `crates/` is Notchling now: a GPUI app (`notchling-app`) over an always-on daemon (`notchling daemon`), both built on one engine library. Since 2026-11-11 it is `/Applications/Notchling.app` on the maintainer's Mac and owns every resource in decision 0011's table. The root [AGENTS.md](../AGENTS.md) has the layers, checks, the line and safety rules; [ARCHITECTURE.md](../ARCHITECTURE.md) and [docs/decisions/](../docs/decisions/) say why; [docs/feature-map.md](../docs/feature-map.md) names each feature's files and tests. The working notes (architecture in one pass, things that bit before) are one file per topic in [docs/notes/](../docs/notes/), listed below and in the root index; nothing loads them, so read the one for your change. Keep these notes true: when you change what a note describes, change the note, and when something bites you, add it under Things that bit before in that topic's note (or below, when it fits no topic).

## Crates

| Crate (package) | What it is | Builds | Used by |
| --- | --- | --- | --- |
| `protocol` (`notchling-protocol`) | The phone bridge's wire types, `src/generated.rs`, written by `python3 tools/protocol/gen.py` from `protocol/agentconsole.v1.schema.json`. Never edit it by hand (`GUARD-PROTOCOL-GEN`; skill `add-protocol-field`) | library | engine |
| `locale` (`notchling-locale`) | The words the Mac app shows: tables from `locales/` (`src/generated.rs`, written by `python3 tools/locale/gen.py`, never by hand), the language choice, plurals and numbers from ICU4X, the pseudo-language `en-XA` ([its notes](locale/AGENTS.md)) | library | app |
| `platform` (`notchling-platform`) | OS adapters behind `cfg`: data folders (`paths`), file watching (`watch`), keep awake, notifications, private writes (`files`), running apps (`apps`); a missing one returns `Error::Unsupported` | library | engine, local-api, notchlingd, app |
| `engine` (`notchling-engine`) | Everything with no UI, half the code ([docs/notes/overview.md](../docs/notes/overview.md)) | library | notchlingd, ptyhost, app |
| `cluster-net` (`notchling-cluster-net`) | Node links for the cluster: the node certificate, TLS 1.3 pinned by fingerprint on both ends, Tailscale addresses only, the join window, an in-memory transport for tests | library | notchlingd |
| `local-api` (`notchling-local-api`) | Daemon to client socket: framing, peer identity, tiers, the events for refusals and calls, `Client`, `ensure_daemon` (decision 0007) | library | notchlingd, app |
| `events` (`notchling-events`) | The log's registry: the envelope, the closed list of field types (no text), every kind with its texts, a test sink; the golden and `docs/log-kinds.md` come from it (decision XXXX) | library | none yet; engine, local-api, notchlingd, ptyhost and app from Phase 2 |
| `notchlingd` (`notchling-notchlingd`) | The daemon and the command line (decision 0004) | `notchling` | bundled beside the app |
| `ptyhost` (`notchling-ptyhost`) | Terminal host, with the `hostd.sock` contract of the retired Swift `agent-console-hostd` | `notchling-ptyhost` | bundled in `Contents/Helpers` |
| `kit` (`notchling-kit`) | Design kit: tokens (every system), GPUI widgets and `KitGallery` (macOS) | library | app |
| `app` (`notchling-app`) | The GPUI app, and the Machines window | `notchling-app`, `notchling-machines` | nothing |

The code has no comments, so each crate's `AGENTS.md` (`app`, `cluster-net`, `engine`, `events`, `notchlingd`, `kit`, `local-api`, `locale`, `platform`, `ptyhost`) holds what a change could break without knowing it, by file. The wire shapes of the daemon's ops are in [docs/local-api-ops.md](../docs/local-api-ops.md).

GPUI 0.2.2 (with `runtime_shaders`: no Metal Toolchain here) is a dependency of `kit` and `app` only under `[target.'cfg(target_os = "macos")'.dependencies]` (`LINT-GPUI-MACOS-ONLY`); everything else builds and is tested on macOS, Linux and Windows, and off macOS `notchling-app` says it isn't supported and exits 0. `[patch.crates-io]` swaps in `vendor/gpui/` (one fix, `vendor/gpui/NOTCHLING.md`). Rust 1.99.1 is pinned; `deny.toml` allows only permissive licenses (decision 0008). `app` and `kit` deny `unwrap`/`expect` outside tests. Files stay under 1,000 lines, 600 with a warning (`LINT-FILE-SIZE`): tests go in `<module>_tests.rs` or `tests_<area>.rs`.

## Build, run, test

```sh
cargo test --workspace --locked             # CI runs it on macOS, Linux and Windows
cargo clippy --workspace --all-targets -- -D warnings   # CI's clippy runs on Linux and never sees GPUI code: run it on the Mac
cargo fmt --all --check
cargo deny check
cargo test -p notchling-app -- --ignored    # the render tests: they open windows (a Mac with a display)
cargo build --release -p notchling-app -p notchling-notchlingd -p notchling-ptyhost
cargo run -p notchling-notchlingd --bin notchling -- daemon   # the `notchling` binary; set NOTCHLING_DATA_ROOT first (docs/notes/daemon.md)
```

## Notes by topic

- [overview.md](../docs/notes/overview.md): processes, the engine's modules, the app's areas, where a new op, setting, event or area goes
- [daemon.md](../docs/notes/daemon.md): the local API, tiers, the daemon's areas, the switches, the `notchling` command line
- [hook.md](../docs/notes/hook.md): `notchling hook` and the Artifact page hooks
- [rust-dev.md](../docs/notes/rust-dev.md): renders, Chat's benches, a dev app over a fixture, `tools/bundle-next.sh`
- [chat.md](../docs/notes/chat.md): Chat's controller and cached parts, selection and Copy, panel sizes, window frame, history budget
- [chat-details.md](../docs/notes/chat-details.md): the Details Panel's rows and the Changes panel
- [chat-speed.md](../docs/notes/chat-speed.md): native spinners, draw counts, the conversation list and scrolling
- [ui.md](../docs/notes/ui.md): words, the kit, Appearance, popup menus, the render map, GPUI's surprises
- [replies.md](../docs/notes/replies.md): typing into sessions: held, queued and failed replies, pictures
- [transcripts.md](../docs/notes/transcripts.md): tail budgets, queued entries, sessions continued under a new id
- [codex-antigravity.md](../docs/notes/codex-antigravity.md): the tool capability contract, Codex rollouts, Antigravity sessions
- [accounts.md](../docs/notes/accounts.md): account switching and the account log
- [data-and-logs.md](../docs/notes/data-and-logs.md): the data folders, every log, log events
- [inbox.md](../docs/notes/inbox.md): the Inbox, on the Mac and the phone
- [ask.md](../docs/notes/ask.md): `notchling ask`: who asked, who answers, waiting, restarts
- [versions.md](../docs/notes/versions.md): tool versions on workers
- [cluster.md](../docs/notes/cluster.md): nodes, standbys, forwarding, dispatch, the handover

## Testing

- Tests never touch the real `~/.claude`, `~/.codex`, `~/.gemini`, Keychain, ssh or `gh`: roots are parameters, and e2e tests set `NOTCHLING_DATA_ROOT` and a fake `HOME`. Fakes: `MemoryKeychain`, `PanicKeychain`, `ScriptedHttp` (`engine/src/accounts/fake.rs`), `FakeProcs`, `FakeApps` (`accounts/procs.rs`), `FakeHttp` (`providers/http.rs`), `ScriptedRunner` (`machines/runner.rs`), `FakeLauncher` (`control/launcher.rs`), `FakeSpawner` (`tools/codex/fake.rs`, `tools/acp/fake.rs`).
- Fixtures: `crates/app/fixtures/` (roots laid out as `claude/`, `notchling/`, `codex/`), `crates/engine/tests/fixtures/`, `crates/notchlingd/tests/fixtures/`, `tests/golden/` (`NOTCHLING_UPDATE_GOLDEN=1` rewrites; read the diff), `protocol/fixtures/`. The terminal recordings and transcripts the engine tests read are in `crates/engine/tests/fixtures/notchling/` and `console/` (`engine/tests/common/mod.rs`).
- Socket tests run a real server in a short `/tmp` folder (`server_tests.rs`, `session_source_tests.rs`, `switches/feed_tests.rs`); `crates/notchlingd/tests/*_e2e.rs` and `hook_cli.rs` run the built `notchling` over a copied fixture and stop every daemon they start in a guard. Scratch folders: `Scratch`, `TempDir` (`testutil.rs`).
- `crates/engine/tests/live_reply.rs` needs `NOTCHLING_LIVE=1` and a real job: the maintainer's, since each reply costs a turn.
- A fixture's mtime is the checkout's, so anything shown by file age (Codex rows) differs between machines. macOS caps socket paths at 104 bytes: keep test sockets in short `/tmp` folders. A Unix-only helper or test breaks the Windows leg without `#[cfg(unix)]`.

## Things that bit before

These fit no one area; the rest are in each topic's note.

- Locks held across blocking work hung things: ACP's writer lock blocked killing a stuck agy, ptyhost's delivery lock let one slow client stall all, the vault's cache lock waited on Keychain, a switch listener on a Keychain prompt held up `settings.set_switch` (now 500 ms at most). Never hold a lock across I/O, a pipe or a Keychain call.
- `&id[..8]` panics inside a multi-byte character, and ids from files hold anything. Cut by `chars()`.
- `write_private` follows symlinks so a lane's shared files stay shared; for a file Notchling owns use `write_private_no_follow`, which refuses a planted link.
- `Settings::set` refuses a key missing from `settings/keys.rs` ("unknown setting chatUsedSessions"). Add the key, or `set_unchecked` for the app's own. A new key also needs its class for agents, a title and a description (`.free`, `.ask` or `.never` on its line; `settings/agents_tests.rs` fails without them): `free` only for a preference that gives an agent nothing, `ask` for anything that changes what runs, hooks, shortcuts, other apps or model and delegation defaults, `never` for switches, logins, secrets, sandbox permissions and the app's own records.
