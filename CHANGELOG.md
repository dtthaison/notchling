# Changelog

Notable changes to Notchling. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Keep Local Copies of Artifacts, on by default: every Artifact Claude Code publishes is also saved under `~/Library/Application Support/Notchling/pages`, so it stays readable after you switch accounts (claude.ai shows a page only to the account that published it).
- An Artifacts window: search and filter every saved page by account, read it in place, and see which account owns its claude.ai link. The Artifacts menu opens the newest 12 there.
- Continue Agents After Sleep, on by default: when the Mac wakes, background agents whose reply was cut off by sleep are told to continue.
- When the Account in Use Nears Its Limit: warns at 90% and 100%, and switches to the saved account with the most room at a point you set (80–99%, default 95%). After a switch, background agents stopped on the limit are restarted and told to continue.
- Limit alerts show a Notchling banner with the app icon when macOS doesn't allow the app's notifications.
- Reads the usage endpoint's `limits` list, so per-model limits and new kinds of limit show up by themselves.

### Changed

- Accounts without a 5-hour window, such as Max 20x since October 2026, show and watch only their weekly limit.

## [1.1.0] - 2026-10-27

First public release.

### Added

- Agents in the notch, in Input, Working and Done tabs, with peeks when an agent finishes or needs you.
- Keeping the Mac awake: Off, Always On, or While Agents Are Working.
- Replies to background agents, and answers to questions and permission prompts from the notch.
- Account switching for Claude Code, Claude Desktop and Antigravity, with each account's limits.
- Claude Token Cost: today's cost in the menu, and a window with cost at API prices by day, channel, account, model and project.
- The web-swinging pet, which sits on the frontmost window's title bar when no agent is working.
- Open source under the MIT License, with a contributing guide, code of conduct and security policy.

### Fixed

- The menu bar menu no longer lags or skips items when moving between them.

[Unreleased]: https://github.com/dtthaison/notchling/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/dtthaison/notchling/releases/tag/v1.1.0
