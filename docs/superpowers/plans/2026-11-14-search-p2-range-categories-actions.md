# Search P2: Range, Categories and Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ⌘⇧P works like Spotlight with text alone: results grouped by category in the user's order, category prefixes as chips, Actions that run from search, and Settings > Search rows for how far back sessions are indexed and which categories are on.

**Architecture:** Three new keys (`searchIndexRange` Ask, `searchCategories` Ask, `searchCategoryOrder` Free). The engine gets `query::category` and `query::parse` (category prefixes only; P3 adds facets) and an `IndexRange` the session index follows. The daemon reads the two search keys at each refresh pass, drops what is unchecked, and answers `search.query` with `corpora`, `disabled` and `dropped`. The app matches Settings, Actions and Inbox itself, at once, and groups everything.

**Tech Stack:** Rust; rusqlite FTS5; GPUI 0.2.2 and `crates/kit`; `crates/locale`.

**Spec:** `docs/superpowers/specs/2026-11-13-hybrid-search-design.md` sections 5 ("Categories and range"), 7, 8 (rows 3 and 4), 9 (`search.query`'s `corpora`, `disabled`, `dropped`; `embed.refresh`), 10, 12 P2; answers 2 and 5. Plan set: `2026-11-14-search-plans-index.md` (calls 1, 10, 11).

**Branch:** `agent/search-p2-range-categories`, from the integration branch after P1's first pull request merged.

**Depends on:** P1 (`query::fold`, `query::matcher`, the app's settings registry `sheets::settings_entries`); P1 Task 8 (`KeySpec::effect`); L1 for words.

## Global Constraints

- No comments in code; files under 1,000 lines (600 warns); tests in `*_tests.rs`.
- Local API additive only: `search.query` with only `{text, limit}` answers exactly as today (`pages`, `sessions`, `indexing`) plus the new keys; a client that ignores them sees no change.
- No file I/O or daemon call in a model compute or draw; daemon calls off the UI thread (`actions.rs`, the `Pacer`).
- Actions run only in the app, through the same code as the menu; `search.query` never lists or runs one.
- Words in `locales/en/mac/settings.ftl` (`settings-…`) and `locales/en/mac/search.ftl` (`search-…`); `python3 tools/locale/gen.py`; Title Case.
- Every new key has a 0020 class, title, about and why on its line, and an Ask key an `effect`.
- Stage by path; no `git add -A`.

## Review Focus

1. Sessions unchecked: an agent's `search.query` gets no sessions and `"disabled": ["sessions"]`, and `search.db` holds no session rows after the next pass. (Task 4 test `sessions_off_answers_disabled_and_empties_the_index`.)
2. The range lowered from Everything to 14 days while a refresh is running: the running pass finishes, the next one drops what is older; nothing older is shown after it. (Task 3 test `lowering_the_range_drops_older_rows_next_pass`.)
3. A stored `searchCategoryOrder` missing a newer id or holding an unknown one: the missing id takes its default place, the unknown is ignored. (Task 2 test `order_fills_missing_and_drops_unknown`.)
4. An Action whose menu item is disabled (Pair a Phone with the bridge off) shows disabled and Return does nothing. (Task 5 test `a_disabled_action_does_not_run`.)
5. Typing `s:` alone shows all of Settings with no cap, and Backspace in the empty field removes the chip. (Task 6 test `a_chip_alone_lists_its_whole_category`.)

---

### Task 1: The three keys

**Zone:** maintainer (`crates/engine/src/settings/**`). CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Modify: `crates/engine/src/settings/keys.rs` (`Fallback::List(&'static [&'static str])`; `default_value` returns it as a JSON array; `accepts` checks a `StringList` against `allowed` when `allowed` is not empty, and refuses duplicates)
- Test: `crates/engine/src/settings/keys_search_tests.rs`

**Interfaces:**
- Produces, verbatim:
  - `searchIndexRange`: `choice("searchIndexRange", "all", &["14d", "30d", "all"], false, …)`, Ask, title "Index sessions", about "How far back sessions are searched: the last 14 days, the last 30 days, or everything on disk", why "it decides which of your sessions agents can find", effect "Changes how far back sessions are searched. A shorter range removes older sessions from search; a longer one indexes them in the background."
  - `searchCategories`: `StringList`, allowed and default on `["sessions", "settings", "pages", "inbox", "actions"]` out of the eight ids, Ask, title "Search categories", about "Which kinds of things Search finds", why "off removes a category from the index, for agents too", effect "Turns a search category on or off. Off removes it from search and from the index, for agents too."
  - `searchCategoryOrder`: `StringList`, default `["sessions", "settings", "pages", "inbox", "actions", "activity", "vault", "machines"]`, Free, title "Search category order", about "The order Search groups its results in", why "a display preference".
  - All eight ids are the `allowed` list of both list keys.

- [ ] **Step 1: Write the failing tests:** `the_search_keys_have_their_classes` (Ask, Ask, Free; effects non-empty); `categories_refuse_unknown_and_duplicate_ids`; `defaults_are_the_specs` (the two lists above).
- [ ] **Step 2: Run** `cargo test -p notchling-engine settings::keys_search`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-engine settings::`. Expected: PASS (0020's every-key test included).
- [ ] **Step 5: Commit** `settings: searchIndexRange, searchCategories and searchCategoryOrder`.

### Task 2: `query::category` and `query::parse`

**Zone:** auto (`crates/engine/src/search/**`). CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/query/category.rs`, `crates/engine/src/search/query/parse.rs`
- Test: `crates/engine/src/search/query/parse_tests.rs`, golden `crates/engine/src/search/query/parse_golden.json` (new)

**Interfaces:**
- Produces:
  - `pub enum Category { Sessions, Settings, Pages, Inbox, Actions, Activity, Vault, Machines }` with `ALL` in that order, `fn id(self) -> &'static str` (`"vault"` for Vault Names), `fn from_id(&str) -> Option<Category>`, `fn default_on(self) -> bool`, `fn cap(self) -> usize` (8 for Sessions, 4 else), `fn in_daemon(self) -> bool` (Sessions, Pages, Activity, Vault, Machines; Settings's vector lists come in P6), `fn app_tier(self) -> bool` (Activity, Vault, Machines).
  - `pub fn order(stored: &[String]) -> Vec<Category>` and `pub fn on(stored: &[String]) -> Vec<Category>`.
  - `pub enum Chip { Category(Category) }` (P3 adds facet and time chips)
  - `pub struct Parsed { pub chips: Vec<Chip>, pub words: Vec<String>, pub rest: String }`
  - `pub fn parse(text: &str) -> Parsed`: a leading token among `s:`/`settings:`, `p:`/`pages:`, `c:`/`sessions:`, `i:`/`inbox:`, `a:`/`>`/`actions:`, `v:`/`vault:`, `m:`/`machines:`, `activity:` (case-insensitive) becomes a chip; `rest` is the text left; `words` is `fold::words(rest)`.

- [ ] **Step 1: Write the failing tests:** `parse_goldens` (each case in `parse_golden.json`: input, chips by id, words; at least `s: dark`, `>new`, `pages: deploy notes`, `S:`, `x: y` (not a chip), `a:b`); `order_fills_missing_and_drops_unknown` (`["pages", "bogus"]` → Pages first, then the rest in default order); `ids_match_the_keys_allowed_list` (against `spec("searchCategories").allowed`).
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::query::parse`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: categories and the prefix parser`.

### Task 3: The session index follows a range

**Zone:** auto. CAP/WEAK: none. Existing tests edited: none (the engine's default stays 14 days, call 11).

**Files:**
- Create: `crates/engine/src/search/range.rs`
- Modify: `crates/engine/src/search/index/mod.rs` (`State` holds `range: IndexRange`; `pub fn set_range(&self, range: IndexRange)`; `pub fn clear(&self) -> Result<(), String>`), `crates/engine/src/search/index/scan.rs` (`claude` and `codex` take `window: Option<f64>`)
- Test: `crates/engine/src/search/index/range_tests.rs`

**Interfaces:**
- Produces: `pub enum IndexRange { Days14, Days30, All }` with `fn from_setting(s: &str) -> IndexRange` (unknown → `All`), `fn window_secs(self) -> Option<f64>`, `impl Default` → `Days14`. `SessionIndex::clear` empties memory and `search.db`.

- [ ] **Step 1: Write the failing tests:** `all_reads_files_older_than_14_days`; `lowering_the_range_drops_older_rows_next_pass` (refresh with `All`, set `Days14`, refresh: the old key is gone from `search()` and from a reopened db); `raising_reads_older_newest_first` (the `read` order of the second pass); `clear_empties_memory_and_disk`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::index::range`. Expected: FAIL.
- [ ] **Step 3: Implement.** The Codex day filter uses the same window; `None` skips it.
- [ ] **Step 4: Run** `cargo test -p notchling-engine search::`. Expected: PASS.
- [ ] **Step 5: Commit** `search: the session index follows an index range`.

### Task 4: The daemon: settings, `corpora`, `disabled`, `dropped`, `embed.refresh`

**Zone:** maintainer (`crates/notchlingd/**`, `crates/local-api/**`). CAP/WEAK: none expected. Existing tests edited: none; `pages_ops_tests.rs` keeps passing unchanged.

**Files:**
- Modify: `crates/local-api/src/wire.rs` (`ops::EMBED_REFRESH = "embed.refresh"`), `crates/notchlingd/src/pages_ops.rs` (`PagesOps::with_search_settings`, `search` reads `corpora`, `poke`), `crates/notchlingd/src/daemon_wiring.rs` (reads the store; routes `embed.refresh`), `crates/notchlingd/src/settings_ops.rs` (after an applied `searchIndexRange` or `searchCategories`, call the poke), `docs/local-api-ops.md`
- Test: `crates/notchlingd/src/pages_ops_corpora_tests.rs`

**Interfaces:**
- Consumes: `IndexRange`, `SessionIndex::{set_range, clear}` (Task 3); `Category` (Task 2).
- Produces:
  - `pub struct SearchSettings { pub range: IndexRange, pub on: Vec<Category> }`; `PagesOps::with_search_settings(read: Arc<dyn Fn() -> SearchSettings + Send + Sync>)`; the default reads nothing and uses `all` and the default categories.
  - At each refresh pass: `set_range`; Sessions off → `clear()` and no refresh.
  - `search.query` args add `corpora?: [id]` (default `["pages", "sessions"]`). Reply adds `disabled: [id]` (asked and off, for every caller) and `dropped: [id]` (`actions`, `inbox`, `settings` until P6, and app-tier ids asked by an agent). An omitted `corpora` keeps today's reply shape plus `"disabled": []`, `"dropped": []`.
  - `embed.refresh {}` → `{}`, `OpSpec::APP`: re-reads the settings and starts a pass now.

- [ ] **Step 1: Write the failing tests:** `sessions_off_answers_disabled_and_empties_the_index`; `an_agent_asking_for_actions_or_inbox_gets_them_dropped`; `an_agent_asking_for_vault_gets_it_dropped`; `no_corpora_answers_as_before` (the keys of the reply are today's plus `disabled` and `dropped`, both empty); `embed_refresh_is_app_tier`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd pages_ops_corpora`. Expected: FAIL.
- [ ] **Step 3: Implement.** The store read runs on the refresh thread, never under the index lock.
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: search follows its range and categories; disabled and dropped`.

### Task 5: The Actions registry

**Zone:** auto (`crates/app/**`); words in `locales/en/mac/search.ftl` (auto after L1). CAP/WEAK: none expected (handlers call existing openers). Existing tests edited: none.

**Files:**
- Create: `crates/app/src/search/actions.rs`
- Test: `crates/app/src/search/actions_tests.rs`
- Modify: `crates/app/src/menu.rs` (`Command` gains `ToggleAwake`, `NewSession`, `OpenPages`, `OpenVault`, `PairPhone`, `ToggleNotch`), `crates/app/src/mac/shell.rs` (`handle` runs each through the existing openers: Chat's New Session sheet, the pages window, the vault sheet, the pair prompt, the `hideNotch` write the sheet makes), `locales/en/mac/search.ftl`

**Interfaces:**
- Produces:
  - `pub struct Action { pub id: &'static str, pub message: &'static str, pub aliases: &'static [&'static str], pub kind: &'static str, pub command: Command }`
  - `pub const ACTIONS: &[Action]`: `toggle-awake` (Toggle Keep Awake), `awake-while-working` (Keep Awake While Agents Work), `new-session`, `open-chat`, `open-settings`, `switch-account` (`Command::Accounts`, as the menu), `open-accounts`, `open-pages`, `open-vault` (Open Secret Vault), `pair-phone` (Pair a Phone), `toggle-notch` (Show or Hide the Notch). Messages `search-action-<id>`.
  - `pub struct ActionState { pub signed_in: bool, pub bridge_on: bool, pub vault_on: bool }` and `pub fn enabled(action: &Action, state: &ActionState) -> bool` (`pair-phone` needs the bridge, `open-vault` the vault switch, `switch-account` a signed-in account).

- [ ] **Step 1: Write the failing tests:** `every_action_has_a_message_and_a_command` (`t::lookup(message)` non-empty; ids unique); `switch_account_opens_the_chooser` (`Command::Accounts`); `a_disabled_action_does_not_run`.
- [ ] **Step 2: Run** `cargo test -p notchling-app search::actions`. Expected: FAIL.
- [ ] **Step 3: Implement**, write the words, run `python3 tools/locale/gen.py`.
- [ ] **Step 4: Run** the tests. Expected: PASS.
- [ ] **Step 5: Commit** `search: Actions registry, run through the menu's commands`.

### Task 6: ⌘⇧P grouped by category, with chips

**Zone:** auto. CAP/WEAK: none. Existing tests edited: `crates/app/src/search/model_tests.rs` where a test pins the old section order (Pages, Vault, Sessions); the pull request lists each one changed. The guard does not trip on in-source tests; the reviewer checks them.

**Files:**
- Create: `crates/app/src/search/groups.rs` (pure), `crates/app/src/search/local.rs` (pure: Settings, Actions and Inbox matched with `query::matcher`)
- Test: `crates/app/src/search/groups_tests.rs`, `crates/app/src/search/local_tests.rs`
- Modify: `crates/app/src/search/model.rs` (`Section` becomes `Category`; `Item` gains `Setting(Found)`, `Action(&'static Action)`, `Inbox(inbox::model::Row)`, `More(Category, usize)`), `crates/app/src/search/wire.rs` (`Call::query` sends `corpora`; parse `disabled`, `dropped`), `crates/app/src/search/view.rs`, `crates/app/src/search/rows.rs` (chips as pills with ×; Backspace in an empty field removes the last), `crates/app/src/search/renders.rs` (renders `search-panel-groups`, `search-panel-chip`)

**Interfaces:**
- Consumes: `query::parse`, `Category::{order, on, cap}`; `SettingsSearch` (P1); `ACTIONS`, `enabled`; `InboxFeed::snapshot` (`crates/app/src/inbox/feed.rs`).
- Produces: `pub fn group(items: Vec<Item>, order: &[Category], only: Option<Category>) -> Vec<Item>` (per category up to `cap()`, then `Item::More(category, hidden)`; with `only`, no cap). Return: Session opens it; Setting opens Settings at the row (P1's `Jump`); Page its window; Inbox opens Chat's inbox panel at the id; Action runs its `Command` when enabled; More narrows to that category.

- [ ] **Step 1: Write the failing tests:** `groups_follow_the_users_order`; `each_group_caps_then_offers_more`; `a_chip_alone_lists_its_whole_category`; `unchecked_categories_are_not_matched_locally`; `inbox_rows_match_by_title_and_detail`.
- [ ] **Step 2: Run** `cargo test -p notchling-app search::`. Expected: FAIL.
- [ ] **Step 3: Implement.** The local matches run on the keystroke with no daemon; the daemon's categories arrive through the `Pacer` and merge by category.
- [ ] **Step 4: Run** the tests; render `NOTCHLING_RENDER_FORCE_FRAMES=1 target/release/notchling-app --render search-panel-groups --out /tmp/g.png` and `search-panel-chip`; look at them in light and dark.
- [ ] **Step 5: Commit** `search: ⌘⇧P grouped by category with prefix chips`.

### Task 7: Settings > Search rows

**Zone:** auto (`crates/app/**`, `locales/en/mac/settings.ftl`). CAP/WEAK: none. Existing tests edited: `crates/app/src/sheets/settings_entries_tests.rs`'s committed id list gains `searchIndexRange`, `searchCategories`, `searchCategoryOrder` (in-source; listed in the pull request).

**Files:**
- Create: `crates/app/src/sheets/search_rows.rs` (pure), `crates/app/src/sheets/search_rows_view.rs` (macOS)
- Test: `crates/app/src/sheets/search_rows_tests.rs`
- Modify: `crates/app/src/sheets/settings.rs` (the Search section gets Index Sessions, a `Field::Choice` with Last 14 Days, Last 30 Days, Everything on Disk; and Categories, a new `Field::Categories(Vec<(Category, bool)>)`), `crates/app/src/sheets/settings_view.rs` (after the sheet writes either key, it sends `embed.refresh` off the UI thread), `crates/app/src/sheets/settings_entries.rs` (three entries), `locales/en/mac/settings.ftl` (`settings-search-index-range`, `-about`, `-words`, and choices `-recent` (14d), `-month` (30d), `-all`, since a key word must start with a letter; `settings-search-categories` and `settings-search-category-order` with theirs)

**Interfaces:**
- Produces: `pub fn toggle(rows: &[(Category, bool)], id: Category) -> Vec<String>` (the new `searchCategories`), `pub fn move_row(rows: &[(Category, bool)], id: Category, by: i32) -> Vec<String>` (the new `searchCategoryOrder`); ⌥↑ and ⌥↓ call `move_row` on the focused row; the drag handle calls it with the drop offset.

- [ ] **Step 1: Write the failing tests:** `toggle_writes_only_categories`; `move_writes_only_order`; `the_search_section_shows_range_and_categories` (Activity and Machines rows absent until P8, call 10); `the_registry_finds_the_new_rows` ("index" → `searchIndexRange`).
- [ ] **Step 2: Run** `cargo test -p notchling-app sheets::search_rows`. Expected: FAIL.
- [ ] **Step 3: Implement**, words, `python3 tools/locale/gen.py`.
- [ ] **Step 4: Run** the tests and `--render settings` (light, dark); look at it.
- [ ] **Step 5: Commit** `settings: Index Sessions and Categories rows`.

### Task 8: Notes and the checks

**Zone:** maintainer (`crates/AGENTS.md`, `crates/app/AGENTS.md`, `crates/notchlingd/AGENTS.md`, `docs/feature-map.md`).

- [ ] **Step 1:** Write the notes: categories and range (who reads them, when rows are dropped), Actions run only in the app, `embed.refresh`, `search.query`'s new keys; the feature-map Search rows name `query/`, `range.rs`, `actions.rs`, `groups.rs`, `local.rs`.
- [ ] **Step 2: Run** `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `python3 tools/strip_comments.py --check`, `PYTHONPATH=tools/guard python3 -m guard lint`, `python3 -m guard featuremap`, `python3 tools/locale/gen.py --check`. Expected: all clean.
- [ ] **Step 3: Commit** `docs: search categories, range and Actions`, and open the pull request. It lists every literal not yet in Fluent (none expected) and every existing in-source test it changed.
