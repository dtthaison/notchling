# XXXX: A provider chooses how Notchling talks to it

## Context

Notchling sends text to a tool in a different way for each one: Claude Code through the session's terminal, Codex through `codex exec`, Antigravity through `agy acp` (a process the daemon starts and drives). Each way grew inside its tool's code. Nothing says what a way can do (reply to an existing session, start one, work from the phone, be stopped), so a caller learns it by failing.

Google's Antigravity FAQ says that wrapping `agy` inside third-party apps, or automating account rotation, "is not supported under consumer plans". Replying through `agy acp` is that wrapping. The maintainer keeps it as the default for now, and wants a way to reach Antigravity that does not wrap anything.

Facts that bound the change:

- The installed Antigravity app handles `antigravity://open?prompt=<text>&project=<id>`: it comes to the front, opens its home screen and puts the text in the message box as a draft. Nothing is sent; the user presses Send there. No link opens an existing conversation or sends one. The app's local 127.0.0.1 API is not used.
- Antigravity Remote Control, in the browser, opens an existing conversation at `https://antigravity.google.com/r/<installation_uuid>-v2?p=c%2F<conversation id>`, behind Google's account chooser (`https://accounts.google.com/AccountChooser?Email=<email>&continue=<the page, encoded>`). It carries no text.
- The Antigravity session id is the conversation id and the folder name under `brain/`, which Notchling already follows. The installation id is the `installation_uuid` line of `antigravity_state.pbtxt` in the Antigravity data folder (a sibling of the `agy` folder Notchling reads). Notchling cannot tell whether Remote Control is on.
- Only a link that is opened can be a way that sends nothing itself, so it is the user's hands, not the daemon's, that finish the message.

## Decision

- A **channel** is one way to reach a tool (`contract::channel::Channel`): an id, a title, a `Mode` and four `Abilities`.
  - `Mode::Sends`: Notchling delivers the text. `Mode::NewDraft`: another app opens with the text as a new conversation draft and the user sends there. `Mode::ExistingSession`: another app or page opens the existing session and the user sends there. `Mode::user_sends()` is true for the last two.
  - `Abilities`: `reply` (to an existing session), `start` (a new session), `phone` (works from the phone), `stop`. `Ability::capability()` ties each to the provider contract (`turn.send`, `session.new`, `turn.interrupt`), and a test fails if a channel claims an ability on a capability its tool marks Not applicable.
  - `contract::channel::refusal(channel, ability)` is the one plain refusal: "Antigravity can't be used from the phone while Notchling talks to it through ...".
- A provider lists its channels (`Provider::channels()`, `Registry::channels(tool)`), first one the default. `contract/channels.rs` declares them:
  - Claude Code: the session's terminal. Codex: `codex exec`. Both are declarations only: their delivery code is not wrapped, because the Outbox couriers, the held queue and the verify step are woven through it and a wrapper would touch every one. They show no choice.
  - Antigravity: `acp` (Antigravity CLI, `agy acp`, the default), `app` (the Antigravity app, a draft you send there) and `remote` (Antigravity Remote Control, in the browser, you press Send there).
  - `acp` does everything. `app` and `remote` reply and start, but not from the phone and not stop.
