---
version: alpha
name: Notchling Chat on the phone
description: The Notchling Chat kit (docs/chat-kit.md, ChatKit.swift) drawn for a phone, so the Mac and the phone read as one product.
colors:
  neutral: "#1B1917"
  neutral-light: "#FAF8F5"
  sidebar: "#151311"
  sidebar-light: "#F4F0EA"
  card: "#242120"
  card-light: "#FFFFFF"
  raised: "#2F2B28"
  raised-light: "#ECE6DD"
  hairline: "#FFFFFF14"
  hairline-light: "#3B2A1A1F"
  on-surface: "#F3EEE8"
  on-surface-light: "#1F1B17"
  muted: "#A69D93"
  muted-light: "#6C6359"
  faint: "#736B62"
  faint-light: "#9B9187"
  primary: "#F7873B"
  on-primary: "#0F0E0D"
  on-primary-light: "#1F1B17"
  accent-text: "#FFB47E"
  accent-text-light: "#B4540E"
  working: "#6EB0FF"
  working-light: "#2C78D4"
  done: "#7DCB2C"
  done-light: "#3F7D0B"
  shell: "#FF7AB8"
  shell-light: "#C2307A"
  on-shell: "#0F0E0D"
  on-shell-light: "#FFFFFF"
  danger: "#FF8A7A"
  danger-light: "#C2362B"
typography:
  title:
    fontFamily: system
    fontSize: 19px
    fontWeight: 600
    lineHeight: 1.3
  heading:
    fontFamily: system
    fontSize: 15px
    fontWeight: 600
    lineHeight: 1.4
  body:
    fontFamily: system
    fontSize: 15px
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: system
    fontSize: 13px
    fontWeight: 500
    lineHeight: 1.35
    fontFeature: "tnum"
  meta:
    fontFamily: system
    fontSize: 12.5px
    fontWeight: 400
    lineHeight: 1.4
  section:
    fontFamily: system
    fontSize: 12px
    fontWeight: 600
    lineHeight: 1.3
    letterSpacing: 0.06em
  mono:
    fontFamily: Geist Mono
    fontSize: 13px
    fontWeight: 400
    lineHeight: 1.5
rounded: { sm: 10px, md: 16px, bubble: 20px, bubble-tail: 6px, composer: 22px, sheet: 28px, full: 999px }
spacing: { xs: 4px, sm: 8px, md: 12px, lg: 16px, xl: 24px }
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.on-primary}"
    typography: "{typography.label}"
    rounded: "{rounded.md}"
    height: 48px
    padding: 16px
  button-secondary:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.on-surface}"
    typography: "{typography.label}"
    rounded: "{rounded.md}"
    height: 48px
    padding: 16px
  button-primary-light:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.on-primary-light}"
  button-secondary-light:
    backgroundColor: "{colors.raised-light}"
    textColor: "{colors.on-surface-light}"
  card:
    backgroundColor: "{colors.card}"
    textColor: "{colors.on-surface}"
    typography: "{typography.body}"
    rounded: "{rounded.card}"
    padding: 16px
  card-light:
    backgroundColor: "{colors.card-light}"
    textColor: "{colors.on-surface-light}"
  window:
    backgroundColor: "{colors.neutral}"
    textColor: "{colors.on-surface}"
  window-light:
    backgroundColor: "{colors.neutral-light}"
    textColor: "{colors.on-surface-light}"
  bar:
    backgroundColor: "{colors.sidebar}"
    textColor: "{colors.muted}"
  bar-light:
    backgroundColor: "{colors.sidebar-light}"
    textColor: "{colors.muted-light}"
  composer:
    backgroundColor: "{colors.card}"
    textColor: "{colors.on-surface}"
    typography: "{typography.body}"
    rounded: "{rounded.composer}"
    padding: 4px
  composer-light:
    backgroundColor: "{colors.card-light}"
    textColor: "{colors.on-surface-light}"
  composer-shell:
    backgroundColor: "{colors.shell}"
    textColor: "{colors.on-shell}"
  composer-shell-light:
    backgroundColor: "{colors.shell-light}"
    textColor: "{colors.on-shell-light}"
  chip:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.muted}"
    typography: "{typography.label}"
    rounded: "{rounded.md}"
    height: 40px
  chip-meta:
    backgroundColor: "{colors.hairline}"
    textColor: "{colors.faint}"
  status-working:
    backgroundColor: "{colors.working}"
    textColor: "{colors.done}"
  status-working-light:
    backgroundColor: "{colors.working-light}"
    textColor: "{colors.done-light}"
  accent-word:
    backgroundColor: "{colors.neutral}"
    textColor: "{colors.accent-text}"
  accent-word-light:
    backgroundColor: "{colors.neutral-light}"
    textColor: "{colors.accent-text-light}"
  error:
    backgroundColor: "{colors.neutral}"
    textColor: "{colors.danger}"
  error-light:
    backgroundColor: "{colors.neutral-light}"
    textColor: "{colors.danger-light}"
