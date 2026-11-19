# XXXX: Organizations own machines, accounts and secrets

## Context

Facts as of 2026-11-19:

- Claude Code's saved accounts (`daemon/accounts`), Codex's and Antigravity's (`accounts/codex`, `accounts/antigravity`), the vault's list (`daemon/vault`) and the machines registry (`machines/registry`, version 2) have no owner. Limit switching picks from every saved account.
- Three of those stores are rewritten whole by older builds, and a field they don't know is dropped: `save_list`, `write_accounts` and the vault's serde. None of their guards checks a version (`book.rs::ours`, `list.rs::is_list`), so a version bump would not stop an older build there, and making those readers refuse would stop account switching and the vault in an older build altogether.
- Each tool's live login is Mac-wide: Claude Code's Keychain item with `~/.claude.json`, Codex's `auth.json`, Antigravity's item and token file.
- `oracle-sg` runs GitHub runners of two owners under one `ghrunner` with Docker.
- The maintainer asked on 2026-11-18 and 2026-11-19 for machines "in organizations, like Company A, Company B, Personal", and then for the organization to be the top-level owner of machines, accounts and vault secrets (the design of 2026-11-19, kept under `docs/local/`, which is not in the repository).

## Decision

- **An `organizations` document** in `<data>/notchling.db` (Node class, this Mac only). Personal (id `personal`) always exists, can be renamed, can't be deleted, and is where everything starts. Each organization has a stable id (made once from the name, never changed), a unique name of 1 to 40 characters, an optional GitHub owner, a colour, the folders on this Mac that belong to it (a folder is in one organization only; the longest match wins), and Machines' settings.
- **One organization per machine, account and vault entry**, kept in each store's own record as an optional `organization` field, with a copy in the `organizations` document (`shadow.accounts` keyed `claude:<id>`, `codex:<id>`, `antigravity:<id>`, and `shadow.vault` keyed by the entry's lower-cased name), written in the same transaction as the field. A reader takes the field, else the shadow, else Personal; when they differ the field wins and the shadow is fixed. The registry goes to version 3 instead, so an older build refuses it and never writes over it.
- **A session's organization comes from its folder**, else Personal, worked out on each call. The folder is matched as the vault already matches a caller's folder: standardized, `~` expanded, and symlinks resolved.
- **No sharing.** A resource is in one organization. Moving is a plan that lists what leaves, and a value two organizations need is saved twice.
- **The rules** (a secret goes only to machines of its organization; a machine signs in only with accounts of its organization; limit switching stays inside the organization of the account in use; a session's agents get only its organization's machines, secrets and accounts; cluster dispatch stays inside the organization) are each their own change and their own record of the check. This record covers the model and the migration only.
- **Migration** runs at every daemon start and is idempotent. It makes the document with Personal, gives every existing account, vault entry and machine the field and the shadow, and changes nothing else: no login switches, no secret moves, nothing runs on any machine. The text of each account and vault document is saved once before its first write with organizations, and the version 2 registry is saved once before the first version 3 write, as the version 1 registry was.
- **An unreadable document fails closed.** A document that isn't valid JSON, has another version, has no Personal, or repeats an id, a name or a folder is never written over, never migrated past, and the view of it is marked closed, so the later rules can refuse what would be sent or switched. Fields still read.
- **A rename by an older build** (a vault entry saved under a new name has neither field nor shadow) reads as Personal and, once there are two organizations, raises one notice; the entry is then marked Personal.

## Consequences

- With only Personal every answer is today's. The only visible change in the stores is the added field, in each list.
- Downgrading past this record makes the machines registry unreadable to the older build. It shows it as unreadable and never writes over it; the saved version 2 text is beside it.
- The stores' own writers keep the field, and the new build never drops it; an older build does, and the shadow brings it back.
- The removal paths of the stores (Claude, Codex and Antigravity accounts, the vault) call `Organizations::forget`, so a re-added name starts in Personal. A vault rename moves the shadow key in the same write as the list.
- The first start writes the stamped lists with their keys in sorted order (the JSON is read and written through `serde_json::Value`, which sorts keys); the text before the change is kept whole in `organizations-backup/`.
- The version 2 registry text is kept in `organizations-backup/`, beside the registry, in a private folder.
- Folders overlap on their expanded and resolved form, and match without regard to case on macOS.
- Amends [cluster secrets and logins](XXXX-cluster-secrets-and-logins.md) once the rules land: release checks the organization. `Member.organization` is not part of this record.
- Known gaps: Mac-wide logins; a vault entry renamed by an older build reads as Personal; a caller with no session is placed by its process folder.

## Status

Proposed. The number is a placeholder, assigned at merge.

## Date

2026-11-19
