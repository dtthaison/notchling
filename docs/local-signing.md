# Signing local builds

`tools/bundle-next.sh` signs ad hoc unless told otherwise. After an ad hoc build is installed, macOS asks for the login password once per Keychain item the daemon reads ("notchling wants to use your confidential information stored in "dev.notchling.audit.head""). This page says why, and how a build signed with a stable identity stops it.

## Why it asks

The daemon (`notchling`) keeps three generic password items through the Security framework (`security_framework::passwords`, which calls `SecItemAdd` with the default access):

| Item (service) | What | Code |
| --- | --- | --- |
| `dev.notchling.audit` | the audit's sealing key | `crates/engine/src/logs/keys_mac.rs` |
| `dev.notchling.audit.head` | the audit's checkpoint | same |
| `Notchling Secret Vault` | the vault's values | `crates/engine/src/vault/keychain_mac.rs` (also `notchling-machines --vault`) |

An item made that way carries two checks, and a reader must pass both (Apple's `securityd`, `acls.cpp` `SecurityServerAcl::validate`):

1. **The trusted application list**: the designated requirement of the program that made the item. For an ad hoc signature that requirement is `cdhash H"…"`, the hash of that exact build.
2. **The partition list**: an id `securityd` gives the reader (`clientid.cpp` `partitionIdForProcess`): `apple:` for Apple's code, `apple-tool:` for `/usr/bin/security`, `teamid:<team>` for code signed by an Apple-issued Mac Development, Mac Distribution, Developer ID, App Store or TestFlight certificate, and `cdhash:<hash>` for anything else that is signed. Ad hoc and self-signed code both get `cdhash:`.

A new ad hoc build has a new cdhash, so it fails both. Always Allow adds the new build to both lists, and changing a partition list needs the login password. That is the prompt, once per item and per build.

The account logins don't ask: they go through `/usr/bin/security` (`crates/engine/src/accounts/keychain.rs`), so the reader is always Apple's tool (`apple-tool:`), whatever Notchling build calls it. That is also why anything that can run `security` can read them; the vault and the audit key avoid it on purpose.

The app itself (`notchling-app`) and `notchling-ptyhost` read no Keychain item.

## What fixes it

A signature whose designated requirement and partition id stay the same from build to build: a certificate Apple issued (Apple Development, or Developer ID), with the same identifier on each binary every time. `bundle-next.sh` gives them fixed identifiers: `dev.notchling.daemon` for `notchling`, `dev.notchling.ptyhost` for the terminal host, and the bundle id for the app.

A self-signed certificate is not enough. It keeps the designated requirement stable (check 1 passes), but its partition id is still `cdhash:`, so check 2 fails and the password prompt stays.

## What it costs

Today an item trusts one exact build, so a program an agent compiles can't read it without the maintainer typing the password. With a stable identity the items trust every program signed by that identity under `dev.notchling.daemon`. If the signing key can be used without asking, an agent with a shell can sign its own program with it and read the vault and the audit key with no prompt. Keep the key behind a question (step 4 below), or in its own locked keychain, so signing needs the maintainer.

Releases end this for good (decision 0010): Developer ID builds are signed in CI, the key never sits on this Mac, and every release shares one designated requirement and team.

## One-time steps (the maintainer)

1. See which identities exist (read only):

   ```sh
   security find-identity -v -p codesigning
   ```

   An `Apple Development: …` or `Developer ID Application: …` line will do. If there is none: Xcode > Settings > Accounts, add an Apple ID (a free one works), Manage Certificates, + > Apple Development.

2. Check the certificate's team and end date:

   ```sh
   security find-certificate -c "Apple Development: <name> (<id>)" -p | openssl x509 -noout -subject -enddate
   ```

   `OU` is the team. A renewed certificate under the same name and team keeps the same requirement (it names the certificate's common name and Apple's intermediate, not the certificate itself).

3. Build with it:

   ```sh
   NOTCHLING_SIGN_IDENTITY="Apple Development: <name> (<id>)" tools/bundle-next.sh
   ```

   The SHA-1 from step 1 works too. The script stops before building if no identity matches, prints the team it signed with, and warns when the daemon's signature doesn't meet the requirement `securityd` gives a `teamid:` to (then the prompts stay). `NOTCHLING_SIGN_KEYCHAIN=<path>` signs from a keychain other than the default list.

4. When `codesign` asks to use the private key, choose Allow, not Always Allow, so nothing else can sign with it silently. For no prompt at all while building, move the identity to its own keychain with its own password and an auto-lock (`security set-keychain-settings -l -u -t 300 <path>`), unlock it only to build, and pass `NOTCHLING_SIGN_KEYCHAIN`.

5. Install that build as usual. On its first start each item above asks once more, since it was made by an ad hoc build: type the password and choose Always Allow. That adds the identity's requirement and its `teamid:` to the item. Every later build signed the same way reads the items without asking.

Don't delete the items to start clean: the audit key seals the log and the vault item holds the values.

## When it asks again

- The identity changes (Apple Development to Developer ID, another team, another common name) or a binary's identifier changes: once per item.
- A build is signed ad hoc again (no `NOTCHLING_SIGN_IDENTITY`): once per item, as today.
- `notchling-machines` run from `cargo` is signed ad hoc by the linker: `--vault` asks for the vault item after each rebuild.
