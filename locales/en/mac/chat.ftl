### Chat on the Mac: the Sidebar, the conversation, the reply box, the Details Panel and the window with its sheets.

## Sidebar

# The Sidebar's section titles: the sessions that wait for an answer, the ones at work and the finished ones.
chat-needs-input = Needs input
chat-working = Working
chat-done = Done
chat-no-sessions = No sessions in the last day

## Sidebar footer

# Tooltips of the footer buttons at the foot of the Sidebar.
chat-inbox-tip = Notifications that need you
chat-settings-tip = Settings: accounts, token cost and the app (⌘,)
chat-accounts-tip = Accounts: add, switch and sign out
chat-vault-tip = Secret Vault: passwords and tokens agents can use without seeing them
chat-show-sidebar = Show Sidebar
chat-hide-sidebar = Hide Sidebar
chat-new-session = New Session
chat-phone-tip-on = Phone: the bridge is on. Pair or manage phones
chat-phone-tip-off = Phone: pair a phone with this Mac

## Session menu

chat-copy-session-id = Copy Session ID
chat-session-not-started = This session hasn't started yet, so it has no id.
chat-close-session = Close Session
chat-close-all-codex = Close All { -brand-codex } Sessions
chat-close-all-antigravity = Close All { -brand-antigravity } Sessions
chat-remove-session = Remove Session…
# The title of the confirmation; $title is the session's name.
chat-remove-title = Remove “{ $title }”?
chat-remove-note = { -brand-claude-code } stops it and removes it from its list. This can't be undone.
chat-remove = Remove
chat-cancel = Cancel
# Shown in the Chat window's error line; $error is the reason.
chat-save-used-failed = Couldn't save the sessions used in Chat: { $error }
chat-save-closed-failed = Couldn't save the closed sessions: { $error }

## Account menu

chat-move-all-to = Move All Sessions To
chat-add-account = Add Account…
chat-codex-accounts = { -brand-codex } Accounts…
# Shown in the error line; $error is the reason.
chat-move-failed = Couldn't move it: { $error }

## Usage limits

# The ring's tooltip card. The 5-hour limit is the Claude window that resets every five hours.
chat-limit-five-hour = 5-hour limit
chat-limit-weekly = Weekly · all models
chat-limit-weekly-short = weekly limit
# $name is a limit's own name, such as Opus.
chat-limit-named = { $name } limit
# $name is the provider, $window the kind of limit.
chat-limit-window = { $name } · { $window }
# $plan is the account's plan, such as Team.
chat-limits-title-plan = Your usage limits · { $plan }
chat-limits-title = Your usage limits
chat-limit-resetting = Resetting
# $clock is the time of the reset, such as Fri 9:00 AM.
chat-limit-resets = Resets { $clock }
chat-limit-see-all = See all →

## Delegated jobs

# The names of the tools a session hands a job to, shown as a subagent's title.
chat-delegated-codex = { -brand-codex }
chat-delegated-antigravity = { -brand-antigravity } CLI

## Conversation

# Shown beside a stalled turn's activity line.
chat-retry = Retry
chat-retry-tip = Nothing came back: stop this turn and send the message again
chat-show-output-button = Show Output
# The dot is part of the message: it separates the line's facts.
chat-no-reply-yet = · No reply yet
# The button that jumps the conversation to its newest message.
chat-latest = Latest
chat-sending-note = Sending…
chat-queued-note = Queued
# $count is the number of pictures sent with a message.
chat-images = { $count ->
    [one] { $count } image
   *[other] { $count } images
}
chat-output = Output
chat-error = Error
chat-loading = Loading…
chat-no-sessions-yet = No sessions yet
chat-no-transcript = No transcript yet for this session
chat-no-transcript-subagent = No transcript yet for this subagent
chat-agy-loading = Loading from { -brand-antigravity }…
chat-agy-through-daemon = { -brand-antigravity } sessions load through the daemon while acting is on.
chat-show-in-finder = Show in Finder
chat-open = Open
chat-back = Back
# The title of a code block that names no language.
chat-code = code
chat-copy = Copy
chat-copy-done = Copied
chat-select-first = Select some text first.
chat-run = Run
chat-run-tip = Runs in a terminal (it asks for input)
chat-delivered = Delivered

## Conversation steps

