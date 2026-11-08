# Feature map

One row per feature area: where its code is, which tests cover it, whether a change there may merge on its own, and which parts of the port it becomes. Use it to find what to read before a change, and what to port for a part.

- **Folders** are under `legacy/macos/Sources/Notchling/` unless they start with another top folder. `CC/` is `legacy/macos/Sources/ConsoleCore/`.
- **Tests** are in `legacy/macos/Tests/NotchlingTests/`, with `CC:` for `legacy/macos/Tests/ConsoleCoreTests/` and phone tests in `apps/flutter/test/`. Fixtures are in each test folder's `Fixtures/`, except the protocol's, which are in `protocol/fixtures/`. Rust and Python tests are named by their path.
- **Zone** comes from [`tools/guard/zones.json`](../tools/guard/zones.json), which is the source of truth: `auto` may merge on its own once every check is green, `protected` waits for the maintainer, `mixed` names the files that are `auto`. A new test file is allowed in the zone; a change to an existing test is protected. Check one path with:

  ```sh
  PYTHONPATH=tools/guard python3 -c "from guard.zones import load; print(load('tools/guard/zones.json').in_zone('legacy/macos/Sources/Notchling/Search/SearchPanel.swift'))"
  ```

  A file in the zone still goes to the maintainer when it adds a process, pty, network, Keychain, file write or protected-type use (the capability check), or weakens a test.
- **Parts** are the port's part ids from the [design](superpowers/specs/2026-11-07-agent-run-open-source-design.md#parts): E (Rust engine), U (GPUI desktop app), P (phone). A part is done when its ported tests pass and its behavior fixtures match the Swift app.

Checked against the tree on 2026-11-08. When you add, move or delete a file, update its row.

## Mac app: reading

| Area | Folders and files | Tests | Zone | Parts |
| --- | --- | --- | --- | --- |
| Claude Code transcripts | `Agents/TranscriptReader.swift`, `Agents/TranscriptCursor.swift`, `Chat/ConversationReader.swift`, `Agents/Subagents.swift`, `Chat/BackgroundShells.swift`, `Chat/CommandJob.swift` | `TranscriptTests`, `SnapshotTests`, `TranscriptCursorsTests`, `QueuedMessageTests`, `SuccessorTests`, `SubagentsTests`, `BackgroundShellsTests`, `CommandJobTests`, `ModelConfirmationTests`, `ZBenchmarkTests` (opt-in); fixtures `conversation.jsonl`, `notch.jsonl`, `compact.jsonl`, `bash-mode.jsonl`, `limit-hit.jsonl`, `rate-limited-429.jsonl` | mixed: `TranscriptReader.swift`, `ConversationReader.swift` auto | E1 |
| Codex and Antigravity readers | `Agents/CodexSessions.swift`, `Agents/ACPTranscript.swift`, `Agents/AntigravityCLI.swift` | `CodexSessionsTests`, `ACPTranscriptTests`; fixtures `codex-rollout.jsonl`, `acp-agy.jsonl` | mixed: `CodexSessions.swift`, `ACPTranscript.swift` auto | E1 |
| Phone-side transcripts | `CC/ChatTranscript.swift`, `CC/SubagentReader.swift`, `CC/TranscriptFacts.swift`, `CC/TranscriptLocator.swift`, `CC/ChatImages.swift` | CC: `ChatTranscriptTests`, `ChatRichTests`, `ChatPagingTests`, `ChatImagesTests`, `TranscriptLocatorTests`; fixtures `chat/` | mixed: `ChatTranscript.swift`, `SubagentReader.swift`, `TranscriptFacts.swift` auto | E1 |
| Sessions and jobs | `Agents/AgentMonitor.swift`, `CC/SessionCatalog.swift`, `CC/SessionSource.swift`, `CC/SessionStore.swift`, `CC/Models.swift`, `CC/Support.swift`, `CC/PendingPromptReader.swift`; Rust: `crates/engine/src/sessions/` | `SessionIDTests`, `JobTranscriptTests`, `MovedJobTests`; CC: `CatalogTests`, `SessionSourceTests`, `SessionSwitchTests`; fixtures `claude/`, `notchling/` | protected | E2 |
| Search | `Search/SearchPanel.swift`, `Search/SessionIndex.swift`, `Search/SessionSearch.swift`; Rust: `crates/engine/src/search/` (matching, `index/` the session index), `crates/notchlingd/src/pages_ops.rs` (`search.query`) | `SessionSearchTests`; Rust: `search/tests.rs`, `search/index/tests.rs`, `pages_ops_tests.rs` | auto | E1 (matching), E7 (index), U3 |

