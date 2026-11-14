### The Settings sheet: its General, Agents and Activity pages.
### A setting's title is settings-<setting key>, a choice is settings-<setting key>-<value>; a row with no setting key is named by what it says.

## The sheet

# Title at the top of the Settings sheet.
settings-title = Settings

# The button that closes the Settings sheet.
settings-done = Done

## Pages

# Tab of the Settings sheet: the app-wide settings.
settings-page-general = General

# Tab of the Settings sheet: Claude Code's hooks and what Notchling can do for each tool.
settings-page-agents = Agents

# Tab of the Settings sheet: the list of what Notchling did and refused.
settings-page-activity = Activity

## General page: section titles

# Section title on the General page: the appearance of the app.
settings-section-general = General

# Section title on the General page: the shortcut that opens Search.
settings-section-search = Search

# Section title on the General page: the daemon's switches.
settings-section-daemon = Daemon

# Section title on the General page: keeping the Mac awake.
settings-section-keep-mac-awake = Keep Mac Awake

# Section title on the General page: the notch.
settings-section-notch = Notch

# Section title on the General page: settings for Claude Code.
settings-section-claude-code = { -brand-claude-code }

# Section title on the General page: what happens when the account in use nears its usage limit.
settings-section-account-limit = When the Account in Use Nears Its Limit

# Section title on the General page: settings for Codex.
settings-section-codex = { -brand-codex }

# Section title on the General page: the Chat window.
settings-section-chat = Chat

## General page: notes under a section

# Note under the Search section. The shortcut is a keyboard shortcut; keep ⌘⇧P as it is.
settings-footer-search = Opens Search from any app, so no other app can use this key. Inside { -brand-notchling }, ⌘⇧P always opens Search.

# Note under the Daemon section.
settings-footer-daemon = Each is off whenever the daemon starts. After a restart a notice offers to turn back on the ones you had on.

# Note under the Keep Mac Awake section.
settings-footer-keep-mac-awake = Closing the lid still puts the Mac to sleep. When it wakes, background agents whose reply was cut off by sleep are told to continue.

# Note under the section about the account in use nearing its limit. Notify and Notify and Switch are the names of two choices above it.
settings-footer-account-limit = Notify tells you at 90% and 100% of a limit. Notify and Switch also moves to the saved account with the most room.

# Note under the Chat section.
settings-footer-chat = Chat shows the newest part of a long session. More shows earlier turns and uses more memory. It applies the next time a session loads.

## General page: rows with no setting key

# Row in the Daemon section: lets Notchling answer and control sessions.
settings-act-on-sessions = Act on Sessions

# Row in the Daemon section: the connection to the phone app.
settings-phone-bridge = Phone Bridge

# Row in the Daemon section: signing in to accounts and switching lanes.
settings-logins-and-lanes = Logins and Lanes

# Row in the Daemon section: the store for passwords and tokens.
settings-secret-vault = Secret Vault

# Row in the Daemon section: installing the hooks and skills into the agents' folders.
settings-install-hooks-and-skills = Install Hooks and Skills

## General page: rows

# Row in the General section.
settings-appearance = Appearance

# Choice of the Appearance row: follow the Mac's light or dark setting.
settings-appearance-system = System

# Choice of the Appearance row.
settings-appearance-light = Light

# Choice of the Appearance row.
settings-appearance-dark = Dark

# Row in the Search section: the keyboard shortcut that opens Search from any app.
settings-search-shortcut = Search Shortcut

# Button in the Search Shortcut row while it waits for the keys.
settings-shortcut-recording = Press a shortcut…

# Button in the Search Shortcut row that starts recording a new shortcut.
settings-shortcut-record = Record

# Button in the Search Shortcut row that turns the shortcut off.
settings-shortcut-off = Off

# Note under the Search Shortcut row when the keys pressed can't be a shortcut.
settings-shortcut-needs-key = Use a letter, a number or Space with Control, Option or Command.

# Row in the Keep Mac Awake section.
settings-mode = Keep awake

# Choice of the Keep awake row.
settings-mode-off = Off