# $duration is how long the model thought, such as 12s.
chat-thought-for = Thought for { $duration }
chat-thought-briefly = Thought briefly
# $duration is how long a turn took and $model the model's name, such as Opus 5.5.
chat-worked-for = Worked for { $duration }
chat-worked-for-model = Worked for { $duration } · { $model }
# $count is how many steps of a group failed.
chat-steps-failed = { NUMBER($count) } failed
# The title of a step's input box: a shell command, or any other input.
chat-step-command = Command
chat-step-input = Input
# Under a cut output; the text above it stays on its own line. $more is how many lines were cut.
chat-output-more = … +{ $more } lines

## Tool summaries

# What a group of tool steps did, such as "Ran 3 commands". The first letter is capitalised by the app.
chat-steps-ran-commands = { $count ->
    [one] ran { $count } command
   *[other] ran { $count } commands
}
chat-steps-read-files = { $count ->
    [one] read { $count } file
   *[other] read { $count } files
}
chat-steps-edited-files = { $count ->
    [one] edited { $count } file
   *[other] edited { $count } files
}
chat-steps-wrote-files = { $count ->
    [one] wrote { $count } file
   *[other] wrote { $count } files
}
chat-steps-searched-times = { $count ->
    [one] searched { $count } time
   *[other] searched { $count } times
}
chat-steps-browsed-pages = { $count ->
    [one] browsed { $count } page
   *[other] browsed { $count } pages
}
chat-steps-ran-agents = { $count ->
    [one] ran { $count } agent
   *[other] ran { $count } agents
}
chat-steps-updated-tasks = { $count ->
    [one] updated { $count } task
   *[other] updated { $count } tasks
}
chat-steps-asked-questions = { $count ->
    [one] asked { $count } question
   *[other] asked { $count } questions
}
chat-steps-used-skills = { $count ->
    [one] used { $count } skill
   *[other] used { $count } skills
}
chat-steps-used-tools = { $count ->
    [one] used { $count } tool
   *[other] used { $count } tools
}

## Tools

chat-reply-placeholder = Reply…
chat-message-codex-placeholder = Message { -brand-codex }…
chat-message-antigravity-placeholder = Message { -brand-antigravity }…
chat-codex-account-tip = The { -brand-codex } account. Click to switch or add one.
chat-antigravity-account-tip = The { -brand-antigravity } account. Click to switch or add one.
# $command is the slash command the user typed, such as /model.
chat-menu-opens = { $command } opens a menu: run it in this session's own terminal
# Stands in for $command when the user typed no command.
chat-this-command = This command
# $label is the account's name; the session runs in a lane.
chat-account-lane = { $label } · lane

# A line after the activity text, such as the running shells. The dot is part of the message.
chat-activity-shells = · { $shells }

## Reply box

chat-first-message-placeholder = Your first message starts the session
# $count is how many messages wait to be sent when the turn ends.
chat-messages-wait = { $count ->
    [one] { $count } message waits for this turn to end.
   *[other] { $count } messages wait for this turn to end.
}
chat-add-files = Add files or photos
chat-add-folder = Add folder
chat-slash-commands = Slash commands
chat-read-only = Read only here. Reply where the session runs.
chat-agy-busy = { -brand-antigravity } is still on the last turn. Send again when it ends.
chat-session-exited = This session has exited.
# $message is the reason, given by the tool.
chat-change-failed = Couldn't change it: { $message }
chat-drop-hint = Drop to add to your reply

## Pickers

chat-model-placeholder = Model
chat-effort-placeholder = Effort
chat-more-models = More Models
chat-model-tip = The model this session uses. Pick another to switch it (like /model in the terminal).
chat-effort-tip = How hard the model thinks. Pick another to switch it (like /effort in the terminal, which also makes it the default for new sessions).
chat-no-efforts = This model has no effort levels
chat-effort-extra-high = Extra High
# $model is the name of the model whose price is used.
chat-priced-as = Priced as { $model } until { -brand-notchling } knows this model
chat-codex-model-tip = The model this { -brand-codex } session's next turns use. { -brand-codex } Default follows { -brand-codex }'s own setting.
chat-codex-effort-tip = How hard the model reasons on this session's next turns.
chat-default-model = Default Model
chat-pick-model-first = Pick a model first.
chat-agy-not-started = Your first message starts the session; then this can be picked here.
chat-agy-opening = { -brand-antigravity } is still opening this session.
# $name is the option's name, such as Model.
chat-agy-option-tip = { $name } for this session's next turns.

## Activity line

