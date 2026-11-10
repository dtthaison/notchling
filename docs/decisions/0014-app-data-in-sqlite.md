# 0014: The app's own data in SQLite, and search by bm25

## Context

The Rust app and the daemon kept their own data in JSON files rewritten whole: `settings.json`, the search index's cache `daemon/search-index.json`, and (for one build) Chat's unsent replies in `chat-drafts.json`. A whole-file rewrite from two processes at once keeps only one of the changes, and the search index matched words as substrings over everything in memory. On 2026-11-10 the maintainer asked for SQLite to hold the app's data (settings, chat drafts, configs) and for search to use SQLite with bm25.

## Decision

- The app's own data goes in one SQLite database, `notchling.db` in the data folder (`notchling_engine::store`): a row per value, WAL mode, a busy timeout, 0600, a symlink in its place refused. Chat's unsent replies are its first table (`drafts`, one row per session row); the old JSON file is read in once and removed.
- The session index lives in `daemon/search.db` (`notchling_engine::search::fts`): each session's metadata in `docs` and its title, folder and text in an FTS5 table (`unicode61 remove_diacritics 2`). A query's words are required prefixes, ranked by bm25 (title 10, folder 4, text 1) times a bonus of up to 1.5 for a change in the last 14 days. When that finds nothing, the old substring match runs, so a word inside another still finds. It replaces `search-index.json`; a damaged file is set aside as `search.db.damaged` and rebuilt.
- Settings are in `notchling.db` (the `settings` table, a row per key, the value as JSON text), behind the same `Settings` API: `Settings::open` still takes the path of `settings.json`, whose folder is the data folder. While the database has no settings, `open` reads the old file and the first write moves its values into rows and removes it; a file that isn't a settings file is left where it is and reported as corrupt. A write touches only the rows it names, so the app and the daemon setting different keys at once both keep theirs. Nothing watches the file: every reader opens the settings again when it needs them.
- Files other programs read stay as they are: `lanes.json` (the Swift hook reads it), `pages.json`, `machines.json`, the bridge's files and Claude Code's `settings.json`.

## Consequences

- Two processes writing at once each keep their change, and a crash mid-write leaves the last committed state.
- Search folds accents ("sua" finds "sửa") and ranks by how well a session matches, not by fixed points. The first search after the upgrade reads every transcript again, since the JSON cache isn't imported.
- The data is no longer readable with a text editor; `sqlite3` reads it.
- `rusqlite` with its bundled SQLite was already a dependency (Antigravity's summaries), so nothing new is pulled in.

## Status

Accepted: the maintainer asked for it on 2026-11-10.

## Date

2026-11-10
