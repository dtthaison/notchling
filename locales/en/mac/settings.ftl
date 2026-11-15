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

# Button in the Client section that asks for the newest version again.
settings-client-check-again = Check Again

# Button in the Client section that updates the tool on this Mac.
settings-client-update = Update

# Button in the Client section that backs out of going back to an older version.
settings-client-cancel = Cancel

# Note beside the Update button when the Install switch is off.
settings-client-install-first = Turn on Install in Settings first

# Line in the Client section before the tool has ever been checked.
settings-client-not-checked = Not checked yet

# Line in the Client section while a check runs.
settings-client-checking = Checking…

# Line in the Client section when the check could not decide; it will run again.
settings-client-cant-tell = Couldn't tell yet. Trying again in a couple of minutes.

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

# What the hook before every tool call does. A cluster is a group of Macs and servers.
settings-hook-fence = Keeps a cluster session from running tool calls here once it moved to another machine.

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

# Result under an Activity row after Check Against the Audit Log: the entry is the one that was sealed.
settings-activity-check-matches = Matches the sealed entry

# Result under an Activity row after Check Against the Audit Log: the entry was changed.
settings-activity-check-differs = Doesn't match the sealed entry

# Result under an Activity row after Check Against the Audit Log: the entry was never sealed.
settings-activity-check-unsealed = That entry isn't sealed

# Result under an Activity row when the daemon sent nothing for the entry.
settings-activity-check-no-entry = The daemon didn't send that entry

# Result under an Activity row when the row left the list while it was checked.
settings-activity-check-gone = That row is no longer listed

# Button under an Activity row while the check runs.
settings-activity-check-running = Checking…

# Button under an Activity row that compares the entry with the audit log.
settings-activity-check-button = Check Against the Audit Log

# Note under an Activity row the app recorded while the daemon was down.
settings-activity-spooled = Recorded by the app while the daemon was down

# Detail line under an Activity row. $event is the name of the event.
settings-activity-detail-event = Event: { $event }

# Detail line under an Activity row. $outcome is how the event ended.
settings-activity-detail-outcome = Outcome: { $outcome }

## Skills & Models page

# Tab of the Settings sheet: how skills and model choices work, which skills are installed and which model each job uses.
settings-page-skills = Skills & Models

# Name of a tool on the Skills & Models page.
settings-tool-codex = { -brand-codex }

# Name of a tool on the Skills & Models page.
settings-tool-antigravity = { -brand-antigravity }

# Section title on the Skills & Models page: how skills and model knowledge are kept up to date.
settings-how-title = How It Works

# First line under How It Works: skills follow the app's version.
settings-how-skills = Skills are text that ships with { -brand-notchling }, so they update when the app does.

# Second line under How It Works: the commands a skill calls do not change between versions.
settings-how-commands = The commands a skill runs, such as notchling delegate, stay the same from one version to the next.

# Third line under How It Works: model choices come from the providers' lists.
settings-how-models = Which model to use for a job comes from the built-in list and each provider's own list of models, so a new model needs no app update.

# Fourth line under How It Works: the user's file beats the built-in list.
settings-how-override = Your override file wins over the built-in list.

# Section title on the Skills & Models page: the skills Notchling writes for each tool.
settings-skills-title = Skills

# Shown above the skills while Notchling asks the daemon.
settings-skills-checking = Checking the skills

# Shown when the daemon is not attached, so the skills cannot be read.
settings-skills-detached = The daemon isn't attached, so the skills can't be checked.

# State tag of a skill: written, and the same as this version's text.
settings-skill-installed = Installed

# State tag of a skill: written by an older version, so its text differs.
settings-skill-out-of-date = Out of date

# State tag of a skill: not written yet.
settings-skill-not-installed = Not installed

# State tag of a skill: the tool it is for is not on this Mac.
settings-skill-tool-missing = Tool not installed

# State tag of a skill: a folder of that name is there and Notchling did not write it, so it is left alone.
settings-skill-left-alone = Left alone

# Button under the skills: writes the missing and out-of-date skills.
settings-skills-reinstall = Reinstall Skills

# Button text while the skills are being written.
settings-skills-reinstalling = Reinstalling…

# Line under the skills after a reinstall.
settings-skills-reinstalled = Skills are up to date.

# Line under the skills when the daemon's install switch is off.
settings-skills-switch-off = Skills can't be written yet: the install switch is off in the daemon.