# $count is how many background commands run.
chat-shells-running = { $count ->
    [one] { $count } background command running
   *[other] { $count } background commands running
}
# $minutes and $seconds are whole numbers.
chat-stats-minutes = { NUMBER($minutes) }m { NUMBER($seconds) }s
# $time is how long the turn has run, such as 12s, and $count the tokens received, such as 1.2k.
chat-stats-tokens = { $time } · ↓ { $count } tokens
# What a step is doing now. $rest is the rest of the step's title, such as a file name.
chat-doing-ran = Running { $rest }
chat-doing-read = Reading { $rest }
chat-doing-edited = Editing { $rest }
chat-doing-wrote = Writing { $rest }
chat-doing-searched-web = Searching the web for { $rest }
chat-doing-searched = Searching for { $rest }
chat-doing-found = Finding files matching { $rest }
chat-doing-used = Using { $rest }
chat-doing-command = Running a command
chat-thinking = Thinking…
chat-writing = Writing…

## Questions and permissions

chat-allowing = Allowing…
chat-allowing-always = Allowing, and not asking again…
chat-denying = Denying…
chat-allowing-typing = Allowing… typing it into the session, a few seconds
chat-allowing-always-typing = Allowing, and not asking again… a few seconds
chat-denying-typing = Denying… typing it into the session, a few seconds
chat-allowed = Allowed
chat-allowed-always = Always allowed
chat-denied = Denied
# $verb is Allowed or Denied and $tool the tool's name, such as Bash.
chat-decision-tool = { $verb }: { $tool }
# $subject is the file or command the decision was about.
chat-decision-tool-subject = { $verb }: { $tool } { $subject }
chat-already-answered = Already answered in the terminal: { $tool }
chat-already-answered-subject = Already answered in the terminal: { $tool } { $subject }
# $said is the reason, without its final full stop.
chat-may-have-reached = { $said }. It may still have reached the session: look before sending again.
# Replaces "Answer it in the terminal" when the question card is already on screen.
chat-answer-above = Answer it above
chat-not-answered = Not answered
# $step is the question's number and $total how many questions there are.
chat-step-of = { NUMBER($step) } of { NUMBER($total) }
chat-pick-any = Pick any that apply
chat-claude-asks = { -brand-claude } asks
chat-type-something-else = Type something else…
chat-next = Next
chat-submit = Submit
chat-allow = Allow
chat-allow-this-time = This time only
chat-allow-always = Always allow this
chat-allow-always-note = Don't ask again for this
chat-deny = Deny
chat-deny-note = { -brand-claude } tries something else
chat-permission-needed = Permission needed
# $tool is the tool's name, such as Bash.
chat-wants-to-use = { -brand-claude } wants to use { $tool }

## Second opinions

chat-second-opinion-tip = Second Opinion: ask { -brand-codex } or { -brand-antigravity } to review this
chat-no-store = Second opinions need the app's own data folder, not a fixture
chat-second-opinion = Second Opinion
# $name is the file the opinion is about.
chat-second-opinion-of = Second Opinion: { $name }
# $tool is the reviewing tool and $subject what it reviews.
chat-opinion-running = { $tool } is reviewing { $subject }
chat-opinion-done = { $tool } on { $subject }
chat-opinion-failed = { $tool } couldn't review { $subject }
chat-opinion-cancelled = { $tool } on { $subject }, cancelled
chat-opinion-cancelled-early = Cancelled before it answered.

## Tasks

chat-tasks-hide = Hide the tasks until the list changes
# $subject is the task being worked on.
chat-tasks-now = Now: { $subject }
chat-tasks-all-done = All done

## Details Panel

# The Details Panel's section titles.
chat-details-tasks = Tasks
chat-details-session = Session
chat-details-subagents = Subagents
chat-details-shells = Shells
chat-details-git = Git
# $done is how many tasks are finished and $total how many there are.
chat-details-tasks-progress = Tasks { NUMBER($done) }/{ NUMBER($total) }
chat-details-tasks-counted = Tasks · { NUMBER($done) }/{ NUMBER($total) }
# A section title with a count. $name is the section's title and $count a whole number written already.
chat-details-running = { $name } · { $count } running
chat-details-total = { $name } · { $count }
chat-details-no-tasks = No tasks
chat-details-nothing-read = Nothing read yet
chat-details-none-started = None started
chat-details-none-running = None running
# $count is how many more rows the button loads.
chat-details-load-more = Load { $count } More
# $input and $output are token counts such as 15.4k.
chat-details-tokens = { $input } in · { $output } out
# $tokens is a token count such as 3.1k and $share the percent of the input that was cached.
chat-details-cached = { $tokens } cached ({ $share }%)
# $count picks the form; $text is the same number written plainly.
chat-details-turns = { $count ->
    [one] { $text } turn
   *[other] { $text } turns
}
chat-details-commands = { $count ->
    [one] { $text } command
   *[other] { $text } commands
}
chat-status-running = Running
chat-status-failed = Failed
chat-status-stopped = Stopped
chat-status-not-running = Not running

