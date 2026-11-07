---
name: add-provider
description: Use when adding a new account provider to Notchling (another app or CLI whose logins and limits the Accounts window should list and switch, like Codex or Antigravity), or when moving an existing store onto the AccountProvider registry.
---

The recipe is "Adding a provider" in `docs/architecture.md`; this is it with the files and checks. Every file here is protected: the PR waits for the maintainer, and it should. A provider touches someone's real login.

Before starting, read the login rules in `Sources/AGENTS.md` ("Things that bit before": Claude Code, Codex, Antigravity and Claude Desktop each keep their login in a different place) and look at the smallest existing provider, `Sources/Notchling/Providers/CodexProvider.swift`, with its store `Sources/Notchling/Accounts/Codex.swift`.

1. **The store** (`Sources/Notchling/Accounts/<Name>.swift`). It finds where the login lives, reads it, keeps saved copies of other accounts, switches, and reads usage.
   - Never renew a token that is in use: refresh tokens rotate and the app holding the old one is signed out.
   - Set every place the login lives (Antigravity: the Keychain item and the mirror file).
   - Quit the apps that read the login at launch before switching, and reopen them after.
   - Keychain for logins goes through `/usr/bin/security` (hex on stdin), never the Security framework. Files that hold logins are written with `PrivateFile`.
   - Keep parsing (login file, usage JSON) in pure functions that take values, so tests can reach them.

2. **The adapter** (`Sources/Notchling/Providers/<Name>Provider.swift`). A final class conforming to `AccountProvider` (`Providers/AccountProvider.swift`) that holds the store.
   - Add a `ProviderID` constant next to `.codex` and `.antigravity`.
   - Set `capabilities` (`.addAccount`, `.addFromMenu`, `.restartsApp`, `.usage`).
   - Map the store's accounts to `ProviderAccount`, its limits to `UsageReport` (`Providers/UsageReport.swift`; `UsageReport(GroupedUsage)` for named groups of bars).
   - Wrap `switchTo` with `awaitStore`. Put the confirmation texts in `switchConfirmation` and `addConfirmation` (nil when nothing is interrupted).
   - Set `menuStyle`: the not-signed-in text, `showsLimits`, `limitsPlaceholder`, `waitsInMenu` if an app sits at a sign-in screen, `syncsOnOpen` if reading the live login is cheap.

3. **The page.** A SwiftUI view built on `AccountRow` in `Accounts/AccountsWindow.swift`, with a `content` closure for limits (`GroupedLimits`). Don't hand-roll another card. Return it from `makePage`. Use kit styles so buttons show the pointing hand.

4. **Register it** in `AppDelegate.providers` (`Sources/Notchling/App/AppDelegate.swift`). The Accounts tab, the menu section, the refresh and the 60 s sync timer follow from the registry. Add every new UserDefaults key to the `migrateFromAwake()` list in the same file (`LINT-DEFAULTS-MIGRATED`).

5. **Limits, if it has them.** Watching goes through `ProviderLimitWatcher` (`Limits/ProviderLimits.swift`) and `LimitLogic`. Add a mode key (`<name>LimitMode`) and say in the PR which modes make sense. A provider whose switch restarts an app gets Off and Notify only, like Antigravity.

6. **Chat.** Account work must be doable from Chat, not only the menu bar. Say in the PR whether New Session (`Chat/NewSessionSheet.swift`) and the Accounts sheet (`Chat/AddAccountSheet.swift`) should list it; those read the stores directly.

7. **Tests** in a new file, `Tests/NotchlingTests/<Name>ProviderTests.swift`: the store's parsing from fixtures (a made-up login file and usage response under `Tests/NotchlingTests/Fixtures/`) and the mapping to `UsageReport`, as `ProviderTests.testCodexUsageJSONReachesTheReport` does. Never test by switching a real login; tests never touch Keychain.

8. **Checks.**
   ```sh
   swift test
   PYTHONPATH=tools/guard python3 -m guard lint
   ./build.sh
   ```
   The capability check will flag the store's process, network and Keychain use. That's expected; list each use in the PR and why it's needed.

9. **Docs.** Add the provider to `README.md` and `docs/features.md`, its row to `docs/feature-map.md`, and its login facts to "Things that bit before" in `Sources/AGENTS.md`.

Tried: not yet