- The choice is the setting `antigravityChannel` (`acp`, `app`, `remote`; default `acp`; an unknown value reads as the default). It is an `ask` key: an agent that wants it changed asks the user in the Inbox, because it decides whether Notchling runs `agy` and sends text for you. Settings has an Antigravity section with one row, "Talk to Antigravity through". A tool with one channel gets no row.
- **Routing.**
  - `acp` behaves exactly as before. Nothing in the `agy acp` code (the runner, the Outbox courier, `agy.send`, `agy.open`, permissions, Stop) was removed; the only addition on that path is the check that the chosen channel is `acp`.
  - With `app` or `remote` chosen, the Mac's Chat hands the text over itself (it owns the window and the clipboard) through an `Opener` (open a URL, copy text, check the app) and does not send anything to the daemon. The composer keeps the text, says so under the box ("Opened in Antigravity as a draft. Press Send there."), and its Send button and placeholder say it opens in Antigravity.
  - `app`: the draft link, the text percent-encoded, no `project` (no project id is on disk for a session). `remote`: the text is copied (the page cannot take it) and the conversation link opens for the active Antigravity account, from Notchling's own account book, so a switch changes the link; with no email the account chooser is left out. A draft row opens the device page. With no `installation_uuid` the error says to turn on Remote Control in Antigravity Settings > Application.
  - The daemon refuses every other way in while a hand-off channel is chosen: `session.reply`, `session.send` and `agy.send` (a reply, or a new session by folder) answer "Antigravity is set to open in another app. Reply from Chat on the Mac, then press Send there."; a message already queued when the choice changed fails with the same words instead of starting `agy`. From the phone the answer is the channel's own refusal, sent as the existing `view_only` error, which the phone already shows. Nothing is added to `agentconsole.v1`.
- **Remote Control opens in Chat's right-hand panel.** The setting `antigravityRemoteIn` (`panel`, the default, or `browser`; a display preference, not an `ask` key) decides where the `remote` link opens. In the panel it is a new page kind, `PanelPage::Antigravity(url)` (shown as `Opens::Antigravity`), in the same place Artifact pages open and with the same size, hover and layout rules; Browser sends it to the default browser with `cx.open_url`. The page has its own persistent web data store (`ANTIGRAVITY_STORE`, separate from the Artifact store), so a Google sign-in lasts. Navigation is limited to `https` hosts of `google.com`, `gstatic.com` and `googleusercontent.com` (accounts.google.com, antigravity.google.com and the sign-in helpers); any other link, and any new-window request, opens in the default browser, and the page never leaves for another site. The panel shows one line, "Use password or phone sign-in; passkeys don't work here.", the panel's existing "Open in Browser" button is the fallback, and WebRTC is left to the web view's own defaults. The panel path is only a choice of opener: the link is built the same way, and `Opener::panel` is the one added method. Nothing here was run against Google's live pages.
- Words are in `.ftl` files; the refusals the daemon sends are plain English like its other errors.

## What stays

- `agy acp` is the default and is untouched, by the maintainer's choice.
- Reading and watching Antigravity sessions from their logs, `notchling delegate agy`, the Antigravity account and limit code (including the limit auto-switch, which also counts as automating account rotation) and Codex and Claude Code delivery are unchanged. Reviews still go through `notchling delegate agy`.
- `agy.open`, the permission card, the model pickers and Stop still use `agy acp` while the session is open in Chat, whatever the channel, because Chat reads the live conversation that way.

## A new channel or provider

A channel is a struct implementing `Channel` in `contract/channels.rs` and an entry in that provider's list; the first entry is the default. A tool with more than one channel names its setting in `channel::setting_key` and lists the ids in the `KeySpec`'s `allowed` (a test checks they match). The Chat side adds the words and the hand-over in `tools/agy_hand_off.rs`; a channel that opens a link is only a link builder and an `Opener` call. A channel that sends for itself (`Mode::Sends`) must be routed in the daemon where the tool's reply path is; the check `OutboxOps` and `AgyOps::allow` make is the model.

Other channels this shape fits: a Remote Control link that carries the text once Google gives one, an editor-extension link, a tool's own deep link. A channel that opens a page inside Notchling calls `Opener::panel`, adds a `PanelPage` kind with its own data store and allowlist, and says in its `Mode` that the user sends there.

## Consequences

- The default changes nothing for anyone who does not open Settings.
- With `app` or `remote` chosen the phone cannot reply to Antigravity sessions, and nothing in Notchling sends to Antigravity.
- The link formats are Google's and can change; a tool that opens nothing shows no error (the app only checks that it is installed, and Notchling cannot check Remote Control).
- Very long messages make very long links; a limit was not measured.

## Status

Proposed. The maintainer settles it by merging the change that brings it, and assigns its number then.

## Date

2026-11-18