## Mac app: acting

| Area | Folders and files | Tests | Zone | Parts |
| --- | --- | --- | --- | --- |
| Replies and question dialogs | `Agents/ReplySender.swift`, `Agents/ReplySender+Dialog.swift`, `Agents/AskDialog.swift`, `Agents/ShellCommand.swift`, `CC/JobReplySender.swift`, `CC/MessageSending.swift`, `CC/PTYSession.swift`, `CC/SpawnEnvironment.swift` | `AskDialogTests`, `ShellCommandTests`, `HostReplyTests`, `ZLiveReplyTests` (live, opt-in); CC: `InputRoutingTests`, `SpawnTests`; fixtures `terminal/` | protected | E3 |
| Terminal sheet | `Terminal/TerminalSession.swift`, `Terminal/TerminalScreen.swift`, `Terminal/TerminalView.swift` | `TerminalScreenTests`; fixtures `terminal/` | protected | E3, U1 |
| Session host (hostd) | `legacy/macos/Sources/hostd/main.swift`, `CC/Hostd/`, `tools/install-hostd.sh` | CC: `HostdTests` | protected | E3 |
| Hook | `Hook/HookBridge.swift`, `CC/HookResponder.swift` | `HookAndPagesTests`; CC: `HookResponderTests` | protected | E4, E13 |
| Session control | `Agents/AgentActions.swift`, `Chat/NewSessionSheet.swift`, `Chat/Model/ChatModel+Sessions.swift`, `Chat/ChatTakeover.swift`, `App/WakeResumer.swift`, `Chat/GitInfo.swift`, `CC/Takeover.swift`, `CC/StopOriginal.swift`, `CC/SessionPidMapper.swift` | `GitInfoTests`; CC: `TakeoverTests`, `StopOriginalTests`, `OwnershipTests`, `SendNowTests` | protected | E11, U7 (Trust Folder), U8 |
| Slash commands | `Agents/SlashCommands.swift`, `Agents/CodexCommands.swift` | `SlashCommandTests`; fixtures `installed_plugins.json`, `known_marketplaces.json` | protected | E11 |
| Codex and Antigravity runners | `Agents/CodexRunner.swift`, `Agents/CodexModels.swift`, `Agents/ACPRunner.swift`, `Agents/ACPConnection.swift`, `Chat/AntigravityChat.swift`, `Chat/Model/ChatModel+Codex.swift`, `Chat/Model/ChatModel+ACP.swift` | `CodexSessionsTests` (runner arguments), `NotchForeignTests` | protected | E6 |
| Delegate and Second Opinion | `Delegate/` (4 files), `Chat/Model/SecondOpinionStore.swift`, `Chat/Views/SecondOpinionViews.swift` | `DelegateTests`, `SecondOpinionTests` | mixed: `SecondOpinionViews.swift` auto | E6, E13 (`DelegateSkill`), U8 |
| Pages library | `Pages/PageLibrary.swift`, `Pages/PageSkill.swift`, `Pages/OwnerPublish.swift`, `Pages/PagesWindow.swift`; Rust: `crates/engine/src/pages/` (library, hook functions, owner publish), `crates/notchlingd/src/pages_ops.rs` (`pages.list`, `pages.find`), `pages_cli.rs` (`notchling pages`, `notchling page`) | `HookAndPagesTests`; Rust: `pages/tests*.rs` with `pages/fixtures/`, `notchlingd/tests/pages_e2e.rs` | mixed: `PagesWindow.swift` auto | E7 (owner publish protected), E13 (`PageSkill`), U4 |

