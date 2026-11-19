### Settings > Models & Costs: the page where the person sees and edits the Cost Profile, the table of models and efforts per role and tool, runs Set Up, checks the files it wrote and sees how long each tool keeps its cache.
### A role is a kind of work (read, code, fix, judge, hardest, review, image). A floor is the lowest model and effort a role may use. Set Up writes rule files, agent files and settings keys into the tools' own folders, and Undo puts them back.

## Page

# Tab of the Models & Costs page: the role table and the preset.
costs-tab-roles = Roles

# Tab of the Models & Costs page: whether the files Set Up wrote still match the table.
costs-tab-check = Check

# Tab of the Models & Costs page: how long each tool keeps its prompt cache.
costs-tab-cache = Cache

# Shown on the page while the daemon has not answered.
costs-loading = Reading the Cost Profile…

# Shown on the page when the daemon's answer can't be read.
costs-unreadable = The Cost Profile can't be read.

# Shown on the page when the profile could not be read; $why is the reason in English.
costs-read-failed = The Cost Profile couldn't be read: { $why }

## Set Up entry

# Section title above the Set Up button.
costs-setup-title = Set Up

# One line under that title: what Set Up does.
costs-setup-intro = Writes this table into each tool's rules, agents and settings. You see every file first, and nothing is written until you press Apply.

# Button that opens the Set Up sheet.
costs-setup-open = Set Up…

## Preset

# Section title above the Saver, Balanced, Quality and Custom buttons.
costs-preset-title = Preset

# Name of a preset: the cheapest one.
costs-preset-saver = Saver

# Name of a preset: the default one.
costs-preset-balanced = Balanced

# Name of a preset: the one that spends more for quality.
costs-preset-quality = Quality

# Name of the choice that shows once a pick has been edited.
costs-preset-custom = Custom

# Note under the preset buttons; $preset is the name of the chosen preset.
costs-preset-note = Every role and habit below comes from { $preset }. Pick a model to make it Custom.

# Note under the preset buttons once a pick was edited; $preset is the name of the preset it started from.
costs-preset-custom-note = Custom, started from { $preset }. Choose a preset to start over.

## Role table

# Section title above the table of roles.
costs-roles-title = Roles

# Note under the table of roles.
costs-roles-footer = Choose a cell to pick its model and effort. Only models the tool lists are offered, and no pick goes below a role's floor.

# Name of a role: reading many files, searching, scanning logs.
costs-role-read = Read

# Name of a role: builders, UI tasks, tests and refactors.
costs-role-code = Code

# Name of a role: a bug fix.
costs-role-fix = Fix

# Name of a role: design, root causes and writing.
costs-role-judge = Judge

# Name of a role: the hardest work, such as a bug two fixes failed to kill.
costs-role-hardest = Hardest

# Name of a role: a second opinion on a change.
costs-role-review = Review

# Name of a role: making or editing pictures.
costs-role-image = Image

# Under a role's name; $floor is the lowest model and effort, such as "opus high".
costs-floor-line = Floor: { $floor }

# In a cell of the role table when the tool is switched off.
costs-tool-off = Off

# In a cell of the role table when the role has no pick for that tool.
costs-cell-not-set = Not set

# In a cell of the role table when the pick names no model and the tool chooses.
costs-cell-no-model = Tool default

# Tip on a cell the person has changed.
costs-cell-own = Your pick. Choose a preset to go back.

# Tip on a cell: a model was replaced; $detail is the change, such as "opus -> sonnet".
costs-note-model-missing = The tool no longer lists that model: { $detail }

# Tip on a cell: a floor raised the pick; $detail is the change.
costs-note-floor = Raised to the role's floor: { $detail }

# Tip on a cell: the model doesn't offer that effort; $detail is the change.
costs-note-effort-clamped = That effort isn't offered for this model: { $detail }

# Tip on a cell: the tool's model list has not been read yet.
costs-note-unchecked = The tool's model list isn't known yet, so this pick is unchecked.

## Picker

# Heading of the picker under the table; $role is the role and $tool the tool.
costs-picker-title = { $role } · { $tool }

# Label over the list of models in the picker.
costs-picker-model = Model

# Label over the effort buttons in the picker.
costs-picker-effort = Effort

# Button in the picker that drops the person's own pick.
costs-picker-reset = Use Preset Value

# Line at the end of the picker; $floor is the lowest model and effort, such as "opus high".
costs-picker-floor = Floor for this role: { $floor }. Lower picks are off.

# Tip on a model or effort that is off; $role is the role and $floor the lowest model and effort.
costs-below-floor = Below the floor for { $role }: { $floor } or better.

## Habits

# Section title above the habits list.
costs-habits-title = Habits

# A habit: when a builder hands back; $count is the number of turns.
costs-habit-hand-back = { $count ->
    [one] Builder hands back after { $count } turn
   *[other] Builder hands back after { $count } turns
}

# A habit: how many builders run at once; $count is how many.
costs-habit-parallel = { $count ->
    [one] { $count } builder at once
   *[other] { $count } builders at once
}