---

## Overview

Agent Console for a developer who runs Claude Code on a Mac and checks on it from the phone: what needs an answer, what is working, what finished. It is the same product as Notchling's Chat window, so the phone copies the Mac's kit instead of inventing a second look: warm near-black surfaces, one orange accent, status always an icon plus a word, the green pet in empty states. Dark first; the light theme is warm paper. Density is calm: a row says what the session is doing in one line, and nothing moves unless an agent does.

Source of truth: `docs/chat-kit.md` and `legacy/macos/Sources/Notchling/DesignKit/ChatKit.swift` (palette, meanings, radii). When the Mac kit changes, change the values here and in `lib/app/theme/tokens.dart` together. No palette row from the uix-skill data tables was used; the values are the kit's. Roles and the 4 pt grid are identical; sizes are phone sizes (body 15 where the Mac has 13). The linter was not run (it needs a network download); the references between tokens were checked by hand.

## Colors

Every role has a dark value and a `-light` value; the screen picks one by system brightness (`ThemeMode.system`).

- **neutral** (`window`): the page behind messages and lists. **sidebar**: bars (top bar, tab bar). **card**: Claude's messages, cards, the composer. **raised**: hover and pressed fills, chips, code, tool capsules. **hairline**: borders and dividers.
- **on-surface**: body text. **muted**: secondary lines. **faint**: ages and hints only; it is about 3:1 on the window, so no sentence is set in it.
- **primary** (orange): send, the primary button, focus, selection. Text on it is **on-primary**, which is dark, because white fails on orange. Orange as words uses **accent-text**.
- **working** blue and **done** lime are status. Needs Input is orange. A status is never color alone: it is a capsule with an icon and a word.
- **shell** pink marks a `!` command: the composer border and header, the run button, the `!` in a sent command. It is the color Claude Code's terminal uses for shell mode.
- **danger**: failures, as text on the window.
- Diff and syntax colors derive from these: added lines are done at 16% over the card, removed lines danger at 16%; code uses accent-text for keywords, a lime for strings, working for numbers, shell pink for type names, faint for comments.

Contrast pairs relied on: on-surface on neutral and on card, muted on neutral, on-primary on primary, accent-text on card, working and done on card (as the words of a status capsule).

## Typography

Text uses the platform font (Roboto on Android, San Francisco on iOS), as the Mac kit uses the system font. Code uses Geist Mono (SIL OFL, bundled; `assets/fonts/OFL.txt`). Vietnamese tone marks need room: body line height is 1.5, labels 1.35, headings 1.3 to 1.4, and no text has a fixed height.

- **title** 19: the session name in a header, a state's headline. **heading** 15: session names in rows, card titles. **body** 15: messages, fields. **label** 13: buttons, pills, tabs, with tabular figures. **meta** 12.5: project lines, hints, the line under a title. **section** 12 uppercase, tracked: list group titles (Needs Input, Working, Done), once per group. **mono** 13: code, commands, paths, tool input.
- Sizes follow the system font scale up to the largest setting. The chat header keeps the name to one line from 1.4x so the conversation keeps its room.

## Layout

4 pt grid: 4, 8, 12, 16, 24. Screen padding 16 (12 inside the chat list). Every control is at least 48 by 48 dp; a session row is at least 56 dp and grows with text. Rows are separated by space, not lines; the list has no card around it. The chat is one column: your messages right-aligned at 82% width, Claude's cards full width, tool groups and notices full width. From 840 dp the three panels sit side by side like the Mac. The tab bar stays; the keyboard moves only the composer (KeyboardLift) and never resizes the list per frame.

## Elevation

One light, from above. Three levels (`Surface` in `lib/app/widgets/kit.dart`, tokens in `tokens.dart`):

- **e0**: the window and anything flat (your bubbles, the shell bubble). No shadow.
- **e1**: cards, Claude's bubbles, the tool capsule. A 1 px edge that is lit at the top (white 6% on dark, 70% on light) and fades out by the middle, a slightly darker bottom edge, and a short shadow: dark `0 1 2 black 35%` plus `0 6 16 black 25%`; light the same in warm brown at 10% and 8%.
- **e2**: the composer, the permission card, sheets and banners. The same edge and a stronger shadow: `0 1 3` plus `0 10 30`.