## Mac app: accounts and secrets

| Area | Folders and files | Tests | Zone | Parts |
| --- | --- | --- | --- | --- |
| Accounts and providers | `Accounts/Accounts.swift`, `Accounts/AccountLogin.swift`, `Accounts/ClaudeDesktop.swift`, `Accounts/Codex.swift`, `Accounts/Antigravity.swift`, `Accounts/AccountsWindow.swift` (also the Settings page), `Accounts/UsageMenuViews.swift`, `Providers/` (6 files), `Chat/AddAccountSheet.swift` | `UsageParsingTests`, `ProviderTests`, `CodexLoginTests`, `AntigravityLoginTests`, `AccountRemoveRaceTests` | protected | E8, E14 (settings), U5, U8 |
| Limits | `Limits/LimitWatcher.swift` (also `NoticeBanner`), `Limits/ProviderLimits.swift` | `LimitLogicTests`, `ProviderLimitsTests` | protected | E8, U7 (banners), U9 |
| Cost | `Accounts/CostTracker.swift`, `Accounts/CostSources.swift` | `CostTrackerTests`, `CostSourcesTests`; fixture `antigravity-transcript.jsonl` | protected | E8, U9 |
| Lanes and session moves | `Lanes/Lanes.swift`, `Chat/MoveAll.swift`, `Chat/Model/ChatModel+Moves.swift`, `Chat/Model/ChatModel+Accounts.swift` | `MoveAllPlanTests`, `MovedJobTests` | protected | E8, E11 |
| Secret Vault | `Vault/SecretVault.swift` (also `VaultSkill`), `Vault/VaultStore.swift`, `Vault/VaultServer.swift`, `Vault/VaultCLI.swift`, `Vault/VaultPrompt.swift`, `Vault/VaultReminders.swift`, `Vault/VaultSheet.swift` | `SecretVaultTests`, `VaultStoreRenameTests`, `ReviewFixTests` | protected | E9, E13 (`VaultSkill`), E16 (CLI), E17 (`VaultServer`'s peer check is the local API's model), U5, U7 |
| Machines | `Machines/` (18 files) | `Machines*Tests` (16 files), `MachineSetupFakes.swift`, `MachineWindowFakes.swift`; fixtures `worker-status*.json`, `worker-templates/`, `simctl-ok.txt` | protected | E10, U5 |

## Mac app: windows and shell

| Area | Folders and files | Tests | Zone | Parts |
| --- | --- | --- | --- | --- |
| Chat window and model | `Chat/ChatWindowController.swift`, `Chat/Model/ChatModel.swift`, `Chat/Model/ChatModel+Drops.swift`, `Chat/Model/ChatModel+Panel.swift`, `Chat/Model/ChatModel+Sending.swift`, `Chat/Model/ChatModel+Turns.swift`, `Chat/ChatParts.swift`, `Chat/ChatMarkdown.swift`, `Chat/ChatPaths.swift`, `Chat/FilePanel.swift`, `Chat/MediaPreview.swift`, `Chat/JSONPretty.swift`, `Chat/SubagentViews.swift`, `Chat/ToolIcon.swift` | `ChatPathsTests`, `FilePreviewKindTests`, `JSONPrettyTests`, `ComposeAttachmentsTests` | protected | U1 |
| Chat views | `Chat/Views/` (17 files: sidebar, conversation, composer, cards, pickers, meters, shells, page panel) | `ReplyEditorTests` | auto | U1, U8 (pickers), U9 (`ChatSessionMeters`) |
| Notch views and pet | `Notch/NotchView.swift`, `Notch/NotchRows.swift`, `Notch/NotchPeek.swift`, `Notch/NotchDetail.swift`, `Notch/NotchCardParts.swift`, `Notch/TabPerch.swift`, `Notch/Pet/` | none; check with the `render-notch-view` skill | auto | U2 |
| Notch model, controller and cards | `Notch/NotchModel.swift`, `Notch/NotchController.swift`, `Notch/NotchForeign.swift`, `Notch/QuestionCard.swift`, `Notch/PermissionCard.swift`, `Notch/AgyPermissionCard.swift`, `Notch/PetLayer.swift` | `NotchForeignTests` | protected | U2 |
| Design kit | `DesignKit/ChatKit.swift`, `DesignKit/PointerCursors.swift` | `PointerCursorsTests` | auto | U0 |
| App shell, keep awake and system | `App/main.swift`, `App/AppDelegate.swift`, `App/AwakeController.swift`, `App/DockPresence.swift`, `App/PrivateFile.swift`, `App/TestRun.swift`, `System/FileEvents.swift`; Rust (E14): `crates/engine/src/settings/` | `ReviewFixTests` (`PrivateFile`) | protected | E12, E14 (`migrateFromAwake`), E16 (`main.swift` modes), U0, U11 |