## Details Panel, Git

chat-git-detached-at = Detached at { $head }
chat-git-detached = Detached HEAD
chat-git-no-branch = No branch
chat-git-more-not-shown = More files not shown
# $count is the number of files cut off, with a plus when git cut its own list.
chat-git-hidden = { $count } more files not shown
# $upstream is the branch being followed.
chat-git-up-to-date = { $upstream } · up to date
# $ahead and $behind are commit counts written already.
chat-git-ahead = { $upstream } · { $ahead } ahead
chat-git-behind = { $upstream } · { $behind } behind
chat-git-ahead-behind = { $upstream } · { $ahead } ahead, { $behind } behind
chat-git-no-uncommitted = No uncommitted changes
chat-git-no-changes = No changes
# $base is the branch the changes are measured against.
chat-git-vs-base = vs { $base }

## Details Panel, Changes

chat-changes-title = Changes
chat-changes-untracked = Untracked
# $folder is the file's folder and $what says Changes or Untracked.
chat-changes-subtitle = { $folder } · { $what }
chat-changes-large = Large diff
chat-changes-close = Close
chat-changes-open-in-panel = Open in File Panel
chat-changes-filter-placeholder = Filter files
chat-changes-compare-against = Compare Against
chat-changes-reading-branches = Reading the branches…
chat-changes-no-branches = There are no other branches.
chat-changes-nothing = Nothing to compare.
chat-changes-no-changes-base = No changes against the base.
chat-changes-no-match = No files match.
chat-changes-reading = Reading the changes…
chat-changes-none-to-show = There are no changes to show.
chat-changes-truncated = Only the first part of this diff is shown.
chat-changes-unreadable = The diff came back unreadable.
# $count picks the form; $text is the same number written plainly.
chat-changes-file-count = { $count ->
    [one] { $text } file
   *[other] { $text } files
}
# $files is the file count line and $count the files not yet committed.
chat-changes-uncommitted = { $files } · { $count } uncommitted

## Window

chat-window-hide-details = Hide the session details
chat-window-show-details = Show the session details: branch, model, tokens, tasks, files changed
chat-window-no-session = No session selected
# The tools' names as the header and the sheets write them.
chat-tool-claude-code = { -brand-claude-code }
chat-tool-codex = { -brand-codex }
chat-tool-antigravity = { -brand-antigravity }
# $tool is the tool's name and $host the machine the session runs on.
chat-header-host = { $tool } · { $host }
# $model is the name of the model whose context window is used.
chat-window-model-note = Window as { $model } until { -brand-notchling } knows this model

## File and page panels

chat-panel-file-gone = This file is gone.
chat-panel-file-unreadable = This file can't be read.
# $folder is the folder the file is in.
chat-panel-file-cut-subtitle = { $folder } · first 1 MB shown
chat-panel-saved-copy = Saved copy
# $date is the day the page was saved, such as Nov 9.
chat-panel-saved-copy-date = Saved copy · { $date }
chat-panel-live = Live on claude.ai
chat-panel-artifact = Artifact
chat-panel-no-pages-folder = There is no pages folder here.
chat-panel-not-a-page = Not a page.
chat-panel-opening-live = Opening the page on claude.ai…
chat-panel-opening-page = Opening the page…
chat-panel-opening-saved = Opening the saved copy…
chat-panel-file-empty = This file is empty.
chat-panel-picture-unshown = Can't show this picture.
chat-panel-file-unshown = Can't show this file here.
chat-panel-open-default = Open in Default App
chat-panel-file-cut = Only the first 1 MB is shown. Open it in its app to read the rest.
chat-panel-reload = Reload the page
chat-panel-open-browser = Open in Browser
chat-panel-show-in-finder = Show in Finder
chat-panel-close = Close
chat-panel-back-to-details = Back to Details

## Terminal panel

# $n is the terminal's number.
chat-terminal-tab = Terminal { $n }
chat-terminal-new = New Terminal
chat-terminal-minimize = Minimize: back beside the conversation
chat-terminal-expand = Expand: over the conversation too
chat-terminal-close = Close the terminal (its shells keep running)
chat-terminal-ended = Process ended
chat-terminal-button-tip = Terminal: a shell in this session's folder
# $program is the shell that did not start and $error the reason.
chat-terminal-start-failed = Couldn't start { $program }: { $error }

## Simulator panel

