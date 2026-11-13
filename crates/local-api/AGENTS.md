# local-api: notes for agents

## Why the code is this way

The code carries no comments (see [AGENTS.md](../../AGENTS.md)). What a change could break without knowing it, by file:

- `crates/local-api/src/audit.rs::audit_reason`: A failed call keeps its message in the audit line only for ops that type into a session's terminal, whose messages are fixed sentences; other ops' errors may carry paths, accounts or secrets and must not be logged.
- `crates/local-api/src/client.rs::subscribe`: Subscribe first, then list, so no change falls between the two; a `Client` call blocks, so run it off the UI thread.
- `crates/local-api/src/conn.rs::HubGuard`: A connection is taken off the hub however its thread ends, a handler panic included: left there, an app-tier entry keeps `app_attached` true so approval fails open, and the hub's handle keeps the socket open.
- `crates/local-api/src/conn.rs::peer re-read`: A connection in a slot kept for the app is closed at hello unless it gets the app tier; the agent tier applies until hello, when the peer is read again (the exec race).
- `crates/local-api/src/hub.rs::publish`: Events go through each connection's bounded queue without waiting; a client whose queue is full or gone is closed, so a client that doesn't read never holds up the publisher or others.
- `crates/local-api/src/lock.rs::DaemonLock`: One daemon per socket by an exclusive `flock` the kernel drops on any exit; only once the lock is held is a socket file left behind known stale and removed, and never a path that isn't a socket.
- `crates/local-api/src/peer.rs::start_time`: On Linux the start time is field 22 of `/proc/<pid>/stat`, counted after the last `)` because the command name may hold spaces or parentheses; read it before the exe path so an exec in between shows as a changed file.
- `crates/local-api/src/server.rs::app_follows`: "App attached" alone says nothing about what the app reads: a pairing request published to no follower is seen by nobody, so check `app_follows(topic)` before relying on it.
- `crates/local-api/src/server.rs::slots`: Only the accept loop takes a client slot (threads only give it back), so a burst of connects can't pass `max_clients`; an accept error that ends one connect must not end the listener.
- `crates/local-api/src/spawn.rs::ensure_daemon`: The daemon is started from the canonical path that `spawn_allowed` checked (so a retargeted symlink starts nothing new), with every `NOTCHLING_*` variable of the starter removed, in its own process group.
- `crates/local-api/src/spawn.rs::is_cargo_target`: A cargo target folder moved by `CARGO_TARGET_DIR` is recognized by the `CACHEDIR.TAG` at its root; without the tag, a folder that merely holds a `debug/` is not one.
- `crates/local-api/src/spawn.rs::wait`: A started daemon that is alive but not yet listening when the 3 s wait ends is left running, never killed: it is only slow, and the app keeps trying to connect to it.
- `crates/local-api/src/tier.rs::AppTierPaths`: The app-tier allowlist is canonicalized once at load, so retargeting a symlink later grants nothing, and a path that doesn't exist at load is dropped; an unknown path is the agent tier.
- `crates/local-api/src/transport.rs::bind`: The socket is tightened to 0600 right after bind, and its 0700 folder keeps others out before that; the caller must already hold the lock.
- `crates/local-api/src/wire.rs::Kind`: A client's `hello` kind is self-reported and never decides the tier; the tier comes only from the peer's executable path.
- `crates/local-api/src/wire.rs::Reply`: A reply whose `ok` is `null` is still a success, and extra reply fields never replace `id`, `ok` or `err`; clients ignore keys they don't know, so new fields stay additive.
- `crates/local-api/src/wire.rs::vault events`: Vault events (`vault.approval.pending`, `vault.request.pending`, `vault.changed`) never carry a secret value; `verified` in an approval is what the daemon checked with the kernel, `command` and `folder` only what the agent claims.