## Mac app: phone bridge

| Area | Folders and files | Tests | Zone | Parts |
| --- | --- | --- | --- | --- |
| Phone bridge and pairing | `Phone/BridgeController.swift`, `Phone/BridgeIdentity.swift`, `Phone/BridgeServer.swift`, `Phone/TunnelOriginServer.swift`, `Phone/LiveBridgeBackend.swift`, `Phone/PhoneModel.swift`, `Phone/PhoneService.swift`, `Phone/ForeignSessions.swift`, `Phone/PhonePanel.swift`, `CC/Bridge/` (17 files); Rust (E5a): `crates/engine/src/bridge/` (router, hub, wire, `live.rs` backend, `foreign.rs`) | `ForeignSessionsTests`; CC: `BridgeRouterTests`, `BridgeSecurityTests`, `TunnelAccessTests`, `PhoneUploadTests`, `AppURLTests`, `ProtocolFixturesTests` (the bridge against `protocol/fixtures/`); Rust: `bridge/fixtures_tests.rs` (every fixture through the router and hub), `router_tests.rs`, `wire_tests.rs`, `hub_tests.rs`, `foreign_tests.rs`, `live_tests.rs` | protected | E5, U6 |
| Phone extras | `Phone/PhoneStreamer.swift`, `Phone/WebSnapshotter.swift`, `CC/VizEngine.swift`, `CC/VizSpec.swift`, `CC/VisualizeSuggester.swift`, `CC/WebURLResolver.swift`, `CC/DeviceWindowPicker.swift`, `CC/SchemaParsers.swift`, `CC/GitReader.swift`, `viz/` | CC: `VizEngineTests`, `VizBridgeTests`, `VisualizeSuggesterTests`, `SchemaParserTests` | protected | E15 |

## Phone app (`apps/flutter/`)

The phone stays Flutter; its parts are P0 to P3, and it isn't ported. Rules for the folder are in [`apps/flutter/AGENTS.md`](../apps/flutter/AGENTS.md).

