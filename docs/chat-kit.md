# Chat design kit

Built with BAMA's `design-kit` method. Code: `Sources/Notchling/DesignKit/ChatKit.swift` (tokens, type, icons, parts, gallery). Screens: `Sources/Notchling/Chat/ChatWindow.swift`.

## Feel

Warm, quiet, alive. It looks like the notch it opens from: near-black surfaces with a warm cast, one orange accent, and the green pet in the empty states. Dark first; the light version is warm paper.

## Tokens

### Palette (only `ChatKit.swift` reads it)

| Name | Hex | Name | Hex |
| --- | --- | --- | --- |
| night0 | #0F0E0D | paper0 | #FFFFFF |
| night1 | #151311 | paper1 | #F4F0EA |
| night2 | #1B1917 | paper2 | #FAF8F5 |
| night3 | #242120 | paper4 | #ECE6DD |
| night4 | #2F2B28 | paper5 | #DED5C9 |
| night5 | #3B3632 | | |
| snow | #F3EEE8 | soot | #1F1B17 |
| fog | #A69D93 | stone | #6C6359 |
| smoke | #736B62 | pebble | #9B9187 |
| orange300 | #FFB47E | orange500 | #F7873B |
| orange700 | #B4540E | lime500 | #7DCB2C |
| blue400 | #6EB0FF | blue600 | #2C78D4 |
| pink400 | #FF7AB8 | pink600 | #C2307A |

### Meaning (`KitInk`, light / dark)

| Token | Light | Dark | Used for |
| --- | --- | --- | --- |
| window | paper2 | night2 | conversation background |
| sidebar | paper1 | night1 | sidebar background |
| card | paper0 | night3 | Claude's messages, cards, composer |
| raised | paper4 | night4 | hover, chips, code |
| selection | orange500 12% | orange500 16% | selected session |
| hairline | paper5 | night5 | borders, dividers |
| text | soot | snow | body |
| textMuted | stone | fog | secondary lines |
| textFaint | pebble | smoke | hints, ages |
| accent | orange500 | orange500 | send, primary buttons, focus |
| accentText | orange700 | orange300 | orange words |
| onAccent | soot | night0 | text on accent fills (dark text: white fails on orange) |
| accentWash | orange500 14% | orange500 18% | your messages |
| needs / working / done | orange500 / blue600 / lime500 | orange500 / blue400 / lime500 | status, always with an icon and a word |
| shell | pink600 | pink400 | a `!` shell command: composer border and header, run button, the `!` in sent commands (Claude Code's terminal marks shell mode in pink) |
| shellWash | pink600 10% | pink400 14% | composer in shell mode, sent shell commands |
| onShell | paper0 | night0 | the icon on the run button |

### Metric

| Token | Value |
| --- | --- |
| sidebarWidth | 272 |
| titleBar | 38 (traffic lights sit in the sidebar's top) |
| rowHeight | 52 (session row), control 28 |
| radius | sm 6, md 10, card 14, composer 18 |
| conversation | full window width; 16 pt side padding, the same as the sidebar header and the inspector; replies have no avatar |
| space | 4 pt grid: 4, 8, 12, 16, 20, 24 |

### Motion

| Token | Curve | Used for |
| --- | --- | --- |
| hover | ease out 0.12 s | hover fills |
| press | spring 0.25 / 0.7 | button press scale |
| appear | ease out 0.2 s | new message, pending reply |

All of them become `nil` (no animation) under Reduce Motion.

## Type (`KitType`)

| Role | Size | Weight | Used for |
| --- | --- | --- | --- |
| title | 17 | semibold | sidebar and header titles |
| heading | 13 | semibold | session names, card titles |
| body | 13 | regular | messages |
| label | 12 | medium | buttons, pills |
| meta | 11 | regular | status lines, hints |
| section | 10.5 | semibold, tracked, uppercase | sidebar groups |
| mono | 12 | regular, monospaced | code, tables, tool input |

## Icons (`KitIcon`)

SF Symbols at one weight (medium), one size per place: 12 in rows and pills, 13 in buttons. Filled forms only for status badges. Claude's mark is `sparkle` on an orange disc.

## Window

Title bar hidden and transparent, full-size content. Traffic lights stay where AppKit puts them, over the sidebar's top 38 pt, which holds nothing else. Default size 1040 × 720, min 720 × 460. Sidebar is kit rows, not `List`: arrow keys move the selection, VoiceOver gets `isSelected`.

## Parts

### Session row (`KitSessionRowStyle`)
Avatar 30 with a status badge, name (heading), status line (meta, muted), age (meta, faint). Height 52, inset 8, radius md.

| State | Fill | Border |
| --- | --- | --- |
| rest | none | none |
| hover | raised 60% | none |
| pressed | raised | none |
| selected | selection | accent 35%, 1 pt |
| focused | as selected or rest | accent, 2 pt |

### Button (`KitButtonStyle`: primary, secondary, ghost)
Height 28, padding 12, radius sm, label type. Primary: accent fill, onAccent text. Secondary: raised fill, hairline border. Ghost: none until hover. Pressed scales to 0.97, disabled 40%.

### Option (`KitOptionStyle`)
A question's answer: number badge (1, 2, 3, matching the terminal's digits), title (heading), detail (meta). Card fill, hairline; hover raised; pressed accent border.

### Composer
Card fill, radius composer, hairline border (accent 60% while focused), soft shadow. Send is a 30 pt accent disc with an `arrow.up`, faint when empty.

**Shell mode**, when the draft starts with `!`: a 1.5 pt shell border over a shellWash fill, a header row (`terminal` icon, "Shell command" in label type, then "runs in <folder> · Claude sees the output" in meta), the draft in mono, a shell-colored run disc, and the hint "Return runs it like ! in the terminal · delete the ! to send a message instead". A sent `! command` shows in a shellWash bubble, mono, with the `!` in shell.

### Messages
Claude: card with the mark, radius card. You: accentWash bubble, right-aligned, max 75% of the column. Tools: raised capsule chip with a wrench. Code: raised box, mono.

### Status pill (`KitStatusPill`)
Capsule, status color 16% fill, status-colored icon and word.

### Avatar (`KitAvatar`)
Initials on a two-stop gradient picked from the session name, so a session keeps its color.