# Choice of the Keep awake row.
settings-mode-on = On

# Choice of the Keep awake row: only while an agent is working.
settings-mode-while-working = While Agents Work

# Row in the Keep Mac Awake section.
settings-keep-display-on = Keep display on

# Row in the Keep Mac Awake section.
settings-continue-after-sleep = Continue agents after sleep

# Row in the Notch section. It is on when the notch is shown.
settings-hide-notch = Show agents in the notch

# Row in the Notch section: how wide the notch opens.
settings-notch-width = Width

# Choice of the notch Width row.
settings-notch-width-small = Small

# Choice of the notch Width row.
settings-notch-width-medium = Medium

# Choice of the notch Width row.
settings-notch-width-large = Large

# Row in the Notch section.
settings-show-pet = Animate pet

# Row in the Notch section.
settings-show-last-reply = Show the last reply when peeking

# Row in the Notch section.
settings-reply-from-notch = Reply to background agents from the notch

# Row in the Claude Code section.
settings-answer-from-notch = Answer prompts from the notch

# Row in the Claude Code section.
settings-keep-pages = Keep local copies of artifacts

# Row in the account limit section: what to do for Claude Code. The title is the tool's name.
settings-limit-mode = { -brand-claude-code }

# Choice of the Claude Code limit row.
settings-limit-mode-off = Off

# Choice of the Claude Code limit row: tell the user at the limit.
settings-limit-mode-notify = Notify

# Choice of the Claude Code limit row: tell the user and move to another saved account.
settings-limit-mode-notify-and-switch = Notify and Switch

# Row in the account limit section: what to do for Codex. The title is the tool's name.
settings-codex-limit-mode = { -brand-codex }

# Choice of the Codex limit row.
settings-codex-limit-mode-off = Off

# Choice of the Codex limit row: tell the user at the limit.
settings-codex-limit-mode-notify = Notify

# Choice of the Codex limit row: tell the user and move to another saved account.
settings-codex-limit-mode-notify-and-switch = Notify and Switch

# Row in the account limit section: what to do for Antigravity. The title is the tool's name.
settings-antigravity-limit-mode = { -brand-antigravity }

# Choice of the Antigravity limit row.
settings-antigravity-limit-mode-off = Off

# Choice of the Antigravity limit row: tell the user at the limit.
settings-antigravity-limit-mode-notify = Notify

# Choice of the Antigravity limit row: tell the user and move to another saved account.
settings-antigravity-limit-mode-notify-and-switch = Notify and Switch

# Row in the account limit section: the percentage of a limit at which to switch accounts. A stepper shows the number.
settings-limit-switch-at = Switch at

# Row in the Codex section.
settings-codex-network = Allow network in Codex turns

# Row in the Chat section: how much of a long conversation Chat loads.
settings-chat-history = Conversation history to load

# Choice of the Conversation history row: two megabytes.
settings-chat-history-v2mb = 2 MB

# Choice of the Conversation history row: four megabytes.
settings-chat-history-v4mb = 4 MB

# Choice of the Conversation history row: eight megabytes.
settings-chat-history-v8mb = 8 MB

# Choice of the Conversation history row: sixteen megabytes.
settings-chat-history-v16mb = 16 MB

## Agents page

# Name of the tool whose hooks the Agents page shows.
settings-tool-claude-code = { -brand-claude-code }

# Section title on the Agents page: the tool's program on this Mac and on other machines.
settings-client-title = Client

# Shown in the Client section while the daemon is asked.
settings-client-looking = Looking…

# Shown in the Client section when the daemon can't be reached.
settings-client-not-running = { -brand-notchling } isn't running

# Shown in the Client section when the tool's program was not found.
settings-client-not-found = Not found

# Line in the Client section. $path is where the program is and $version its version.
settings-client-found = Found at { $path } · { $version }

# Line in the Client section when the install method is known. $method says how it was installed.
settings-client-found-by = Found at { $path } · { $version } · { $method }

# Part of the Client section's line when the program's version could not be read.
settings-client-version-unknown = version unknown