| Area | Folders and files | Tests | Zone | Parts |
| --- | --- | --- | --- | --- |
| Phone shell, kit and strings | `apps/flutter/lib/main.dart`, `apps/flutter/lib/app/` (theme, widgets, alerts, notifications), `apps/flutter/lib/l10n/`, `apps/flutter/DESIGN.md` | `widgets_test`, `motion_test`, `background_alerts_test`, `message_alerts_test`, `keyboard_close_gap_test`, `goldens/kit_golden_test` | protected | P0, P3 |
| Phone protocol and network | `apps/flutter/lib/core/protocol/` (`models.dart`, hand-written, is what the app reads; `generated.dart` comes from `tools/protocol/gen.py`), `apps/flutter/lib/core/net/` | `bridge_client_test`, `tunnel_test`, `tunnel_socket_test`, `phase2_test`, `protocol_conformance_test` (every fixture through both) | protected | P1, E5 |
| Phone pairing and Macs | `apps/flutter/lib/core/pairing/`, `apps/flutter/lib/features/pairing/`, `apps/flutter/lib/features/macs/`, `apps/flutter/lib/features/connection/` | `pairing_test`, `tunnel_test`, `chat_sync_test`, `computers_test` (two computers, same session id; decision 0012) | mixed: `features/macs/`, `features/connection/` auto | P2, U6 |
| Phone chat | `apps/flutter/lib/features/chat/` | `chat_kit_test`, `chat_sync_test`, `markdown_table_test`, `send_now_test`, `takeover_test`, `keyboard_composer_gap_test`, `keyboard_focus_test`, `goldens/kit_golden_test` | auto | P1 (generated types) |
| Phone sessions | `apps/flutter/lib/features/sessions/` | `takeover_row_test`, `state_test`, `widgets_test`, `computers_view_test` (sessions grouped per computer) | auto | P2 |
| Phone voice | `apps/flutter/lib/features/voice/` | `voice_controller_test`, `voice_logic_test` | auto | none |
| Phone visualize | `apps/flutter/lib/features/visualize/` | `state_test` | auto | E15 (Mac side) |
| Phone settings | `apps/flutter/lib/features/settings/` | `settings_test` | auto | none |
| Phone debug tools | `apps/flutter/lib/debug/` | none | protected | none |

## Rust workspace (`crates/`)

Since Phase 4. Every crate but `crates/protocol` is a stub that builds, has one unit test in its `src/`, and does nothing else; the ported code arrives part by part from Phase 7. A crate's `Cargo.toml` and `build.rs` are protected even where its code is `auto`.

