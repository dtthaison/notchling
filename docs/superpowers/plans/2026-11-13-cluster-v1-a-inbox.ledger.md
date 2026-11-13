# SDD ledger — plan: docs/superpowers/plans/2026-11-13-cluster-v1-a-inbox.md

Spec: docs/superpowers/specs/2026-11-13-notchling-cluster-design.md (Notifications; Scope v1 step 0)
Branch: agent/cluster-design, start d2b9cc2

## Preflight scan

| Rows | Produces / consumes | Found |
| --- | --- | --- |
| T1→T2 | Item/Book/Change/State → persist load/save | consistent |
| T1→T3 | Book API → InboxOps | consistent |
| T2→T3 | load/save → InboxOps persists each Change | consistent |
| T3→T4 | raise/resolve/Responder → vault, limits producers | consistent; T3 builds InboxOps before vault in daemon_wiring, T4 hands it to VaultOps |
| T3/T4→T5 | wire constants, item shapes, `open` payloads → app model | consistent |
| T5→T6 | InboxModel → notify_new, notch badge | consistent |
| T3→T7 | InboxOps → bridge backend ops | consistent |
| T1 self | tests vs types | consistent |
| T2 self | store.rs schema + persist.rs | consistent |
| T3 self | `cargo test -p notchlingd` | package is `notchling-notchlingd` → ruling R1 |
| T4 self | `cargo test -p notchlingd vault_ops_inbox limits_ops` | same package name; cargo takes one filter → ruling R1 |
| T5 self | `Icon::Bell` "if missing" | missing in crates/kit/src/icon.rs; T5 adds it |
| T6 self | consistent |
| T7 self | consistent (uses add-protocol-field skill) |
| T8 self | consistent |
| Global | "No new comments" vs this branch's AGENTS.md ("short doc comments") | ruling R2 |

Ruling R1: test commands use `-p notchling-notchlingd` and one filter per run — the plan's package name is wrong — costs nothing if wrong.
Ruling R2: no new comments, doc comments included, per the maintainer's rule (agent/no-code-comments); the branch's older AGENTS.md line is superseded — if wrong, comments are easy to add back.
Ruling R3: implementers must not touch or commit `.claude/worktrees/` (untracked nested worktrees) — stage files by path only.