# Part of a machine's line in the Client section: the tool is not installed there.
settings-client-not-installed = Not installed

# Section title on the Agents page: the hooks Notchling puts into the tool's settings.
settings-hooks = Hooks

# Shown above the hooks while Notchling reads the tool's settings file.
settings-hooks-checking = Checking { -brand-claude-code }'s settings

# Under a hook with no description of its own.
settings-hooks-other = Runs { -brand-notchling }'s hook

# Note under the hooks.
settings-hooks-footer = Read from { -brand-claude-code }'s settings.json when Settings opened. Install Hook in Welcome adds or changes them.

# Note under the hooks. claude -p is a command; keep it as it is.
settings-hooks-pass-note = The permission and question hooks pass straight through, and { -brand-claude } asks in its terminal, when the daemon, the app or Act on Sessions is off, and in a claude -p run. The Artifact hooks work without them.

# State tag of a hook: it is in the settings file.
settings-hook-installed = Installed

# State tag of a hook: it is not in the settings file.
settings-hook-missing = Not installed

# State tag of a hook: it was written by the old Swift app.
settings-hook-swift = Swift app

# State tag of a hook: it is in the file but Notchling does not manage it.
settings-hook-not-managed = Not managed

# What the PermissionRequest hook does.
settings-hook-permission-request = { -brand-claude }'s permission asks show in the notch, in Chat and on the phone, not only in the terminal.

# What the AskUserQuestion hook does.
settings-hook-ask-user-question = { -brand-claude }'s multiple-choice questions can be answered from the notch, Chat or the phone.

# What the hook before a read of an Artifact page does.
settings-hook-artifact-read = When { -brand-claude } reads a page saved on this Mac, it is given the local copy's path instead of the live page.

# What the hook after an Artifact publish does. notchling pages is a command; keep it as it is.
settings-hook-artifact-saved = After a publish, a local copy is saved for the Pages window and notchling pages.

