# Cluster v1, plan F: Phone Push Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The phone hears about what needs the maintainer (a secret to allow, a join to approve, the home gone quiet, a cut step to check) even when the app isn't open and the laptop is asleep, through push notifications that carry no names, commands or text, sent by the home, or by a Standby when the home is gone.

**Architecture:** Firebase Cloud Messaging HTTP v1 (which carries APNs to iPhones). A pure `engine::cluster::push` decides which inbox events are worth a push, coalesces them per device and builds a payload of ids only; a small FCM client signs a service-account JWT with ring and sends through the engine's existing `HttpClient`. The phone registers its FCM token over the bridge (`push.register`, additive); tokens live in `cluster.db`, so they move with the home. The service-account key reaches every node that may push as a standing grant (plan C), never through the vault switch on a headless node.

**Tech Stack:** Rust (engine, notchlingd, local-api), ring (RS256), the engine's `providers::http::HttpClient`, `agentconsole.v1` (additive), Flutter with `firebase_core` and `firebase_messaging` behind a build flag.

**Spec:** `docs/superpowers/specs/2026-11-13-notchling-cluster-design.md`: "Scope: v1 first" (Notifications: "phone push decided before Promote ships"), review issue 11 (notifications without the leader; ids must survive failover), "Notifications and the inbox". Task 1 can be written any time; Tasks 2 to 7 need plan C's inbox federation (C Task 6) and standing grants (C Task 7); plan E's items are pushed once E lands (the kind table already names them).

## Global Constraints

- Plan B's constraints hold.
- A push payload holds no session name, folder, command, secret name, node name or message text. Visible text is fixed: "Notchling needs you" (with a count when several wait) or "Your home Mac isn't answering". The phone fetches details over the bridge when opened.
- Pushes go out only while the bridge switch is on on the sending node (the phone link is the bridge's business; decision 0007's switches).
- A push collapse id is the inbox item's id, which survives a move and a promote (`cluster.db`, plan D Task 1).
- Only two hosts are ever contacted: `oauth2.googleapis.com` and `fcm.googleapis.com`; anything else is refused before a connection opens.
- No test reaches the network: `FakeHttp` only. The test RSA key is a generated, test-only file (precedent: `crates/engine/src/bridge/net/testdata/access-key-*.pem`), never a real service account.
- The Flutter app builds and runs with no Firebase files unless `--dart-define=NOTCHLING_PUSH=1`.

## Review Focus

1. A payload never carries text from the item. (Task 2 `payload_has_ids_only` with marker strings in every item field.)
2. A burst of items doesn't flood the phone: one alert per 30 s per device, each id at most once. (Task 2 `coalesces_to_one_alert_per_30s`, `same_id_never_pushed_twice`.)
3. The FCM client talks only to its two hosts and signs correctly. (Task 3 `refuses_other_hosts`, `jwt_verifies_with_the_public_key`.)
4. With the home down, the Standby still pushes "home isn't answering", once. (Task 4 `standby_pushes_home_unreachable_once`.)
5. Bridge switch off: nothing is sent. (Task 4 `bridge_off_sends_nothing`.)

---

### Task 1: Decision record 0025, phone push

**Zone:** maintainer. No CAP.

**Files:**
- Create: `docs/decisions/0025-phone-push.md` (Proposed), index row in `docs/decisions/README.md`