chat-sim-name = Simulator
chat-sim-zoom-out = Zoom Out
chat-sim-zoom-fit = Fit to the Panel
chat-sim-zoom-actual = Actual Size
chat-sim-zoom-in = Zoom In
chat-sim-booted = Booted
chat-sim-none = None
chat-sim-available = Available (will boot)
chat-sim-shut-down = Shut Down
# $device is the simulator's name.
chat-sim-shut-down-device = Shut Down { $device }
chat-sim-add = Add Simulator…
chat-sim-add-media = Add Photos or Videos…
chat-sim-open = Open in Simulator
chat-sim-detach = Detach Simulator
chat-sim-new = New Simulator
chat-sim-cancel = Cancel
chat-sim-create = Create and Boot
chat-sim-looking = Looking for a simulator…
chat-sim-waiting = Waiting for the screen…
chat-sim-none-running = No simulator is running
chat-sim-choose = Choose a Simulator
chat-sim-clear = Clear
chat-sim-add-to-reply = Add to Reply
chat-sim-home = Home
chat-sim-rotate = Rotate
chat-sim-shot-tip = Save a screenshot to the Desktop
chat-sim-stop-recording-tip = Stop recording and save the video to the Desktop
chat-sim-record-tip = Record the screen to a video on the Desktop
chat-sim-annotate-tip = Annotate: draw on the screen and add it to the reply
chat-sim-button-tip = iOS Simulator: its screen, live
# $error is the reason, given by the system.
chat-sim-list-failed = Couldn't list simulators: { $error }
# $device is the simulator's name.
chat-sim-booting = Booting { $device }…
chat-sim-boot-failed = Couldn't boot it: { $error }
chat-sim-shutting-down = Shutting down { $device }…
chat-sim-shut-down-failed = Couldn't shut it down: { $error }
chat-sim-reading-runtimes = Reading the runtimes…
chat-sim-no-runtime = No iOS runtime is installed (Xcode > Settings > Components)
# $name is the name given to the new simulator.
chat-sim-making = Making { $name }…
chat-sim-make-failed = Couldn't make it: { $error }
chat-sim-adding-photos = Adding to { $device }'s Photos…
chat-sim-added-one = Added to Photos
# $count is how many files were added.
chat-sim-added-many = Added { $count } to Photos
chat-sim-add-failed = Couldn't add them: { $error }
# $file is the name of the file that was saved.
chat-sim-saved = Saved { $file }
chat-sim-screenshot-failed = Couldn't take the screenshot: { $error }
chat-sim-recording = Recording…
chat-sim-record-failed = Couldn't record: { $error }
chat-sim-saving-recording = Saving the recording…
chat-sim-recording-unfinished = The recording didn't finish
chat-sim-annotate-note = Draw on the screen, then Add to Reply
chat-sim-annotation-added = Added to the reply
chat-sim-save-failed = Couldn't save it: { $error }

## Sheets: New Session

chat-sheet-new-session = New Session
chat-sheet-account = Account
chat-sheet-folder = Folder
chat-sheet-choose-folder = Choose Folder…
chat-sheet-choose = Choose
chat-sheet-start = Start Session
chat-sheet-trust-start = Trust Folder and Start
chat-sheet-cancel = Cancel
chat-sheet-no-accounts-lane = No accounts to make a lane with
chat-sheet-no-accounts-switch = No accounts to switch
chat-sheet-main-login = Main login
chat-sheet-lane = Lane
chat-sheet-opens-lane = Opens a lane
chat-sheet-in-use = In use
# $tool is the tool whose account it switches.
chat-sheet-switches = Switches { $tool }
# $host is the tool's name.
chat-sheet-draft-name = New { $host } Session
chat-sheet-draft-label = New
chat-sheet-untrusted = { -brand-claude-code } hasn't trusted this folder yet.
chat-sheet-starting = Starting…
chat-sheet-trusting = Trusting the folder…
# $account is the account's name, $detail what it does and $folder the folder's name.
chat-sheet-chosen = { $account } ({ $detail }) in { $folder }
chat-sheet-pick = Pick an account and a folder.
chat-sheet-trust-title = Trust This Folder?
# $folder is the folder that is not trusted yet.
chat-sheet-trust-text = { -brand-claude-code } hasn't trusted { $folder } yet. Trust Folder and Start accepts { -brand-claude-code }'s “Do you trust this folder?” prompt for it, as running claude there would, then starts the session.

## Sheets: Second Opinion

chat-sheet-default = Default
chat-sheet-run = Run
chat-sheet-close = Close
chat-sheet-prompt = Prompt