## Tasks
Task 1: dispatched (base d2b9cc2, sonnet high). Ruling R4: Book::raise returns Vec<Change> so the 200-cap can report the dropped item — the brief's single Change can't express it — costs a signature change in T3 if wrong.
Task 1: implemented cfe3c8b (DONE_WITH_CONCERNS: prune keys on created_ms, Item has no answered_ms; cap may evict the item just raised). Reviewer told to judge the prune key against the spec's "done items kept 7 days".
Task 1 review: Needs fixes (2 Important). Fix round 1 → resume implementer.
Ruling R5: Item gains closed_ms: Option<i64>, set by answer/expire/resolve (resolve gains now_ms); prune ages from closed_ms — spec keeps done items 7 days after done — costs a field if wrong.
Ruling R6: the cap never evicts the item being raised; an evicted open approval/request becomes Withdrawn (Updated, not Removed) so its waiter is told; notices/limits are removed as before — costs a state change if wrong.
Ruling R7: re-raising an id whose item is not open returns no changes — first answer wins — costs a retry path if wrong.
Ruling R8: AGENTS.md/feature-map notes for inbox wait for Task 8 — the plan puts docs there — costs a guard failure mid-branch if wrong.
Task 1 fix round 1: 3d6da42 (R5–R7 applied; withdrawn victim's closed_ms = raised item's created_ms, accepted).
Task 1: complete (cfe3c8b, 3d6da42; re-review Approved)
Ruling R9: persist::load takes now_ms (load(data_dir, now_ms)) so items withdrawn at load get closed_ms = now_ms and keep their 7 days — the brief's signature has no clock — costs one arg in T3 if wrong.
Ruling R10: load reads through store::open_existing (a missing db creates nothing and loads empty) — reading makes nothing, like drafts/settings — costs nothing.
Task 2: dispatched (base 3d6da42, sonnet high).
Task 2: implemented 2d40eeb (SCHEMA 2→3, Book::from_items). Ruling R11: load stays read-only (R10); Task 3's InboxOps saves an Updated for each item load withdrew (closed_ms == load's now_ms) so they age out — costs one loop in T3 if wrong.
Task 2: complete (2d40eeb; review Approved, minors parked: bad rows linger, store.rs module doc omits inbox)
Ruling R12: InboxOps gets a clock (new uses system time; a test constructor takes Arc<dyn Fn() -> i64 + Send + Sync>); resolve/answer pass it to Book — Book::resolve now needs now_ms (R5) — costs a ctor if wrong.
Ruling R13: a persist error never fails an op or drops the publish; it is logged (eprintln/the crate's log pattern) and the in-memory book stays the truth for this run — the inbox is advisory, the waiter is the authority — costs lost rows on disk errors.
Task 3: dispatched (base 2d40eeb, sonnet high). Carries R11 (save load-withdrawn items at new).
Task 3: implemented f30d5fa (Arc<dyn Room>; tick publishes Removed for pruned).
Task 3 review (opus): Needs fixes — Important 1 cross-thread publish order, Important 2 Book lock across SQLite. Fix round 1 → resume implementer.
Ruling R14: one writer thread fed by an mpsc; changes are sent under the Book lock (so channel order = Book order); the writer persists then publishes; a flush() waits for the queue (tests, close). R11's withdraw saves go through the same queue (fixes Minor 3: nothing before bind but load). — keeps the repo rule, no lock across I/O — costs async events (tests need flush) if wrong.
Parked for Task 4: responders must not block in answered() (hand off); producers must raise fresh ids (a closed id's raise drops the responder). Minor 4/5 parked.
Task 3 fix round 1: d3083f5 (R14 writer thread, flush, close drains).
Task 3: complete (f30d5fa, d3083f5; re-review Approved; parked: race test weak, flush from writer would deadlock, tick holds Arc until close).
Ruling R15: inbox item ids for producers are fresh: "vault-approval:<ticket id>", "vault-request:<ticket id>", "limit:<random or kind+ms>" — Task 3 parked note (a closed id's raise drops the responder) — costs an id format if wrong.
Ruling R16: "Open Form" is handled by the app from `open` without inbox.answer; the request item resolves when its wait ends: saved → Answered (answer Open), cancelled/timed out → Withdrawn/Expired. Cancel via inbox.answer cancels the request — keeps the form the only place a secret is typed — costs an extra answer path if wrong.
Ruling R17: Responders hand off without blocking (approvals.answer / requests.answer only set a slot) — Task 3 parked note.
Task 4: dispatched (base d3083f5, sonnet high).
Task 4: implemented b046431. Accepted: a cap-withdrawn secret item leaves its vault wait running (the wait and the floating window stay the authority; it ends by its own timeout) — costs a 200+ open-approvals edge only.
Task 4 review (opus): Needs fixes — I1 limit ids collide across announcers, I2 window/inbox race leaves item showing wrong decision, I3 tests race raise vs pending publish. Fix round 1 → resume implementer.
Ruling R18: Responder::answered returns bool; InboxOps::answer calls it under the inbox lock (never blocks; takes only Waiters' lock, which never takes the inbox's) and commits book.answer only on true, else {answered:false, state} — the inbox must never show a decision that wasn't made — costs a Task 3 interface change.
Ruling R19: also fix Minor 5 (expires_ms after the wait opens, plus slice), 6 (let Book cut limit titles), 7 (Open/Done-less answers: responder returns false for Open), 8 (limit created_ms from self.now). Minor 4 parked (load withdraws on next start).
Task 4 fix round 1: 480d420 (R18 bool responder under lock; I1, I3, minors 5/6/8).
Task 4: complete (b046431, 480d420; re-review Approved). Parked: limits_ops_tests.rs 604 lines (size warning) → Task 8 splits it; pre-existing vault_wait try_send vs timeout microsecond race (not ours).
Ruling R20 (Task 5): every daemon call (inbox.list/answer/seen) runs off the UI thread like actions.rs; row action "Open Form" (and a row body click on a vault_request) opens the existing request form locally, never inbox.answer; a row with any other `open` does nothing in v1 — costs a click target if wrong.
Task 5: dispatched (base 480d420, sonnet high).
Task 5: implemented 9a600c6. Renders blank under a locked screen; controller re-rendered with NOTCHLING_RENDER_FORCE_FRAMES=1 (inbox-ff.png, chat-inbox-ff.png): panel, bell badge 3, primary buttons, Title Case OK; Done list shows expanded in the render; bottom row cut by the panel's height (scroll?). Ruling R21: guard featuremap gap for crates/engine/src/inbox → Task 8.
Task 5 review (opus): Approved with 1 Important (vault_prompts bring_forward needs cx.activate(true) first). Fix round 1: Important 1 + Minors 1,2,3,5,6; Minor 4 parked (empty inbox while daemon away).
Task 5: complete (9a600c6, 5048c25; fix diff checked by controller: activate before windows, doc comment removed).
Ruling R22 (Task 6): notify only on inbox.changed events (never on a seed/reseed), and keep a set of notified ids for the app's life so a reconnect or re-applied event never notifies twice — feed's lost() clears the model, so before/after alone would re-notify everything — costs a set if wrong.
Ruling R23: if notchling_platform::notify has no click callback, a click does what other Notchling notifications do; opening the inbox panel on that path is best effort, not new platform API — costs a click target.
Ruling R24: render with the Rust `notchling-app --render <notch view>` (NOTCHLING_RENDER_FORCE_FRAMES=1; screen locked), not the Swift render-notch-view skill.
Task 6: dispatched (base 5048c25, sonnet high).
Task 6: implemented 4579284; controller viewed notch-inbox.png (badge "3" top right of the open notch, OK).
Ruling R25: no badge on the collapsed notch in v1 — the collapsed shape is the camera housing only; system notifications cover that state — costs a later layout change if the maintainer wants it.
Parked → Task 8: notch/view.rs 628 lines (pre-existing 618); banner click only activates the app (no platform click callback).
Task 6: complete (4579284; review Approved). Parked minors → final fix: Detached one worker thread via mpsc; apply clones model only for inbox.changed.
Ruling R26 (Task 7): a phone answer goes through the bridge backend to InboxOps::answer with answered_by "phone"; same first-answer-wins and responder bool (R18). A paired phone may answer Allow/Deny — the spec's phone continues work when the Mac is away — costs a narrower action set if wrong.
Ruling R27: the phone never opens the secret form: "Open Form" shows as "Open on Mac" (disabled hint); Cancel works — secrets are typed only on the Mac's form (R16).
Ruling R28: Flutter is at ~/flutter/bin (not on PATH); use that path.
Task 7: dispatched (base 4579284, sonnet high).
Task 7: implemented fbf905d (push uses 'event' key; answer takes itemId; test_fixtures.py reads Rust router arms; inbox.answer not behind acting — reviewer to judge).
Task 7 review (opus): Needs fixes — I1 phone inbox.answer must be gated by acting; I2 re-subscribe resends all and can drop a removal; I3 privacy.md out of date.
Ruling R29: gate every phone inbox.answer on acting (BridgeFailure ACTING_OFF), vault switch stays a second gate; list/pushes ungated — bridge_backend's own rule and 0007 — supersedes R26's silence.
Ruling R30: fix minors too (HashSet diff, refused feedback, keep items on reconnect, one primary per screen + kit font size, audit records the action, hub events in test_fixtures, Vietnamese not_found). "You answered" for another phone accepted for v1.
Task 7 fix round 1: c070c28 (R29 acting gate, I2, privacy.md, minors).
Task 7: complete (fbf905d, c070c28; re-review Approved). Parked: ACTING_OFF text mentions sessions; some daemon messages pass through in English.
Ruling R31 (Task 8): skip Step 4's install — AGENTS.md: only the maintainer installs; the final report gives the commands. Fold parked items into Task 8: split limits_ops_tests.rs (604 lines), feature-map rows for crates/engine/src/inbox and app/inbox, Detached → one worker thread via mpsc, feed apply clones the model only for inbox.changed, store.rs module doc already lists tables (leave — no-comments rule).
Task 8: dispatched (base c070c28, sonnet high).
Task 8: complete (558b072, bd4abfc; workspace 3757 pass; codex interrupt test flaky, pre-existing, untouched file). All tasks complete; final review next.
Final review (fable xhigh): Needs fixes — I1 phone can release a secret, devices.json is agent-writable, 0007 not amended; I2 sleep/clock step expires a live approval, real answer lost; I3 Mac panel never shows the answer or who gave it.
Ruling R32 (reversed 2026-11-13: the maintainer chose option (a); a paired phone may allow a secret, 0007's note of 2026-11-13): I1 → option (b) for now: the bridge refuses allow_once/allow_always on secret_approval items (Deny, Cancel, Done stay); the phone shows Allow as "Allow on Mac" disabled. Docs (ARCHITECTURE.md:152, privacy.md, vault_wait text) say so. Option (a) (amend 0007, accept phone approval) is the maintainer's call — asked in the final report. Reversible; the safe side of a security call.
Ruling R33: I2 → tick never expires an item that still has a responder; the producer owns its expiry via resolve. Test with with_clock.
Ruling R34: I3 → Row carries the result ("Allowed Once · Phone", "Denied · Mac", "Expired"); model test.
Ruling R35: minors in the same dispatch: log line on a refused system notification (no in-app banner in v1); cap done items (keep newest 300) and cut text on load; inbox.answer also requires ClientKind::App like vault.approval.answer; Waiters wait checks try_recv after close; InboxOps loads the book in start() after bind; bridge_models_tests temp folder via TempDir; AGENTS.md/ARCHITECTURE.md doc fixes. action_failed producer, source, banner click: out of plan A scope, listed in the final report.
Final fix: dispatched (opus high).
Final fix: 9df123e, 3226c58, 62e118b, ce093db (3769 pass, flutter 304). 9df123e alone fails the fixture test; 62e118b fixes it — leave as is (branch not squashed; maintainer merges).
Final re-review (fable): Ready. Residual minors parked for the maintainer (see final report): load before spawn, tie order on reload, drop guard, refused allow not audited, vault_wait header, feature-map line, cancel wording.
Ruling R36: R32 reversed by the maintainer. The bridge no longer refuses an allow on a secret_approval; InboxOps::decide takes an allow on one only from "mac" or "phone", under the inbox lock. Every phone inbox.answer is audited (taken: inbox.answer:<action>; refused: inbox.answer:refused:<reason>). The phone shows Allow Once and Always Allow again; the fixtures capture allow_once again. 0007 note, ARCHITECTURE.md, privacy.md, crates/AGENTS.md say so.