# What the hook after a failed Artifact read does.
settings-hook-artifact-read-failed = When a page read fails (a 404 or another account's page), { -brand-claude } gets the local copy's path.

# Shown above the hooks when the settings file was left alone because it is not valid JSON.
settings-hooks-refused-invalid-json = { -brand-claude-code }'s settings.json was left alone: it isn't valid JSON.

# Shown above the hooks when the settings file was left alone because it is not a JSON object.
settings-hooks-refused-not-object = { -brand-claude-code }'s settings.json was left alone: it isn't a JSON object.

# Shown above the hooks when the settings file was left alone because a key is written twice.
settings-hooks-refused-twice = { -brand-claude-code }'s settings.json was left alone: it has a key written twice.

# Shown above the hooks when the settings file was left alone because hooks is not an object.
settings-hooks-refused-hooks-not-object = { -brand-claude-code }'s settings.json was left alone: it has hooks that aren't an object.

# Shown above the hooks when the settings file was left alone because a hook event is not a list.
settings-hooks-refused-event-not-list = { -brand-claude-code }'s settings.json was left alone: it has a hook event that isn't a list.

# Shown above the hooks when the settings file was left alone because it is a link to nothing.
settings-hooks-refused-broken-link = { -brand-claude-code }'s settings.json was left alone: it is a link to a missing file.

# Shown above the hooks when the settings file was left alone because it is not a regular file.
settings-hooks-refused-not-regular = { -brand-claude-code }'s settings.json was left alone: it isn't a regular file.

# Shown above the hooks when the settings file was left alone because it is too big.
settings-hooks-refused-too-big = { -brand-claude-code }'s settings.json was left alone: it is too big.

# Shown above the hooks when the settings file was left alone because it can't be written.
settings-hooks-refused-read-only = { -brand-claude-code }'s settings.json was left alone: it is read-only.

# Shown above the hooks when the settings file was left alone because it is not UTF-8 text.
settings-hooks-refused-not-utf8 = { -brand-claude-code }'s settings.json was left alone: it isn't UTF-8 text.

# Shown above the hooks when the settings file was left alone and the reason is not known.
settings-hooks-refused-unreadable = { -brand-claude-code }'s settings.json was left alone: it can't be read.

# Section title on the Agents page: what Notchling does for a tool that the tool does not do itself.
settings-capabilities-title = What { -brand-notchling } Can Do

# Note under What Notchling Can Do. notchling providers is a command; keep it as it is.
settings-capabilities-footer = Anything not listed works natively for that tool. notchling providers prints every row.

# Tag on a capability: the tool lacks it and Notchling does it itself.
settings-capability-fallback = { -brand-notchling }'s own

# Tag on a capability: it does not apply to this tool.
settings-capability-not-applicable = Not applicable

# Tag on a capability: Notchling has not built it for this tool yet.
settings-capability-gap = Not built yet

## Activity page

# Filter chip and tag on the Activity page: an operation that was refused.
settings-activity-kind-refused = Refused

# Filter chip and tag on the Activity page: an operation that failed.
settings-activity-kind-failed = Failed

# Filter chip and tag on the Activity page: an operation that was done.
settings-activity-kind-acted = Acted

# Filter chip and tag on the Activity page: a setting that changed.
settings-activity-kind-setting = Setting

# Filter chip and tag on the Activity page: an inbox item that was answered.
settings-activity-kind-inbox = Inbox

# Filter chip and tag on the Activity page: the account in use was switched.
settings-activity-kind-account-switch = Account Switch

# Filter chip with a count. $label is the kind, for example Refused; $count is how many rows have it.
settings-activity-chip = { $label } { NUMBER($count) }

# Placeholder of the search box on the Activity page.
settings-activity-search = Search Operation or Result

# Button on the Activity page.
settings-activity-refresh = Refresh

# The Refresh button while it reloads.
settings-activity-refreshing = Refreshing…

# Shown on the Activity page while the first load runs.
settings-activity-loading = Loading…

# Shown on the Activity page when nothing was recorded.
settings-activity-empty = No activity yet

# Shown on the Activity page when the filter or the search matches nothing.
settings-activity-no-match = Nothing matches

# Who did it, for a row that came from the phone. $who is the phone's name.
settings-activity-from-phone = { $who } (phone)

# Second line of a row. $who is who asked and $result what came of it.
settings-activity-line = { $who } · { $result }

# Second line of a row that has a detail. $who is who asked, $result what came of it and $detail why.
settings-activity-line-detail = { $who } · { $result } · { $detail }

# Note under the list when there are more rows than it shows. $shown is how many show and $total how many there are.
settings-activity-showing = Showing the newest { NUMBER($shown) } of { NUMBER($total) } rows

# Note under the list when the daemon sent only the newest rows.
settings-activity-truncated = The oldest rows are not shown

# Time of a row from earlier today, in the 12-hour clock before noon. $hour is 1 to 12 and $minute two digits.
settings-activity-time-am = { $hour }:{ $minute } AM

# Time of a row from earlier today, in the 12-hour clock from noon on.
settings-activity-time-pm = { $hour }:{ $minute } PM

# Time of a row from another day this year. $month is the short month name, $day the day and $clock the time.
settings-activity-time-date = { $month } { $day }, { $clock }

# Time of a row from another year. $year is the year.
settings-activity-time-date-year = { $month } { $day }, { $year }, { $clock }

# Short month name in a row's time.
settings-activity-month-jan = Jan

# Short month name in a row's time.
settings-activity-month-feb = Feb

# Short month name in a row's time.
settings-activity-month-mar = Mar

# Short month name in a row's time.
settings-activity-month-apr = Apr

# Short month name in a row's time.
settings-activity-month-may = May

# Short month name in a row's time.
settings-activity-month-jun = Jun

# Short month name in a row's time.
settings-activity-month-jul = Jul

# Short month name in a row's time.
settings-activity-month-aug = Aug

# Short month name in a row's time.
settings-activity-month-sep = Sep

# Short month name in a row's time.
settings-activity-month-oct = Oct

# Short month name in a row's time.
settings-activity-month-nov = Nov

# Short month name in a row's time.
settings-activity-month-dec = Dec