# A habit: how many lines one read takes; $count is the number of lines.
costs-habit-read = { $count ->
    [one] Reads of { $count } line
   *[other] Reads of { $count } lines
}

# A habit: the longest wait inside a subagent; $count is the number of seconds.
costs-habit-wait = Subagent waits up to { NUMBER($count) } s

# A habit: how much Bash output stays inline; $count is the number of characters.
costs-habit-output = { $count ->
    [one] Bash output inline up to { NUMBER($count) } character
   *[other] Bash output inline up to { NUMBER($count) } characters
}

## Imported rules

# Section title above the note about rules taken from the person's own files.
costs-imported-title = Imported Rules

# Line under a rule file's name; $moved is how many lines went into the table and $kept how many stayed in the file.
costs-imported-summary = { NUMBER($moved) } moved into the table, { NUMBER($kept) } kept in your file.

## Check

# Section title above the list of files Set Up wrote.
costs-check-title = Files Set Up Wrote

# While the daemon has not answered.
costs-check-loading = Checking…

# When Set Up has not written anything yet.
costs-check-empty = Nothing has been written yet. Run Set Up first.

# When every file matches.
costs-check-clean = Every file matches the table.

# Summary over the list; $drifted is how many files differ and $total how many there are.
costs-check-drift = { NUMBER($drifted) } of { NUMBER($total) } differ from the table.

# Button that reads the files again.
costs-check-again = Check Again

# Button on a row: leave the file as the person made it and stop tracking the difference.
costs-check-keep = Keep Mine

# Button on a row: write the file again from the table.
costs-check-restore = Restore

# On a row while its change is being made.
costs-check-working = Working…

# Under the list when the install switch is off.
costs-check-install-off = The install switch is off, so nothing can be kept or restored. Turn it on in Settings > General.

# Note under the list.
costs-check-footer = Keep Mine accepts your edit. Restore writes the file again from the table, after a backup.

# State of a file: it matches.
costs-drift-in-step = In step

# State of a file: the table has changed since it was written.
costs-drift-outdated = Outdated

# State of a file: the person edited it.
costs-drift-edited = Edited by you

# State of a file: it is gone.
costs-drift-missing = Missing

# State of a file: the tool itself changed a key.
costs-drift-changed-by-tool = Changed by the tool

# State of a file: it came back after Undo took it away.
costs-drift-returned = Came back

# State of a file: it was left out.
costs-drift-skipped = Skipped

# State of a file or key the person took over with Keep Mine: Notchling no longer changes it.
costs-drift-yours = Yours, not managed

# Summary line of the Check list when the check could not run; the reason is shown under it.
costs-check-failed = The check can't run.

## Cache

# Section title for the Keep Warm control.
costs-keep-warm-section = Keep Warm

# Name of the Keep Warm control.
costs-keep-warm-title = Keep Warm

# Explains Keep Warm.
costs-keep-warm-note = Sends one tiny message just before a chat's cache expires, so coming back doesn't re-write the whole chat. Claude Code only.

# Badge: Keep Warm is not built yet.
costs-coming-later = Coming Later

# Section title above each tool's cache time.
costs-cache-section-title = Cache Time

# Cache time of Claude Code; $main is the main chat's time and $helpers the subagents' time, such as "1 h".
costs-cache-claude = Main chat { $main } · Helpers { $helpers }

# Where Claude Code's cache time was read from.
costs-cache-from-settings = Read from settings.json: promptCacheTtl and subagentPromptCacheTtl.

# Shown when settings.json sets no cache time.
costs-cache-default = Claude Code's defaults: 1 h for the main chat, 5 min for helpers.

# Cache time of Codex.
costs-cache-codex = Cached by OpenAI

# Cache time of Antigravity.
costs-cache-agy = Cached by Google

# Under Codex and Antigravity.
costs-cache-provider-note = Not adjustable, so Notchling doesn't change it.

# A cache time of five minutes.
costs-ttl-five-minutes = 5 min

# A cache time of one hour.
costs-ttl-one-hour = 1 h

# A cache time that is neither; $text is the value in settings.json.
costs-ttl-other = { $text }

# Note under the cache times.
costs-cache-footer = Set Up doesn't change these. Helpers stay at 5 min: a 1-hour cache costs twice as much to write and usually saves less than that in rebuilds.

## Set Up sheet

# Title of the Set Up sheet.
costs-setup-sheet-title = Set Up Cost Profile

# While the plan is read.
costs-setup-reading = Reading what is on this Mac…

# When the plan can't be read.
costs-setup-unreadable = The plan can't be read.

# Footer of the sheet when the plan or the setup record could not be read; there is no Apply then.
costs-setup-failed-footer = Nothing was changed.

# Footer of the sheet before Apply.
costs-setup-nothing-written = Nothing is written until Apply.

# Footer when nothing would change.
costs-setup-no-changes = Everything is in step, so there is nothing to apply.

