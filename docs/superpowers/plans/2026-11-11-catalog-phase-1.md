# Model Catalog (phase 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Models, efforts, context windows and prices for Claude Code, Codex and Antigravity come from one engine catalog (live sources, a cache, a data file) instead of lists written in code.

**Architecture:** A new engine module `catalog` holds a compiled-in data file (`crates/engine/data/catalog.json`) merged with an optional override file, and turns live model lists (Anthropic's `/v1/models`, Codex's `models_cache.json`, `agy acp`'s `configOptions`) into one `Vec<ModelInfo>` per tool. Cost tracking prices through it; the daemon serves it as `catalog.get` and uses it as the `session.new --model` allowlist; Chat's pickers and Context Meter read it.

**Tech Stack:** Rust (workspace crates `notchling-engine`, `notchling-notchlingd`, `notchling-local-api`, `notchling-app`), serde_json, the phone protocol's JSON Schema plus `tools/protocol/gen.py`.

**Spec:** `docs/superpowers/specs/2026-11-11-tool-provider-interface-design.md` (phase 1, "Catalog").

## Global Constraints

- Tests never touch Keychain, real logins, the network, or the real `~/.claude`, `~/.codex`, `~/.gemini`; every source is a fixture or a fake (`FakeHttp`, temp folders).
- Never renew or refresh a token to read a model list; the token comes only from `ClaudeAccounts::usage_token(active id)`, which already returns `None` for an expired live login.
- The override file can change names, prices and windows but never adds a model to the allowlist.
- Unknown is visible: a model with no price adds tokens and no dollars, and is listed as unpriced; never a quiet $0.
- The local API and the phone protocol change additively only; the phone schema first, then `python3 tools/protocol/gen.py`, never hand-edited generated files.
- `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` pass after every task. `cargo` is at `~/.cargo/bin` (not on the default PATH here).
- Commits go to `agent-run-oss`; one commit per task, message in the repo's plain-sentence style.
- The Swift app (`legacy/macos/`) is not changed.

## Review Focus

1. **Anthropic's list is unreachable (offline, 401, 5xx, garbage body):** the picker still shows the last cached list, else the data file's; nothing errors in the UI. Test in Task 3.
2. **A live model id with a date or `[1m]` suffix (`claude-fable-5-2-20261101`, `claude-opus-5-5[1m]`):** it matches the same price, window and allowlist entry as the bare id. Test in Task 1.
3. **A malformed or partial override file:** it is ignored field by field (a bad price entry drops that entry, not the whole file), and the built-in data still loads. Test in Task 1.
4. **A price fixed in the override file:** costs already counted are repriced on the next scan, not only new requests. Test in Task 2.
5. **The phone or an old app sends a short model name or an id the catalog no longer lists:** `session.new` still accepts the four short names, and refuses an unknown id with the allowed list in the message. Test in Task 5.

---

### Task 1: The catalog data file and its types

**Files:**
- Create: `crates/engine/data/catalog.json`
- Create: `crates/engine/src/catalog/mod.rs` (types, `Catalog`, merge, lookup)
- Create: `crates/engine/src/catalog/tests.rs`
- Modify: `crates/engine/src/lib.rs` (add `pub mod catalog;`)

**Interfaces:**
- Produces:
  - `pub enum Tool { Claude, Codex, Antigravity }` (serde `"claude" | "codex" | "antigravity"`), `Tool::parse(&str) -> Option<Tool>`.
  - `pub struct Price { pub input: f64, pub output: f64, pub cache_read: f64, pub over: Option<Over> }`, `pub struct Over { pub tokens: f64, pub input: f64, pub output: f64, pub cache_read: f64 }` (dollars per million tokens; `over` is Haiku 5.5's long-prompt tier).
  - `pub enum Tier { Current, Older }`.
  - `pub struct LiveModel { pub id: String, pub name: Option<String>, pub context_window: Option<u64>, pub efforts: Vec<String>, pub default_effort: Option<String> }`.
  - `pub struct ModelInfo { pub id: String, pub name: String, pub tier: Tier, pub context_window: Option<u64>, pub efforts: Vec<String>, pub default_effort: Option<String>, pub price: Option<Price>, pub borrowed: bool }` (Serialize/Deserialize, camelCase).
  - `impl Catalog { pub fn built_in() -> Catalog; pub fn load(override_file: &Path) -> Catalog; pub fn price(&self, tool: Tool, model: &str) -> Option<Price>; pub fn context_window(&self, tool: Tool, model: &str) -> Option<u64>; pub fn models(&self, tool: Tool, live: &[LiveModel]) -> Vec<ModelInfo>; pub fn allowed(&self, tool: Tool, live: &[LiveModel]) -> Vec<String>; pub fn price_borrowed(&self, tool: Tool, model: &str) -> bool; pub fn prices_hash(&self) -> String }`.
  - `pub fn model_key(id: &str) -> String` moved here from `cost/prices.rs` (strips `[1m]` and a `-YYYYMMDD` / `-YYYY-MM-DD` date suffix); `cost::prices::model_key` re-exports it.

The data file, exact contents for today (values from `cost/prices.rs`, `cost/other.rs`, `chat/claude_picker.rs` and `chat/format.rs` at commit `a5617ed`):

```json
{
  "version": 1,
  "claude": {
    "current": ["claude-opus-5-5", "claude-fable-5-2", "claude-sonnet-5-5", "claude-haiku-5-5"],
    "older": ["claude-haiku-4-5", "claude-sonnet-5", "claude-opus-5", "claude-fable-5-1", "claude-fable-5",
              "claude-opus-4-8", "claude-opus-4-7", "claude-opus-4-6", "claude-sonnet-4-6"],
    "efforts": ["low", "medium", "high", "xhigh", "max"],
    "windows": {"claude-opus-5-5": 400000},
    "prices": [
      {"match": "fable-5-2", "input": 8.0, "output": 40.0, "cache_read": 0.20},
      {"match": "mythos-5-2", "input": 8.0, "output": 40.0, "cache_read": 0.20},
      {"match": "fable-5-1", "input": 10.0, "output": 50.0, "cache_read": 0.25},
      {"match": "mythos-5-1", "input": 10.0, "output": 50.0, "cache_read": 0.25},
      {"match": "fable", "input": 10.0, "output": 50.0, "cache_read": 1.0},
      {"match": "mythos", "input": 10.0, "output": 50.0, "cache_read": 1.0},
      {"match": "opus-5-5", "input": 4.0, "output": 20.0, "cache_read": 0.20},
      {"match": "opus-5", "input": 5.0, "output": 25.0, "cache_read": 0.50},
      {"match": "opus-4-8", "input": 5.0, "output": 25.0, "cache_read": 0.50},
      {"match": "opus-4-7", "input": 5.0, "output": 25.0, "cache_read": 0.50},
      {"match": "opus-4-6", "input": 5.0, "output": 25.0, "cache_read": 0.50},
      {"match": "opus-4-5", "input": 5.0, "output": 25.0, "cache_read": 0.50},
      {"match": "opus-4", "input": 15.0, "output": 75.0, "cache_read": 1.50},
      {"match": "sonnet-5", "input": 2.0, "output": 10.0, "cache_read": 0.20},
      {"match": "sonnet", "input": 3.0, "output": 15.0, "cache_read": 0.30},
      {"match": "haiku-5-5", "input": 0.10, "output": 0.50, "cache_read": 0.01,
       "over": {"tokens": 100000, "input": 0.50, "output": 2.50, "cache_read": 0.05}},
      {"match": "haiku-4-5", "input": 1.0, "output": 5.0, "cache_read": 0.10},
      {"match": "haiku", "input": 0.80, "output": 4.0, "cache_read": 0.08}
    ]
  },
  "codex": {
    "prices": [
      {"id": "gpt-5", "input": 1.25, "output": 10.0, "cache_read": 0.125},
      {"id": "gpt-5-codex", "input": 1.25, "output": 10.0, "cache_read": 0.125},
      {"id": "gpt-5.1", "input": 1.25, "output": 10.0, "cache_read": 0.125},
      {"id": "gpt-5.1-codex", "input": 1.25, "output": 10.0, "cache_read": 0.125},
      {"id": "gpt-5.1-codex-max", "input": 1.25, "output": 10.0, "cache_read": 0.125},
      {"id": "gpt-5-mini", "input": 0.25, "output": 2.0, "cache_read": 0.025},
      {"id": "gpt-5.1-codex-mini", "input": 0.25, "output": 2.0, "cache_read": 0.025},
      {"id": "gpt-5-nano", "input": 0.05, "output": 0.40, "cache_read": 0.005},
      {"id": "gpt-4.1", "input": 2.0, "output": 8.0, "cache_read": 0.50},
      {"id": "o3", "input": 2.0, "output": 8.0, "cache_read": 0.50},
      {"id": "o4-mini", "input": 1.10, "output": 4.40, "cache_read": 0.275}
    ]
  },
  "antigravity": {"prices": []}
}
```

Rules `Catalog` implements:
- Claude prices match by prefix of `model_key(id)` with `claude-` removed, first entry in file order wins (longer ids are listed first). One exception, so cost and the picker agree: an id newer than every known id of its family (the family rule below) takes the price of its family's newest known id, not the bare family entry (`claude-fable-5-3` costs what `fable-5-2` costs, not `fable`'s). `pub fn price_borrowed(&self, tool: Tool, model: &str) -> bool` says when that happened. Codex and Antigravity prices match the whole lowercased id minus a date suffix.
- `load` reads the override file if present; its `claude.prices`/`codex.prices`/`antigravity.prices` entries replace built-in entries with the same `match`/`id` and are added otherwise; `windows` and `names` (`{"id": "Display Name"}`, override only) merge per key; `current`/`older` in the override are ignored. An entry that fails to parse is skipped alone.
- `allowed(Claude, live)` is built-in `current` ∪ built-in `older` ∪ live ids (bare and as listed). Codex and Antigravity: live ids only.
- `models(Claude, live)`: built-in `current` in order (Tier::Current), then `older` (Tier::Older), then live ids not already listed (Tier::Older). The family rule: family is the second `-` segment (`claude-fable-5-2` → `fable`); version is the numeric segments after it. A live id newer than every known id of its family takes that family's place in `current` (Tier::Current, `borrowed: true`) and borrows that place's window and price; the model it displaced moves to the top of `older`. Names: override `names`, else live `name`, else `cost::prices::display_name`. Efforts: `claude.efforts` for every Claude model except `claude-haiku-4-5` (none).
- `models(Codex | Antigravity, live)`: the live list as given (Tier::Current), with windows from live, else data; prices from data.
- `prices_hash`: a stable hex hash (e.g. FNV-1a over the canonical JSON of all three price lists after the merge).

- [ ] **Step 1: Write the failing tests** in `crates/engine/src/catalog/tests.rs`:

```rust
#[test] fn every_current_and_older_claude_model_has_a_price() // loop built_in() current+older: price(Claude, id).is_some()
#[test] fn prices_match_todays_tables() {
    let c = Catalog::built_in();
    assert_eq!(c.price(Tool::Claude, "claude-fable-5-2").unwrap().input, 8.0);
    assert_eq!(c.price(Tool::Claude, "claude-fable-5-1").unwrap().output, 50.0);
    assert_eq!(c.price(Tool::Claude, "claude-fable-5-3").unwrap().input, 8.0); // borrows Fable 5.2's
    assert!(c.price_borrowed(Tool::Claude, "claude-fable-5-3"));
    assert_eq!(c.price(Tool::Claude, "claude-fable-4").unwrap().cache_read, 1.0); // older: the bare family entry
    assert_eq!(c.price(Tool::Codex, "gpt-5.1-2026-01-01").unwrap().input, 1.25);
    assert!(c.price(Tool::Codex, "gpt-5.6-sol").is_none()); // whole-id match, no "gpt-5" prefix
    assert!(c.price(Tool::Claude, "claude-new-9").is_none());
}
#[test] fn suffixed_ids_are_the_bare_model() // "claude-opus-5-5[1m]" and "claude-fable-5-2-20261101": same price, window, and both allowed
#[test] fn the_override_changes_a_price_but_cannot_allow_a_model() // temp override: fable-5-2 input 7.0 and current ["claude-evil-1"]: price is 7.0; allowed(Claude, &[]) lacks "claude-evil-1"
#[test] fn a_bad_override_entry_is_skipped_alone() // override with one price missing "input" and one good: the good applies, built-ins intact
#[test] fn a_newer_family_model_from_the_live_list_becomes_current_and_borrows() {
    let live = [LiveModel { id: "claude-fable-5-3".into(), name: Some("Claude Fable 5.3".into()), ..Default::default() }];
    let m = Catalog::built_in().models(Tool::Claude, &live);
    let cur: Vec<&str> = m.iter().filter(|x| x.tier == Tier::Current).map(|x| x.id.as_str()).collect();
    assert_eq!(cur, ["claude-opus-5-5", "claude-fable-5-3", "claude-sonnet-5-5", "claude-haiku-5-5"]);
    let f = m.iter().find(|x| x.id == "claude-fable-5-3").unwrap();
    assert!(f.borrowed); assert_eq!(f.price.as_ref().unwrap().input, 8.0);
    assert_eq!(m.iter().find(|x| x.tier == Tier::Older).unwrap().id, "claude-fable-5-2");
}
#[test] fn an_older_live_model_stays_older() // live "claude-fable-5" changes nothing in current
#[test] fn the_prices_hash_changes_with_a_price_and_only_then()
```

- [ ] **Step 2: Run** `PATH="$HOME/.cargo/bin:$PATH" cargo test --locked -p notchling-engine catalog` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** `crates/engine/src/catalog/mod.rs` with the interfaces above; the data file is `include_str!("../../data/catalog.json")`; `cost/prices.rs` keeps `display_name` and re-exports `model_key`.
- [ ] **Step 4: Run** the same command — Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/engine/data crates/engine/src/catalog crates/engine/src/lib.rs crates/engine/src/cost/prices.rs && git commit -m "Catalog: models, efforts, windows and prices for every tool in one data file ..."`

### Task 2: Cost tracking prices through the catalog, and reprices history

**Files:**
- Modify: `crates/engine/src/cost/prices.rs` (drop `PRICES`; `cost` takes the catalog)
- Modify: `crates/engine/src/cost/other.rs` (drop `OtherPrices::TABLE`; `OtherPrices::cost` takes the catalog and a tool)
- Modify: `crates/engine/src/cost/tracker.rs:395,477,512` (callers; unpriced Claude requests are recorded with cost 0)
- Modify: `crates/engine/src/cost/cache.rs` (`CURRENT` 1 → 2; new field `prices: String`)
- Modify: `crates/engine/src/cost/summary.rs` (new `unpriced: Vec<String>`)
- Modify: `crates/engine/src/cost/service.rs` / `Tracker` constructor (holds `Arc<Catalog>`)
- Modify: `crates/notchlingd/src/cost_ops.rs` (passes the daemon's catalog; reply gains `"unpriced": [...]` in `summary`)
- Test: `crates/engine/src/cost/sources_tests.rs`, `crates/engine/src/cost/tracker_tests.rs`, `crates/notchlingd/src/cost_ops_tests.rs`

**Interfaces:**
- Consumes: `Catalog::price`, `Catalog::prices_hash`, `Tool` (Task 1).
- Produces: `pub fn cost(catalog: &Catalog, model: &str, usage: &Value) -> Option<f64>` (None = no price; the Haiku 5.5 tier comes from `Price.over`; `speed: "fast"` still doubles); `OtherPrices::cost(catalog: &Catalog, tool: Tool, model: &str, counts: &[f64]) -> Option<f64>`; `Tracker::new(..., catalog: Arc<Catalog>)`; `Summary.unpriced: Vec<String>` and `Summary.borrowed: Vec<String>` (model ids, sorted, deduped; borrowed = priced through `Catalog::price_borrowed`).

- [ ] **Step 1: Write the failing tests:**
  - `sources_tests.rs`: rewrite the existing `prices::cost(...)` asserts to `prices::cost(&Catalog::built_in(), ...)` with the same dollar values, and add `assert_eq!(prices::cost(&c, "claude-new-9", &json!({"output_tokens": 1})), None)`.
  - `tracker_tests.rs`: `an_unpriced_claude_model_counts_tokens_and_is_listed` — a transcript line with model `claude-new-9`, 1000 output tokens: summary cost 0, its tokens counted, `summary.unpriced == ["claude-new-9"]`.
  - `tracker_tests.rs`: `a_newer_family_model_is_priced_as_borrowed` — model `claude-fable-5-3`: costed at Fable 5.2's prices, `summary.borrowed == ["claude-fable-5-3"]`.
  - `tracker_tests.rs`: `a_changed_price_reprices_days_already_counted` — scan with the built-in catalog, then with a catalog loaded from an override setting `opus-5-5` input to 8.0: the second scan's total for the same transcript doubles its input part (the cache's `prices` differs, so it re-reads everything).
- [ ] **Step 2: Run** `cargo test --locked -p notchling-engine cost` — Expected: FAIL (signature mismatch).
- [ ] **Step 3: Implement.** In `tracker.rs:395` replace the `cost <= 0.0 → return` with: `None` → record with cost 0 and push the model id into the cache's day `unpriced` set (new `DayTable<()>`-like `BTreeMap<String, BTreeSet<String>>` field `unpriced`), `Some(c)` as today. `Cache::load` treats `prices != catalog.prices_hash()` like a version mismatch (fresh cache).
- [ ] **Step 4: Run** `cargo test --locked -p notchling-engine cost && cargo test --locked -p notchling-notchlingd cost_ops` — Expected: PASS.
- [ ] **Step 5: Commit** "Cost: prices come from the catalog; a model with no price is listed instead of costing $0 quietly; a price change reprices the days already counted".

### Task 3: Claude's live model list

**Files:**
- Create: `crates/engine/src/catalog/claude_live.rs`
- Create: `crates/engine/tests/fixtures/catalog/models-list.json` (a made-up `/v1/models` body: `{"data": [{"type": "model", "id": "claude-fable-5-3", "display_name": "Claude Fable 5.3", "created_at": "2026-12-01T00:00:00Z", "max_input_tokens": 1000000}, {"type": "model", "id": "claude-opus-5-5", "display_name": "Claude Opus 5.5", "created_at": "2026-09-01T00:00:00Z"}], "has_more": false}`; no account data)
- Create: `crates/engine/src/catalog/live_cache.rs`
- Modify: `crates/engine/src/accounts/http.rs` (`HttpRequest` gains `pub headers: Vec<(&'static str, String)>`, written after the fixed headers; `get` and `post` set it empty)
- Test: `crates/engine/src/catalog/claude_live_tests.rs`, `crates/engine/src/catalog/live_cache_tests.rs`

**Interfaces:**
- Consumes: `LiveModel` (Task 1); `accounts::http::{HttpClient, HttpRequest}`, `FakeHttp`-style fake already used by `ClaudeLimits` tests; `Secret`.
- Produces:
  - `pub const MODELS_URL: &str = "https://api.anthropic.com/v1/models?limit=1000";`
  - `pub fn fetch(http: &dyn HttpClient, token: &Secret) -> Result<Vec<LiveModel>, String>` — GET with header `anthropic-version: 2023-06-01` (the OAuth beta header is already sent by the client); 200 → `parse`; anything else → `Err` naming only the status.
  - `pub fn parse(body: &[u8]) -> Vec<LiveModel>` — `data[].id` and `display_name`; `max_input_tokens` is **not** used as the window (Claude Code's own window differs: spec).
  - `pub struct LiveCache { path: PathBuf }`, `LiveCache::new(path)`, `fn get(&self, tool: Tool) -> Option<(f64 /*fetched_at secs*/, Vec<LiveModel>)>`, `fn put(&self, tool: Tool, at: f64, models: &[LiveModel])` (file 0600 JSON `{"claude": {"at", "models"}, "antigravity": {...}}`, written atomically via temp file and rename).
  - `pub fn refresh_due(at: Option<f64>, now: f64) -> bool` — true when none or older than 86 400 s.

- [ ] **Step 1: Write the failing tests:** `parses_ids_and_names_and_ignores_the_api_window` (fixture → two models, `context_window == None`); `a_401_or_a_500_or_garbage_is_an_error_and_nothing_else_is_sent` (FakeHttp answers 401, 500, `"<html>"`: `fetch` is `Err`, exactly one call each, the request carries `anthropic-version: 2023-06-01` and the bearer); `the_cache_round_trips_and_is_private` (put then get; mode 0600); `a_missing_or_broken_cache_reads_as_none`; `refresh_is_due_after_a_day`.
- [ ] **Step 2: Run** `cargo test --locked -p notchling-engine catalog` — Expected: FAIL.
- [ ] **Step 3: Implement** the three items.
- [ ] **Step 4: Run** — Expected: PASS (and `cargo test --locked -p notchling-engine accounts` still passes with the new `headers` field).
- [ ] **Step 5: Commit** "Catalog: Claude's models from Anthropic's model list, cached for a day ...".

### Task 4: The daemon serves the catalog

**Files:**
- Create: `crates/notchlingd/src/catalog_ops.rs` (an `OpGroup`)
- Create: `crates/notchlingd/src/catalog_ops_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (`ops::CATALOG_GET = "catalog.get"`, `events::CATALOG_CHANGED = "catalog.changed"` on topic `tools`)
- Modify: `crates/notchlingd/src/daemon_wiring.rs` (build one `Arc<CatalogService>`, `.with_group(catalog)`)
- Modify: `crates/notchlingd/src/agy_sink.rs` or wherever `configOptions` arrive (`agy_ops.rs`): record the Model option's values as `LiveModel`s (`id` = value, `name` = name; efforts from the Thinking Level option) with `LiveCache::put(Tool::Antigravity, ...)`, then publish `catalog.changed`.

**Interfaces:**
- Consumes: `Catalog`, `LiveCache`, `claude_live::fetch`, `refresh_due`, `codex::models::load` (Task 1, 3; existing), `ClaudeAccounts::usage_token`, the `accounts` `SwitchFlag` the limits use (`limits_sources.rs:48`).
- Produces:
  - `pub struct CatalogService` with `pub fn new(catalog: Catalog, cache: LiveCache, codex_dir: PathBuf, claude: Option<(Arc<ClaudeAccounts>, Arc<SwitchFlag>)>, http: Arc<dyn HttpClient>) -> CatalogService`, `pub fn models(&self, tool: Tool) -> Vec<ModelInfo>`, `pub fn allowed(&self, tool: Tool) -> Vec<String>`, `pub fn catalog(&self) -> &Catalog`, `pub fn refresh_claude(&self)` (off the caller's thread when due; skipped while the accounts switch is off or no active account; never renews).
  - Op `catalog.get {tool}` — agent tier, read only — replies `{"tool": "claude", "models": [ModelInfo...], "fetchedAt": <secs>|null}`; an unknown tool is `invalid`. A call when Claude's list is due starts `refresh_claude` and answers from what it has.
  - Codex models: `codex::models::load(codex_dir)` mapped to `LiveModel` (add `context_window` to `codex::models::Model`, read from the cache's `context_window`).
- [ ] **Step 1: Write the failing tests:** `catalog_get_answers_each_tool_from_its_source` (temp codex dir with a synthetic `models_cache.json` holding one model with `context_window: 272000`; Claude from a seeded `LiveCache`; Antigravity empty); `a_due_claude_list_is_fetched_once_with_the_live_token_and_never_renewed` (fake accounts with a live token; FakeHttp 200 with the fixture; second call within a day makes no request; an expired live login → no request at all); `nothing_is_fetched_while_the_accounts_switch_is_off` (FakeHttp `calls() == 0`); `an_unknown_tool_is_invalid`; `agy_options_seen_are_cached_and_announced`.
- [ ] **Step 2: Run** `cargo test --locked -p notchling-notchlingd catalog` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — Expected: PASS; `python3 -m guard rules` still passes (`PYTHONPATH=tools/guard`).
- [ ] **Step 5: Commit** "Daemon: catalog.get serves every tool's models ...".

### Task 5: The allowlist comes from the catalog

**Files:**
- Modify: `crates/notchlingd/src/control_ops.rs:20-52,217-236` (delete `PICKER_MODELS`; `flags_arg` checks `catalog.allowed(Tool::Claude)`)
- Modify: `crates/notchlingd/src/bridge_backend.rs:6,262-268` (`model_id(short)` picks the first `Tier::Current` Claude id whose family is `short`)
- Test: `crates/notchlingd/src/control_ops_tests.rs`, `crates/notchlingd/src/bridge_ops_tests.rs`

**Interfaces:**
- Consumes: `CatalogService::allowed`, `CatalogService::models` (Task 4).
- Produces: `ControlOps`/`ActingOps` hold an `Arc<CatalogService>` (pass it where `with_launcher` is wired).
- [ ] **Step 1: Write the failing tests:** `session_new_takes_a_model_the_live_list_reports` (seeded live `claude-fable-5-3`: `--model claude-fable-5-3` accepted); `session_new_refuses_a_model_only_the_override_names` (override `current: ["claude-evil-1"]` → refused, message lists the allowed ids); `the_phones_short_names_still_map` (`opus` → `claude-opus-5-5`, `fable` → the current Fable).
- [ ] **Step 2: Run** `cargo test --locked -p notchling-notchlingd control_ops bridge` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** "Daemon: the models a session may start on are the built-in ones plus the live lists' ...".

### Task 6: Chat reads the catalog

**Files:**
- Modify: `crates/app/src/chat/claude_picker.rs` (delete `MODELS`, `OLDER_MODELS`; `models(current, catalog: &[ModelInfo])`, `older_models(current, catalog)`, `efforts(current, model: Option<&ModelInfo>)`)
- Modify: `crates/app/src/chat/pickers.rs:93-125` (callers)
- Modify: `crates/app/src/chat/format.rs:76-82` (`context_window(model, catalog: &[ModelInfo]) -> Option<u64>`; `context_text` takes the catalog)
- Modify: `crates/app/src/chat/header.rs:44` (caller)
- Modify: the app's daemon client (`catalog.get` for `claude` at start and on `catalog.changed`, same pattern as `cost.summary` in `crates/app/src/accounts/wire.rs:153`), stored on `ChatView` as `catalog: HashMap<Tool, Vec<ModelInfo>>`
- Test: the existing tests in `claude_picker.rs` and `format.rs`, rewritten against `Catalog::built_in().models(Tool::Claude, &[])`

**Interfaces:**
- Consumes: `ModelInfo`, `Tier`, `Tool` (Task 1); op `catalog.get` (Task 4).
- [ ] **Step 1: Rewrite the tests:** `the_choices_come_from_the_catalog_in_order` (names `["Opus 5.5", "Fable 5.2", "Sonnet 5.5", "Haiku 5.5"]` and the nine older ones); `a_borrowed_model_says_so_in_its_tip` (live Fable 5.3: its row's tip mentions it uses Fable 5.2's price until the catalog names it); `context_only_for_known_windows` (Opus 5.5 → 400 000, Sonnet 5.5 → None); `an_empty_catalog_falls_back_to_the_built_in_one` (the app has not heard from the daemon yet).
- [ ] **Step 2: Run** `cargo test --locked -p notchling-app claude_picker format` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --locked -p notchling-app` — Expected: PASS.
- [ ] **Step 5: Commit** "Chat: the Model Picker, Effort Picker and Context Meter read the catalog ...".

### Task 7: The phone gets the model list

**Files:**
- Modify: `protocol/agentconsole.v1.schema.json` (additive: `HelloResponse` or `SessionsListResponse` gains optional `models: [{id, name}]`; `SessionNewRequest` gains optional `modelId: string`)
- Regenerate: `crates/protocol/src/generated.rs`, `apps/flutter/lib/core/protocol/generated.dart` (`python3 tools/protocol/gen.py`)
- Modify: `crates/engine/src/bridge/router.rs:20` (`MODELS` stays for the old `model` field; `modelId` is checked against the daemon's allowlist)
- Modify: `crates/notchlingd/src/bridge_backend.rs` (fills `models` with the Claude `Tier::Current` list; `modelId` wins over `model`)
- Add fixtures: `protocol/fixtures/SessionNewRequest.model-id.json` and the response carrying `models`; rewrite with `PROTOCOL_FIXTURES_WRITE=1` only if the repo's Rust fixture test supports it, else hand-write and check with the conformance test
- Test: the protocol fixture tests in `crates/protocol` and `crates/engine/src/bridge/*_tests.rs`
- Follow the `add-protocol-field` skill's steps.

- [ ] **Step 1: Write the failing tests:** `a_model_id_from_the_list_starts_that_model`; `an_unknown_model_id_is_invalid`; `an_old_phone_sending_model_opus_still_works`.
- [ ] **Step 2: Run** `cargo test --locked -p notchling-engine bridge && cargo test --locked -p notchling-protocol` — Expected: FAIL.
- [ ] **Step 3: Implement** (schema first, then gen, then code).
- [ ] **Step 4: Run** the same, plus `PYTHONPATH=tools/guard python3 -m guard lint` — Expected: PASS.
- [ ] **Step 5: Commit** "Phone: session lists carry the current models and session.new takes modelId ...".

### Task 8: Decision 0016 and the notes

**Files:**
- Create: `docs/decisions/0016-provider-interface-in-the-engine.md` (Context: the Fable 5.2 miss and the 170 branches; Decision: the spec's layering, phase order, the allowlist rule; Consequences; Status: Proposed; Date: 2026-11-11)
- Modify: `docs/decisions/0015-one-chat-interface-per-tool.md` (Status line: "Amended by 0016")
- Modify: `ARCHITECTURE.md` (a paragraph on `engine::catalog` and `catalog.get`)
- Modify: `docs/chat-composer-parity.md` (the picker rows now say "from the catalog")
- Modify: `crates/notchlingd/src/catalog_ops.rs` module doc (the op table, as other op files have)

- [ ] **Step 1:** Write the record and the notes.
- [ ] **Step 2: Run** `PYTHONPATH=tools/guard python3 -m guard lint && PYTHONPATH=tools/guard python3 -m guard rules && PYTHONPATH=tools/guard python3 -m guard claudemd` — Expected: all pass.
- [ ] **Step 3: Commit** "docs: decision 0016 ...", then `git push`.

## Out of this phase

- `crates/engine/src/delegate/agy_pick.rs` `LEVELS` stays: it parses the level suffix in agy's own model ids, it is not data to keep current.
- Codex's picker already reads Codex's cache; it moves to `catalog.get` in phase 2 with the rest of the per-tool UI.
- Phases 2 to 5 of the spec (Capabilities, Sessions, Turns, Prompts, Commands, the guard rule) get their own plans.