Shadows are black on dark and warm brown on light (never grey), short and dark; there is no glow, no coloured halo, no light streak. A focus ring is 2 px outside the shape (accent; pink in shell mode). Hairlines are one physical pixel (`1 / devicePixelRatio`) at white 8% on dark and warm brown 12% on light; groups are separated by space where they can be. The top bar and the bottom tab bar are frosted glass: the content scrolls under them, blurred (sigma 24) and tinted with the surface colour at about 72% (dark) and 78% (light), with one hairline on the edge facing the content.

## Shapes

One scale, smooth (superellipse) corners, never a mix inside a component family: **sm 10** small controls (buttons, fields, code boxes, the theme switch), **md 16** cards, rows, capsules, banners, **bubble 20** chat bubbles with the corner nearest the sender at **6** (yours bottom-right, Claude's top-left), **composer 22**, **sheet 28** on the top corners of sheets and dialogs. Chips, pills, badges, avatars and the send button are fully round.

## Components

- **Session row**: avatar 40 with a status badge in the corner (initials on a gradient picked from the name, so a session keeps its color), name, project and age, then the status capsule and, under it, the live activity line ("Editing PaymentService.swift", or what a Needs Input session waits for). A running session with subagents has a disclosure arrow that opens the subagent tree (a trunk and elbows, each child with its type, description, model and state). Swipe right takes a view-only session over, swipe left archives it with an undo; both also exist inside the session.
- **Chat header**: avatar, name, project and control, bell; below it one scrolling line with the status capsule, the Context meter (tokens over the model's window, a 3 dp bar, orange from 80%) and the account's 5h and Week limits.
- **Messages**: you in an accent-wash bubble; Claude in a card with the mark; tools as one raised capsule per run ("4 tool steps"), opened to its steps, each opened to its output or, for Edit and Write, a diff with a gutter; a `!` command in a shell-wash bubble with its output; one centered notice when the conversation is compacted; code in a raised block with a language label and a copy button.
- **Composer**: the field on top, photo, mic and send under it. A draft starting with `!` turns pink: border, header ("Shell command", the folder it runs in), a "Will run" box when a copied command was put back together, run button. A draft starting with `/` lists matching slash commands above the field.
- **Permission and question card**: card with an orange hairline, icon and word, the command in mono, Allow and Deny.
- **States**: every list and the chat have loading (skeleton in the shape of the rows), empty (the pet and one next step), error (plain sentence and Retry; what is on screen stays), populated, and a quiet "Updating" line while newer messages load.

## Motion

Motion tier: lively. Things that arrive spring into place (mass 1, stiffness 380, damping 30 for rows, bubbles and cards; softer for sheets): a message comes from its own side (yours from the right, Claude's from the left) with a 16 dp rise and a scale from 0.97; the first screenful of sessions cascades in 40 ms apart; the permission card and the slash items spring up from the composer, the items staggered. The tab bar's pill slides to the selected tab, its icon morphs from outline to filled with a small pop and its label gets heavier. The loading skeleton has a soft light sweeping across it; the group counts slide when they change; the working indicator under the newest message is three dots that hop in turn plus the activity line; a Needs Input badge bounces once and then a faint ring swells and fades; the send button turns (arrow to run in shell mode); the scroll-down button pops in with a count; the pet glances, hops and falls asleep. Everything animates transform and opacity (layout only for the short expand of a group or a sheet, never on a long list); one shared controller per effect; hidden tabs stop theirs (TickerMode). Reduce Motion drops the movement and keeps short fades. Nothing starts at scale 0.

## Settings and alerts

Appearance (System, Dark, Light) and notification sounds live in Settings (the gear in the top bar), stored on the phone; the theme change crossfades every colour. While the app is open an alert for any session except the open chat is a banner from the top (avatar, name, first line; tap opens that chat, swipe up dismisses, 4 s) with a soft sound and a light tap; in the background it is one system notification per session, grouped. Muted sessions say nothing.

## Do's and Don'ts

- Do read colors and text styles from `Kit.of(context)`; never write a hex, font name or radius in a screen.
- Do show status as an icon and a word. Don't use color alone.
- Do keep the orange for the one primary action of a screen. Don't use it for decoration.
- Do use faint only for ages and hints. Don't set a sentence in it.
- Do let labels wrap and grow; Vietnamese is longer than English. Don't fix a width on text.
- Do keep targets at 48 dp. Don't hide an action behind a swipe alone (long-press and the chat have the same actions).
- Do build surfaces with `Surface` and bars with `GlassBar`; don't draw a grey outline box or a custom shadow.
- Don't use glow, coloured halos, light streaks, gradient text or a card inside a card. The only gradients are the avatar's top light and Claude's mark; the only blur is the two glass bars.
- Don't add a second look for the phone: change the Mac kit and this file together.
