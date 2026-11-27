# ptyhost: notes for agents

## Why the code is this way

The code carries no comments (see [AGENTS.md](../../AGENTS.md)). What a change could break without knowing it, by file:

- `crates/ptyhost/src/host.rs::resume_all`: A session is resumable only after Claude's session file appears and its id is recorded; records without an id or whose folder is gone are dropped, and a session that ended by itself or on purpose is not brought back.
- `crates/ptyhost/src/lib.rs::contract`: This host is built and tested but never installed until handover: it uses `~/Library/Application Support/AgentConsole/hostd.sock`, so tests must always pass `--socket` and `--state` in a scratch folder and never touch the real one.
- `crates/ptyhost/src/main.rs::run`: On SIGTERM the host just leaves (the ptys end with it and the next start resumes them); the signal handler does only an atomic store, and a newer binary installed beside it is taken over only when nothing runs.
- `crates/ptyhost/src/pty_handle.rs::Listener`: Each listener runs on its own thread with a bounded queue (twice the scrollback) and is dropped when it lags; never hold the handle's lock while a listener runs, or one stalled client blocks the pty reader and the rest.
- `crates/ptyhost/src/pty_handle.rs::attach`: The scrollback replay is delivered on the caller's thread before the listener's own thread starts, so replay and live bytes never interleave.
- `crates/ptyhost/src/pty_handle.rs::terminate`: Terminate sends SIGTERM and waits two seconds for the process to save its session before SIGKILL.
- `crates/ptyhost/src/server.rs::bind`: Check the socket path against `sun_path` (104 bytes on macOS, 108 on Linux), and remove a leftover socket file only when nothing still answers on it; only the same uid may connect, checked on every connection.
- `crates/ptyhost/src/server.rs::write`: Ignore SIGPIPE on every client socket (`SO_NOSIGPIPE` on macOS) so a write to a gone client fails with EPIPE instead of ending the host.
- `crates/ptyhost/src/state.rs::HostState`: `hostd-state.json` must stay byte-compatible with the Swift host (sorted keys, ISO 8601 dates, whole seconds, 0600) so either host reads the other's file; the same goes for the wire's sorted JSON keys and upper-case dashed session ids.
- `crates/ptyhost/src/system.rs::scrubbed_environment`: Every `CLAUDE_CODE_*` variable and `NO_COLOR` is dropped from a new session's environment (an inherited marker makes a child session with no transcript), and the login environment comes from the user's shell because launchd gives almost none.
- `crates/ptyhost/src/wire.rs::sort_keys`: The workspace builds serde_json with `preserve_order` (through GPUI), so a Map keeps insertion order unless rebuilt sorted; frames must rebuild it to match Swift's `.sortedKeys`.