- [ ] **Step 1: Write the record.** Decision: FCM HTTP v1 for both platforms; ids-only payloads with fixed text; sent by the home, and by a Standby for `home_unreachable` only; tokens in `cluster.db`; the service-account key as a standing grant; bridge switch required. Considered: APNs directly (an Apple key per Mac, Android left out), an always-connected socket (iOS suspends it), UnifiedPush or ntfy (no iOS), none (the spec's issue 11 stays open: nobody hears a failover). Consequences: a Google project the maintainer owns (M9); Google sees device tokens, timing and item ids, never content; `docs/policies/privacy.md` gains a line in Task 7.
- [ ] **Step 2: Commit** `docs: decision 0025, phone push (proposed)`.
- [ ] **Gate:** plan E ships only after the maintainer accepts this record.

### Task 2: The push decisions, pure

**Zone:** maintainer. No CAP.

**Files:**
- Create: `crates/engine/src/cluster/push.rs`
- Test: `crates/engine/src/cluster/push_tests.rs`
- Modify: `crates/engine/src/cluster/persist.rs` (table `push_devices(device PRIMARY KEY, token, platform, added_ms, last_ok_ms, failures)`), `crates/engine/src/store_class.rs`

**Interfaces:**
- Produces:
  - `pub fn worth_pushing(kind: &str) -> Option<Urgency>`; `pub enum Urgency { NeedsYou, HomeDown }`: `secret_approval`, `secret_request`, `node_join`, `paused_step`, `failover` → NeedsYou; `home_unreachable` → HomeDown; every other kind (`limit_notice`, `login_needed`, `notice`, `action_failed`) → None
  - `pub struct Device { pub id: String, pub token: String, pub platform: Platform }`, `pub enum Platform { Ios, Android }`
  - `pub struct Payload { pub token: String, pub collapse: String, pub title: &'static str, pub body: String, pub data: BTreeMap<String, String> }` (data: `v`, `kind` as the urgency, `id`, `count`)
  - `pub struct Pusher { last_alert: BTreeMap<String, i64>, sent: BTreeSet<(String, String)> }`: `fn on_item(&mut self, devices: &[Device], item_id: &str, kind: &str, open_count: usize, now_ms: i64) -> Vec<Payload>` (one per device not alerted in the last `COALESCE_MS = 30_000`; an id already sent to a device is skipped; `sent` pruned after 7 days)
  - `pub fn on_result(code: u16, body: &str) -> DeviceFate` (`Keep`, `Drop` on `UNREGISTERED` or 404, `Retry` on 429/5xx)
- [ ] **Step 1: Write the failing tests:** `only_listed_kinds_push`, `payload_has_ids_only` (an item with `NL-MARKER` in title, detail, session and source: no payload field holds it), `coalesces_to_one_alert_per_30s`, `same_id_never_pushed_twice`, `count_shows_open_items`, `unregistered_token_is_dropped`, `devices_round_trip_in_cluster_db`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::push`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `engine: which inbox items push, and how often`.

### Task 3: The FCM client

**Zone:** maintainer. CAP-NETWORK.

**Files:**
- Create: `crates/engine/src/cluster/fcm.rs`, `crates/engine/src/cluster/testdata/fcm-test-only.pk8.pem` (an RSA 2048 key generated once with `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048`, never a real account), `crates/engine/src/cluster/testdata/fcm-test-only.pub.der`
- Test: `crates/engine/src/cluster/fcm_tests.rs`

**Interfaces:**
- Consumes: `providers::http::{HttpClient, HttpRequest, HttpResponse, FakeHttp}`.
- Produces:
  - `pub struct ServiceAccount { pub project_id: String, pub client_email: String, key: ring::signature::RsaKeyPair }` (`Debug` names the project only); `pub fn parse_account(json: &str) -> Result<ServiceAccount, String>` (`token_uri` must be `https://oauth2.googleapis.com/token`)
  - `pub fn jwt(sa: &ServiceAccount, now_s: i64) -> Result<String, String>` (RS256; `iss`, `scope` `https://www.googleapis.com/auth/firebase.messaging`, `aud` the token URI, `iat`, `exp` = `iat + 3600`)
  - `pub struct Fcm { sa: ServiceAccount, http: Arc<dyn HttpClient>, token: Mutex<Option<(String, i64)>> }`: `fn send(&self, p: &Payload, now_s: i64) -> Result<u16, String>` (gets or reuses the access token until 60 s before expiry, never holding the lock across the request; posts `messages:send` with `apns` headers `apns-collapse-id` = collapse and `apns-priority` 10, `aps.alert` title and body, `thread-id` `notchling`; `android.collapse_key` and high priority)
  - `pub const HOSTS: [&str; 2] = ["oauth2.googleapis.com", "fcm.googleapis.com"]`; any other URL is refused before the client is called
- [ ] **Step 1: Write the failing tests:** `jwt_verifies_with_the_public_key`, `token_is_reused_until_near_expiry`, `send_posts_collapse_and_ids`, `refuses_other_hosts`, `bad_token_uri_is_refused`, `account_debug_hides_key`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::fcm`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `engine: an FCM client signed with ring`.

### Task 4: Pushing from the daemon

**Zone:** maintainer. CAP-NETWORK.

**Files:**
- Create: `crates/notchlingd/src/push_ops.rs`
- Test: `crates/notchlingd/src/push_ops_tests.rs`
- Modify: `crates/notchlingd/src/daemon_wiring.rs`, `crates/notchlingd/src/cluster_logins.rs` (the `fcm-service-account` vault entry as a standing grant to the home and each Standby, `<data>/cluster/logins/fcm.json`)

**Interfaces:**
- Produces: `PushOps` follows the inbox (`inbox.changed`, mirrored items included) and, on a Standby, its own `home_unreachable`; sends on one worker thread in order; retries `Retry` results after 5, 30 and 120 s; drops `Drop` devices; audits `push.sent:<device id>:<collapse>` and `push.failed:<device id>:<code>`, never the token or text. Sends only on the home, or on a Standby for `home_unreachable`, and only with the bridge switch on and the standing key present.
- [ ] **Step 1: Write the failing tests:** `home_pushes_needs_you_items`, `worker_never_pushes`, `standby_pushes_home_unreachable_once`, `bridge_off_sends_nothing`, `no_key_sends_nothing_and_says_so_once`, `retry_then_give_up`, `audit_has_no_token`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib push_ops`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: push inbox items to paired phones`.

### Task 5: `push.register` on the bridge

**Zone:** maintainer (`protocol/`, `crates/engine/src/bridge/`). No CAP.

**Files:** follow `.claude/skills/add-protocol-field/SKILL.md`:
- Modify: `protocol/agentconsole.v1.schema.json` (ops `push.register {token, platform}` and `push.unregister {}`, additive, described "a paired phone's push token; the Mac stores it with the device"), fixtures, regenerate; `crates/engine/src/bridge/router.rs` (a new arm), `crates/engine/src/bridge/live.rs` and `crates/notchlingd/src/bridge_backend.rs` (stored in `cluster.db` by device id; a client node forwards it to the home)
- Test: `crates/engine/src/bridge/router_push_tests.rs`
- [ ] **Step 1: Write the failing tests:** `register_stores_token_by_device`, `unregister_removes_it`, `unpaired_connection_cannot_register`, `client_node_forwards_to_home`, `old_phone_never_registers_and_still_works`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib bridge::router_push_tests` and `python3 tools/protocol/gen.py --check`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `bridge: phones register for push (additive)`.

### Task 6: The phone receives pushes

**Zone:** maintainer for `apps/flutter/pubspec.yaml`, `ios/`, `android/`; auto for `apps/flutter/lib/features/inbox/` and new tests. No CAP.

**Files:**
- Modify: `apps/flutter/pubspec.yaml` (`firebase_core`, `firebase_messaging`, pinned), `apps/flutter/lib/main.dart` (Firebase starts only when `const bool.fromEnvironment('NOTCHLING_PUSH')`), `apps/flutter/ios/Runner/` (push capability and background mode), `apps/flutter/android/app/build.gradle`
- Create: `apps/flutter/lib/features/inbox/push.dart` (ask permission once, from the inbox screen, with a line saying what it's for; register the token after pairing and on refresh; on a tap open the inbox and reconnect)
- Test: `apps/flutter/test/push_test.dart` (a fake messaging adapter)
- [ ] **Step 1: Write the failing tests:** `no_firebase_without_flag`, `registers_token_after_pairing`, `token_refresh_registers_again`, `tap_opens_inbox`.
- [ ] **Step 2: Run** `~/flutter/bin/flutter test test/push_test.dart` in `apps/flutter`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it, `~/flutter/bin/flutter test` and `~/flutter/bin/dart analyze`. Expected: PASS.
- [ ] **Step 5: Commit** `phone: receive push, behind NOTCHLING_PUSH`.

### Task 7: Docs

**Zone:** maintainer for policies and AGENTS; auto for `docs/push-setup.md`. No CAP.

**Files:**
- Create: `docs/push-setup.md` (the maintainer's steps M9: a Firebase project, the APNs key upload, `GoogleService-Info.plist` and `google-services.json` kept out of git, the service-account JSON saved in the vault as `fcm-service-account`, building with `--dart-define=NOTCHLING_PUSH=1`)
- Modify: `docs/policies/privacy.md` (what Google sees), `apps/flutter/AGENTS.md`, `crates/notchlingd/AGENTS.md`, `docs/local-api-ops.md`, `docs/feature-map.md`, `CHANGELOG.md`, `.gitignore` (the two Firebase config files; maintainer zone)
- [ ] **Step 1: Write.**
- [ ] **Step 2: Run** the full checks, plus `~/flutter/bin/flutter test` and `~/flutter/bin/dart analyze`. Expected: PASS.
- [ ] **Step 3: Commit** `docs: phone push`.

### Checkpoint: the live check (maintainer only)

Gated by M9. With the phone locked: request a secret marked Ask me first from a session; a push arrives reading "Notchling needs you"; tapping it opens the inbox with the request. Close the laptop with a Standby set: within about two minutes a push reads "Your home Mac isn't answering".
