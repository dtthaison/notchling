# XXXX: Machines have several addresses, picked in order; Tailscale networks are switched only on a click

## Context

A machine in Machines had one `host` and one `user`, and every ssh path built its command from them. The maintainer has two Tailscale networks under one login and this Mac is on one at a time, so a machine on the other network answers nothing, and nothing said why. A machine can also be reached by a LAN address, a public address or name, another port, or through a jump host. Adding the machines of a network meant typing each one.

## Decision

- A machine has an ordered list of addresses: a kind (Tailscale name, Tailscale IP, LAN, public, jump), a host, an optional user (the machine's own by default), an optional port, the Tailscale network for the Tailscale kinds, and the machine to go through for a jump. `host` and `user` stay as the first address.
- One function picks the address. It tries them in order, skips a Tailscale address whose network this Mac isn't on, gives each a short connect timeout, and keeps the winner per machine for a few minutes (a failure for seconds). Every ssh, scp and git path of the Machines library uses it. A jump address uses `-J` with the jump machine's own picked address. `StrictHostKeyChecking`, `BatchMode` and the `--` before a host are as before.
- When the only addresses are on a network this Mac isn't on, the result says so ("On work.example, this Mac is on person@example.com") and the Machines row offers Switch Network.
- Switch Network runs `tailscale switch <id>` and is the only place Notchling changes the Tailscale network. It runs on the user's click after a question saying the other network will drop. It is an App-tier op (`tailscale.switch`); agents and the command line can't call it, and nothing switches by itself, not even to reach a machine.
- Add Machine can read the active Tailscale network (`tailscale status --json`, read only) and save the machines picked as Machines entries: their MagicDNS name and Tailscale IP with that network, no node, nothing installed, no join. Only the active network is listed; listing both waits for API keys.

## Consequences

- `machines.json` is version 4 when any machine has addresses and version 3 otherwise. A build that only knows version 3 refuses a version 4 file instead of loading it and dropping the list on its next save. The first save that writes version 4 over a version 3 file keeps the old file once as `machines.json.v3-backup` in the backup folder. The first address is mirrored into `host` and `user`, so a reader of the raw file still sees a working address.
- Probing happens in the Machines library, so Check, Join, the poller, Update Machines and the git and scp steps all follow the same pick. The cache is in memory; it is forgotten when a machine's addresses change or after a switch.
- It adds one new App-tier op to `docs/local-api-ops.md` and one new place that runs a command with a side effect (`tailscale switch`), on a click.
- The Tailscale command line is found in `/Applications/Tailscale.app` or on `PATH`; its path is a parameter, and tests use recorded outputs with made-up names.
- Only machines outside the Cluster get the address line and Edit Addresses in this change. Cluster members keep the address the cluster gives them.
- A failed pick never falls back to the machine's `host`: a machine with addresses and no picked route gets a route that cannot connect, and the steps report the refusal text. Lookups (`machine()`, rendering) read only the cache; probes run on background tasks, the poller and the actions.
- A Tailscale network is matched by its network name or id, never by the login account, because two networks can share one login. An address, user, port, network or jump name is validated before it reaches a command line, and a jump chain is cut at a loop or at three hops.
- Add from Tailscale has an Organization picker; it starts on Personal because this base has no current Organization. It doesn't add API keys for listing both networks.

## Status

Proposed with the change on `agent/machine-addresses`. The number is a placeholder, assigned at merge, together with the notes changes the maintainer makes.

## Date

2026-11-19