| Area | Folders and files | Tests | Zone | Parts |
| --- | --- | --- | --- | --- |
| Workspace | `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` (Rust 1.99.1), `deny.toml` (the license allowlist) | the `rust-test`, `rust-clippy` and `rust-deny` checks | protected | none |
| Engine | `crates/engine/`; E3a terminal in `src/terminal/` (`screen.rs`, `csi.rs`, `width.rs` the emulator; `ask_dialog.rs` with `dialog_open`; `keys.rs`; `pty/` forkpty on macOS and Linux, `Unsupported` elsewhere; `live.rs` a pty with a screen) | unit test in `src/lib.rs`; E1 ports in `tests/transcript_*.rs` (read the Swift fixtures by path); E3a ports in `tests/terminal_screen.rs`, `tests/terminal_ask_dialog.rs`, `tests/fixtures_terminal.rs` (every `Fixtures/terminal/*.bin` replayed), `tests/terminal_pty.rs` (`/bin/cat`, `/bin/sh`) | mixed: `src/transcript/`, `src/search/` auto (not written yet) | E1 to E3, E5 to E9, E11 to E15 |
| Engine service | `crates/notchlingd/`, the `notchling` binary: `src/cli.rs` (arguments), `src/layout.rs` (folders, `NOTCHLING_DATA_ROOT`, `--socket`), `src/daemon.rs` (`notchling daemon`: `engine::sessions` on `platform::watch`, `sessions.list`, `sessions.changed`), `src/clients.rs` (`status`, `sessions`), `src/pages_ops.rs` and `src/pages_cli.rs` (M5: `search.query`, `pages.list`, `pages.find`, `notchling pages`, `notchling page`), `src/service.rs` (LaunchAgent, systemd unit, Task Scheduler XML), `src/signals.rs` (SIGTERM) | unit tests in each module; `src/daemon_tests.rs` (start order with the first read held: the socket answers first, `sessions.list` waits for the read, a late read follows as an event, a second daemon is refused before it reads); `tests/daemon_e2e.rs` (the daemon over a copy of `crates/app/fixtures/notch`: list, change event, second daemon, stale socket, SIGTERM, `ensure_daemon`); `tests/service_files.rs` (golden files in `tests/golden/`, `install --dir`) | protected | E4, E16 |
| Terminal host | `crates/ptyhost/`, the `notchling-ptyhost` binary, a port of hostd with its socket contract (built, not installed): `wire.rs` (frames, `SessionId`), `state.rs` (`hostd-state.json`, update policy), `host.rs` (`SessionHost`), `pty_handle.rs` (the engine's pty with 2 MB scrollback; `PtySpawner`), `server.rs` and `ops.rs` (the socket and its ops), `client.rs`, `system.rs`, `main.rs` (`--status`, `--record-resume`, `--socket`, `--state`) | unit tests in modules; `tests/host.rs` (framing, host core, state, policy over fakes), `tests/socket.rs` (a temp socket: the contract over fakes, then real `/bin/cat` and `/bin/sh`), `tests/binary.rs` (scrollback, the binary on a temp socket) | protected | E3 |
| Local API | `crates/local-api/`: `wire.rs` (requests, replies, events, error codes), `framing.rs` (1 MiB lines), `peer.rs` (uid, pid and executable per OS), `tier.rs` (allowlist), `audit.rs`, `lock.rs` (`flock`), `transport.rs` (Unix socket; Windows pipe stub), `server.rs`, `conn.rs`, `hub.rs`, `client.rs`, `spawn.rs` (`ensure_daemon`) | unit tests in each module; `src/server_tests.rs` (a real socket in `/tmp`: hello first, tiers, `forbidden`, `needs_approval`, oversize, slow and never-reading clients, stale socket, second server); `tests/peer_copy.rs` (a copy of the test binary gets the agent tier, a symlink the app tier) | protected | E17 |
| Desktop app | `crates/app/`: `src/cli.rs`, `src/menu.rs`, `src/dock.rs`, `src/views.rs`, `src/session_source.rs` (the shared source of session rows: the daemon over the local API on a background thread, else the files; pure, every system); `src/mac/` (GPUI and AppKit, macOS only: `shell.rs`, `tray.rs`, `dock.rs`, `window.rs`, `render.rs`, `capture.rs`, `views.rs`) | unit tests in each pure module; `tests/render.rs` (ignored by default: renders `kit-gallery` to a PNG and checks it, `cargo test -p notchling-app -- --ignored`) | mixed: all but `Cargo.toml` and `build.rs` auto | U0 (shell), U10 (`--render`), then U2 to U11 |
| Chat (read-only) | `crates/app/src/chat/`: `data.rs` (the real folders, or a fixture root laid out as the notch's: `--data-root`, `NOTCHLING_DATA_ROOT`, else `crates/app/fixtures/chat`), `codex_list.rs`, `sidebar.rs`, `rows.rs`, `header.rs`, `inspector.rs`, `format.rs` (pure view models, every system); `chat/view/` (GPUI, macOS only: sidebar, header, conversation, disabled reply box, inspector; reloads on `platform::watch`) | unit tests in each pure module; `tests/chat_fixture.rs` over `fixtures/chat/` (every system); `tests/daemon_rows.rs` (Chat and the notch given the daemon's rows); `src/session_source_tests.rs` (an in-process local-api server: list, follow, fallback, reconnect, daemon started once); `tests/render_chat.rs` (ignored by default: `--render chat` over the fixture) | mixed: all but `Cargo.toml` and `build.rs` auto | U1 (Milestone 1: reading only; replies, actions and the markdown view later) |
| Search and pages (app) | `crates/app/src/search/`: `wire.rs` (the daemon's `search.query`, `pages.list` and `pages.find` as the app reads them), `library.rs` (the calls, over the actions' transport), `model.rs` (the panel's rows, selection, height and keys), `hotkey.rs` (the shortcut behind a trait; `NoHotKey` off macOS), `renders.rs` (pure, every system); `panel.rs`, `view.rs` and `src/mac/hotkey.rs` (the ⌘⇧P Carbon hot key) on macOS. `crates/app/src/pages/`: `model.rs` (the library list, its search and selection), `dates.rs`, `local.rs` (a copy's `file:` URL, Markdown wrapped), `renders.rs` (pure); `window.rs`, `view.rs` and `web.rs` (the `wry` page window) on macOS | unit tests in each pure module and `model_tests.rs`; `tests/render_search.rs` (ignored by default: `--render search-panel`, `search-panel-results`, `pages-window`) | mixed: all but `Cargo.toml` auto | U3 (Milestone 5), U4 (Milestone 5) |
| Kit | `crates/kit/`: tokens in `src/` (`ink.rs`, `typescale.rs`, `metric.rs`, `motion.rs`, `icon.rs`, `part.rs`, every system); widgets and `KitGallery` in `src/ui/` (GPUI, macOS only) | unit tests in each token module | mixed: all but `Cargo.toml` and `build.rs` auto | U0 |
| Platform | `crates/platform/` | unit test in `src/lib.rs` | protected | U0, U2 |
| Protocol types | `crates/protocol/src/lib.rs`, `crates/protocol/src/generated.rs` (from `tools/protocol/gen.py`; never edit it by hand) | `crates/protocol/tests/fixtures.rs` (every fixture parses and writes back the same JSON) | protected | P1, E5 |

## Repository

| Area | Folders and files | Tests | Zone | Parts |
| --- | --- | --- | --- | --- |
| Build, release and demo | `build.sh` (root wrapper), `legacy/macos/build.sh`, `legacy/macos/release.sh` and `.github/workflows/release.yml` (the release flow in use until Phase 6's is on), `Package.swift`, `legacy/macos/Info.plist`, `legacy/macos/Icon/`, `legacy/macos/Demo/`, `legacy/macos/Pet/` (pet art), `tools/migrate-and-install.sh` | the `app-build` check | protected | E16, U10 (`legacy/macos/Demo/` renders real views offscreen), U11 |
| Guard rails | `tools/guard/`, `tools/github/`, `.github/` (the agents' and the release workflows are in the rows below) | `tools/guard/tests/` | protected | none (Phase 2) |
| Version and changelog fragments | `tools/release/version.py`, `tools/release/fragments.py`, `changes/` (`README.md` is the guide), `CHANGELOG.md` | `tools/release/tests/test_version.py`, `tools/release/tests/test_fragments.py` | protected, `changes/` too: neither `auto` nor `new_files_only` names it, so an agent's pull request that adds a fragment waits for the maintainer | none (Phase 6) |
| Update feed | `tools/release/appcast.py` (Sparkle 2 appcast: add, promote, verify) | `tools/release/tests/test_appcast.py`, golden files in `tools/release/tests/golden/` | protected | none (Phase 6) |
| Beta and Promote workflows | `.github/workflows/beta.yml`, `.github/workflows/promote.yml` (off until `NOTCHLING_RELEASES` is `on`; only the `sign` job uses the `release` environment) | `tools/release/tests/test_release_workflows.py`, `tools/guard/tests/test_workflows.py` | protected | none (Phase 6) |
| Triage | `.github/workflows/triage.yml`, `tools/agents/triage.py`, `tools/agents/triage_labels.json` (the label allowlist) | `tools/agents/tests/test_triage.py` | protected | none (Phase 5) |
| Worker pre-start check and push wrapper | `tools/agents/preflight.py`, `tools/agents/push_wrapper.py`, `docs/agents/worker-setup.md` (the setup on each worker) | `tools/agents/tests/test_preflight.py`, `tools/agents/tests/test_push_wrapper.py` | protected | none (Phase 5) |
| Review agent | `.github/workflows/review-run.yml`, `.github/workflows/review-post.yml`, `tools/agents/review.py` | `tools/agents/tests/test_review.py`, `tools/agents/tests/test_review_workflows.py` | protected | none (Phase 5) |
| Bridge protocol | `protocol/README.md`, `protocol/agentconsole.v1.schema.json`, `protocol/fixtures/` (74 files) | CC: `ProtocolFixturesTests`; `tools/protocol/tests/test_fixtures.py`; `crates/protocol/tests/fixtures.rs`; `protocol_conformance_test` | protected | P1, E5 |
| Protocol generator | `tools/protocol/gen.py`, `tools/protocol/schema_validate.py` | `tools/protocol/tests/` (`test_gen`, `test_validate`, `test_fixtures`); `GUARD-PROTOCOL-GEN` | protected | P1 |
| Operating manual | `AGENTS.md`, `legacy/macos/Sources/AGENTS.md`, `apps/flutter/AGENTS.md`, `CLAUDE.md`, `ARCHITECTURE.md`, `GOVERNANCE.md`, `docs/goals.md`, `docs/rules.md`, `docs/decisions/`, `docs/policies/`, `.claude/` (skills) | `guard rules` and `GUARD-CLAUDE-MD` | protected | none |
| Other docs | `docs/` except the manual: `features.md`, `architecture.md`, `chat-design.md`, `chat-kit.md`, `console-design.md`, `dev-profile.md`, `agents/`, `plans/`, `superpowers/`, this map | none | mixed: all but `architecture.md`, `dev-profile.md`, `agents/`, `superpowers/` and this map auto | none |
| GPUI spike | `spike/` (throwaway, decision 0001) | none | protected | U0 to U10 (findings only) |

## Parts to areas

| Part | Areas |
| --- | --- |
| E1 Transcripts | Claude Code transcripts; Codex and Antigravity readers; Phone-side transcripts; Search (matching) |
| E2 Sessions | Sessions and jobs |
| E3 Terminal and replies | Replies and question dialogs; Terminal sheet; Session host (hostd) |
| E4 Hook | Hook |
| E5 Phone bridge | Phone bridge and pairing; Phone protocol and network; Bridge protocol |
| E6 Other tools | Codex and Antigravity runners; Delegate and Second Opinion |
| E7 Pages and search index | Pages library; Search (index) |
| E8 Accounts | Accounts and providers; Limits; Cost; Lanes and session moves |
| E9 Vault | Secret Vault |
| E10 Machines | Machines |
| E11 Session control | Session control; Slash commands; Lanes and session moves |
| E12 Keep awake and system | App shell, keep awake and system |
| E13 Installing into other tools | Hook; Delegate (`DelegateSkill`); Pages (`PageSkill`); Secret Vault (`VaultSkill`) |
| E14 Settings and data | App shell (`migrateFromAwake`); Accounts (Settings page) |
| E15 Phone extras | Phone extras; Phone visualize |
| E16 Command line and services | App shell (`main.swift`); Secret Vault (CLI); Build, release and demo |
| E17 Local API | none in Swift; `VaultServer`'s peer check is the model for `crates/local-api` |
| U0 Kit and shell | Design kit; App shell |
| U1 Chat | Chat window and model; Chat views; Terminal sheet |
| U2 Notch | Notch views and pet; Notch model, controller and cards |
| U3 Search | Search |
| U4 Pages | Pages library |
| U5 Accounts, Vault, Machines | Accounts and providers; Secret Vault; Machines |
| U6 Phone pairing | Phone bridge and pairing (`PhonePanel`) |
| U7 Pop-ups outside Chat | Secret Vault (approval, request form); Limits (banners); Session control (Trust Folder) |
| U8 Sheets | Session control (New Session); Accounts (Add Account, Settings); Delegate (Second Opinion); Chat views (pickers) |
| U9 Usage and cost | Limits; Cost; Chat views (meters) |
| U10 Rendering for agents | Notch views (`render-notch-view` skill); Build, release and demo (`legacy/macos/Demo/`) |
| U11 First run and updates | App shell; Build, release and demo |
| P0 Phone guard rails | Phone shell, kit and strings (goldens) |
| P1 Generated protocol types | Phone protocol and network; Phone chat; Bridge protocol; Protocol generator; Protocol types |
| P2 Remote computers | Phone pairing and Macs; Phone sessions |
| P3 Rename and docs | Phone shell, kit and strings |