# Note under the skills.
settings-skills-footer = Skills go in each tool's own skills folder. A tool that isn't installed is skipped, and a folder Notchling didn't write is never changed.

# Section title on the Skills & Models page: the model each job uses.
settings-models-title = Models by Job

# Shown above the jobs while Notchling asks the daemon.
settings-models-checking = Checking the model lists

# Shown when the daemon is not attached, so the model choices cannot be read.
settings-models-detached = The daemon isn't attached, so the model choices can't be read.

# Name of a job that a model is chosen for: review after each task.
settings-role-review = Review

# Name of a job that a model is chosen for: the review of a whole branch.
settings-role-review-final = Final Review

# Name of a job that a model is chosen for: the second review of code that touches secrets.
settings-role-review-secrets = Secrets Review

# Name of a job that a model is chosen for: the check of a spec against what was built.
settings-role-spec-check = Spec Check

# Line under a job that no model is marked for.
settings-model-no-pick = No model is marked for this job.

# Line for a model choice. $tool is the tool, $effort the effort level to run it at.
settings-model-effort = { $tool } · effort { $effort }

# Where a model choice came from: the list that ships with the app.
settings-model-source-built-in = Built-in

# Where a model choice came from: the user's own override file.
settings-model-source-override = Your override

# Where a model choice came from: the provider's own list replaced the marked model.
settings-model-source-live = Live list

# Line for a model choice when nothing in the marked model's family is on the provider's list.
settings-model-none = No model available

# Line under a model choice when the provider's list has not been read yet.
settings-model-unchecked = Not checked against the provider's list yet.

# Note under the jobs.
settings-models-footer = The same choice notchling delegate-models --for makes for an agent.

# Section title on the Skills & Models page: the user's own model list.
settings-override-title = Override

# Line in the Override section when there is no file.
settings-override-missing = No override file yet. Open Override File makes an empty one.

# Line in the Override section when the file is in use.
settings-override-found = Your override file is in use.

# Line in the Override section when the file can't be read as an override.
settings-override-unreadable = The file isn't a JSON object, so it is ignored.

# Button in the Override section: opens the file in the user's editor, making it first if it is missing.
settings-override-open = Open Override File

# Button in the Override section: reads the model choices again.
settings-override-reload = Reload

# Error when the empty override file could not be made. $why says what went wrong.
settings-override-failed = Couldn't make the override file: { $why }

# Note under the Override section.
settings-override-footer = An entry here replaces the built-in one for the same model. Model choices change on Reload; prices, windows and names apply when the daemon starts.

# Shown on the Skills & Models page when the daemon's answer isn't in a shape this version of the app reads.
settings-reply-unreadable = The daemon's answer isn't one this app reads.

# Line under Reinstall Skills when one skill could not be written. $skill is the skill's folder name, $why what went wrong.
settings-skill-error = { $skill }: { $why }

## Cluster section

# Section title on the General page, shown only on a Mac that is in a cluster.
settings-section-cluster = Cluster

# Row in the Cluster section: how long the primary Mac can be unreachable before a Standby takes over its sessions.
settings-cluster-failover = Automatic Failover

# Choice of the Automatic Failover row: no machine takes over by itself.
settings-cluster-failover-off = Off

# Choice of the Automatic Failover row: a Standby takes over after the primary has been unreachable for 30 minutes.
settings-cluster-failover-v30m = After 30 Minutes

# Choice of the Automatic Failover row: a Standby takes over after the primary has been unreachable for 1 hour.
settings-cluster-failover-v1h = After 1 Hour

# Line under the Automatic Failover row while it is on. $standby is the name of the machine that takes over, $primary the name of the primary Mac.
settings-cluster-failover-line = { $standby } takes over when { $primary } can't be reached for this long. { $primary } pauses those sessions 2 minutes before.

# Line under the Automatic Failover row while it is off.
settings-cluster-failover-off-line = Off: no machine takes over by itself.

# Line under the Automatic Failover row while it is on and no machine in the cluster is able to take over.
settings-cluster-failover-no-standby = No Standby can take over yet. Turn on Can Be Master for another machine in the cluster.

# Line under the Automatic Failover row while it is on and the machine that takes over hasn't yet confirmed it is ready.
settings-cluster-failover-not-ready = Automatic failover: not ready
