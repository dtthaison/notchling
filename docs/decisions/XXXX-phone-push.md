# XXXX: Phone push: FCM, ids only, sent by the primary or by any Can Be Master machine while it is silent

## Context

The inbox ([cluster design](../superpowers/specs/2026-11-13-notchling-cluster-design.md), "Notifications and the inbox") tells the maintainer what needs them: a secret to allow, a join to approve, a step that was cut, the primary gone quiet, an old primary back. On the phone it works only while the app is connected to a Mac. Facts as of 2026-11-14:

- The phone has local notifications only (`flutter_local_notifications`). When the app isn't open, or the laptop sleeps, nothing reaches the phone. The cluster design's review issue 11 ("Notifications without the leader") names this: "failover happened" or "no primary" reaches nobody unless the app is connected. It asks for push or an always-connected fallback, and for notification ids that survive failover.
- With the 2026-11-14 decisions (overview, decision 9 and call 20), the primary sends, and while it is silent any reachable Can Be Master machine sends, so a primary that is down is still heard about.
- An inbox item has one id on every machine that holds it. It survives a move and a promote (`cluster.db`, plan D Task 1), and `primary_unreachable:<term>` is the same on every machine.
- A headless node never turns its `vault` switch on ([0007](0007-local-api-tiers.md)), so a key kept in the vault can't be fetched there at send time. Plan C Task 7's standing grants carry one to every Can Be Master machine.
- The phone link, and so every push, is the bridge's business; its switch is off until the maintainer turns it on (0007's switches).
- Notchling has no server ([privacy policy](../policies/privacy.md)). Any push route adds a third party.

## Decision

- **Channel.** Firebase Cloud Messaging HTTP v1 for both platforms; it carries APNs to iPhones. A Firebase project the maintainer owns (M9) holds the APNs key. Notchling sends only to `oauth2.googleapis.com` and `fcm.googleapis.com`; any other host is refused before a connection opens.
- **Payload.** Ids only, fixed text. Visible text is "Notchling needs you" (with a count when several wait) or "Your primary Mac isn't answering". The data holds a version, the urgency, the item id and the count. It never holds a session name, folder, command, secret name, node name or message text. The phone fetches details over the bridge when opened.
- **Which items.** A closed list of inbox kinds: `secret_approval`, `secret_request`, `node_join`, `paused_step`, `failover`, `primary_back` and `session_waits` ("needs you"), and `primary_unreachable` ("primary down"). Every other kind is not pushed (`no_primary`, which the phone raises for itself, `limit_notice`, `login_needed`, `notice`, `action_failed`).
- **Who sends.** The primary always. A Can Be Master machine that isn't the primary sends only while the primary is silent to it (no answer, directly or through a peer, for 60 seconds: plan E's `primary_unreachable` rule), and stops when it hears the primary again. A Worker never sends. While silent, a Standby sends the items it holds (its own and those mirrored to it by plan C Task 6) and its `primary_unreachable` item.
- **One alert.** The collapse id is the inbox item's id on every sender, so two senders give one visible alert on the phone. Each sender sends at most one alert per device per 30 seconds, and each id to a device once.
- **Tokens.** The phone registers its FCM token over the bridge (`push.register`, additive to `agentconsole.v1`; older phones never register and still work). Tokens live in `cluster.db` (`push_devices`), which every snapshot set carries (plan D Task 3), so every Can Be Master machine has them. A token Google reports as unregistered is dropped.
- **Key.** The service-account JSON is a vault entry `fcm-service-account`, released as a standing grant to every Can Be Master machine (plan C Task 7), kept in `<data>/cluster/logins/fcm.json`. The FCM client signs the JWT itself with ring (RS256) and sends through the engine's existing HTTP client. Nothing else about the key is sent anywhere.
- **Switch.** A node pushes only while its bridge switch is on and the key is present. With either missing it sends nothing and says so once.
- **Phone build.** Firebase starts only when the app is built with `--dart-define=NOTCHLING_PUSH=1`; without it the app builds and runs with no Firebase files.
- **Audit.** `push.sent:<device id>:<collapse>` and `push.failed:<device id>:<code>`; never the token or any text.

## Considered

- **APNs directly.** An Apple key on each Mac, and Android left out. Rejected: one more credential on every machine for half the phones.
- **An always-connected socket from the phone.** iOS suspends it, so it fails exactly when the app isn't open. Kept only as the phone's foreground link, which already exists.
- **UnifiedPush or ntfy.** No iOS delivery without a relay of our own.
- **Only the primary sends.** Nobody hears that the primary is gone, which is the case issue 11 is about.
- **Only the failover target sends.** No push when it is down too, and it is one machine by design while Can Be Master may be several.
- **No push.** Issue 11 stays open: nobody hears a failover, and a secret request waits unseen until the app is opened.

## Consequences

- A Google project the maintainer owns (M9) is needed, with the APNs key uploaded and a service account allowed to send. Without it nothing changes: push is off, the app and the phone work as before.
- Google sees the device tokens, when each push is sent and the item ids, never content. An item id must not be derived from a name, command or text; plan F Task 2's `payload_has_ids_only` fails if text from an item reaches a payload. APNs limits a collapse id to 64 bytes, which the sender must check against the longest id shape (`primary_unreachable:<term>`, ticket ids).
- Every Can Be Master machine holds the service-account key and the device tokens. Anyone who gets either from a Standby can send pushes that look like Notchling's, with the fixed text, to that phone. They can't read anything the phone shows, since the phone fetches details itself. A Worker holds neither.
- A Standby that can't reach the primary while the primary is fine (a partition between them) sends "Your primary Mac isn't answering". It is informational, and the phone shows one alert when several Standbys send it.
- Amends [0012](0012-remote-computers.md) and the privacy policy: a node now contacts two Google hosts, and `docs/policies/privacy.md` gains a line for it in plan F Task 7. Changes to the node tier: none, since push goes to Google, not between nodes.
- Plan F's code (Tasks 2 to 6) can start without this record, but plan E ships only after the maintainer accepts it. Without push the cluster still works.
- Still to build: the pure decisions (`engine::cluster::push`), the FCM client, the daemon's `PushOps`, `push.register` on the bridge and the phone's receiver.

## Status

Dropped. The maintainer decided on 2026-11-15 that plan F (FCM, phone push) is dropped: v1 has no phone push, and failover and promote notices stay in the inbox until the phone connects (see XXXX-cluster-failover-and-fencing). This record is kept for the reasoning and for a later v2; it was Proposed when written (2026-11-14), and nothing in it is in force. No code was built from it.

## Date

2026-11-14