# Footer while Apply runs.
costs-setup-applying = Applying…

# Footer after Apply; $count is how many files were written.
costs-setup-done = { $count ->
    [one] { $count } file written. Undo puts it back.
   *[other] { $count } files written. Undo puts them back.
}

# Footer and tip when the install switch is off.
costs-setup-install-off = The install switch is off. Turn it on in Settings > General, then Apply.

# Button that writes the plan.
costs-apply = Apply

# Button on a tool: put its files back as they were.
costs-undo = Undo

# Tool is installed here.
costs-tool-found = Found on this Mac

# Tool is not installed here.
costs-tool-missing = Not found on this Mac

# Next to a tool that is found when its version can't be told.
costs-tool-unknown-version = Version unknown

# Next to a tool that is not found.
costs-tool-skipped = Not found, skipped

# Under a tool that is found when it has no changes.
costs-group-nothing = Nothing to change.

# Under a tool's changes; $count is how many files and keys are already as planned.
costs-group-in-step = { $count ->
    [one] { $count } more is already in step.
   *[other] { $count } more are already in step.
}

# What a planned change does: a file that doesn't exist yet.
costs-change-new = New

# What a planned change does: some keys or a block inside a file the person owns.
costs-change-edit = Edit

# What a planned change does: a whole file is written again.
costs-change-replace = Replace

# What a planned change does: a file is taken away after a backup.
costs-change-remove = Remove

# What a planned change is: optional, off until ticked.
costs-change-offer = Optional

# What a planned change is: left out.
costs-change-skip = Skipped

# What a planned change is: already as planned.
costs-change-in-step = In step

# Button on a row: show what would change.
costs-diff-show = Show

# Button on a row: hide what would change.
costs-diff-hide = Hide

# Last line of a long diff; $count is how many lines are not shown.
costs-diff-more = and { NUMBER($count) } more lines

# Shown for a key in a settings file; $key is its name, $before its value now and $after its new value.
costs-key-change = { $key }: { $before } → { $after }

# Shown for a key that has no value now.
costs-key-unset = unset

# Section title above the lines taken from the person's own rules.
costs-report-title = Imported From Your Rules

# Last line of that list; $count is how many lines are left out.
costs-report-more = and { NUMBER($count) } more lines

# What happened to a line of the person's rule file: it stays.
costs-fate-kept = Kept in your file

# What happened to a line of the person's rule file: Notchling writes it again.
costs-fate-regenerated = Regenerated from the table

# What happened to a line of the person's rule file; $role is the role it set.
costs-fate-moved = Moved into the table: { $role }

# Section title above the files Apply wrote.
costs-written-title = Written

# Tip on a written file that worked.
costs-written-done = Written

# Tip on a written file that failed.
costs-written-failed = Not written

# Note on a written file; $path is where its backup is.
costs-written-backup = backup { $path }

# Section title above the Undo rows.
costs-undo-title = Undo

# Shown after an Undo that touched every file.
costs-undone-clean = Undone

# Shown after an Undo that left files alone; $count is how many.
costs-undone-conflicts = { $count ->
    [one] Undone, { $count } file left as it is
   *[other] Undone, { $count } files left as they are
}

# What Undo did to a file: put the old file back.
costs-outcome-restored = Restored

# What Undo did to a file: took away a file Notchling made.
costs-outcome-removed = Removed

# What Undo did to a file: it was already gone.
costs-outcome-gone = Already gone

# What Undo did to a file: it changed since Apply, so it was left alone.
costs-outcome-conflict = Left as it is

# What Undo did to a file: it failed.
costs-outcome-failed = Failed

## Why a change or a skip

# Under a rule file Set Up would replace; it is imported first.
costs-explain-rules-replaced = Your own model rules. Its table and lines are imported into notchling-cost.md first, then this file is replaced after a backup.

# Under a file that still names the old rule file.
costs-explain-pointer = Still names the old rule file. It will name notchling-cost.md instead.

# Under the offer of a new orchestrator agent.
costs-explain-orchestrator-new = A main chat that plans and delegates, and never edits.

# Under the change to Codex's own config file.
costs-explain-codex-config = Changes only Notchling's keys in your own config.toml.

# After a key name: Notchling left it because the person took it over.
costs-why-kept = Yours, not managed

# After a key name: it was changed after Notchling wrote it.
costs-why-changed-since = changed since Notchling wrote it, left as it is

# After a key name: the tool reads it under another name.
costs-why-alias-set = set under its other name

# After a key name: Codex does not take the value.
costs-why-codex-rejects = Codex doesn't accept it

# After a key name: the file's layout cannot hold it.
costs-why-toml-layout = the file's layout can't hold it

# A file edited since Notchling wrote it.
costs-why-edited = Edited since Notchling wrote it

# A file skipped because the new rule file it depends on is not written.
costs-why-replacement-skipped = Skipped until the new rule file is written

# A file skipped because another action names the same file.
costs-why-duplicate-id = Skipped, listed twice
