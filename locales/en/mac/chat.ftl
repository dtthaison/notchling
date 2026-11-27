### Chat on the Mac: the Sidebar, the conversation, the reply box, the Details Panel and the window with its sheets.

## Sidebar

# The Sidebar's section titles: the sessions that wait for an answer, the ones at work and the finished ones.
chat-needs-input = Needs input
chat-working = Working
chat-done = Done
chat-no-sessions = No sessions in the last day

# The link at the foot of the Sidebar list that opens or closes the section of ended and older sessions, the section's title and the row that loads more of them.
chat-show-completed = Show Completed
chat-hide-completed = Hide Completed
chat-completed = Completed
chat-load-more = Load More
chat-completed-none = Nothing completed yet

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
chat-resume = Resume
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
# The caption under your message when Claude Code took it together with another one as a single prompt.
chat-merged = Sent together with another message
# $count is the number of pictures sent with a message.
chat-images = { $count ->
    [one] { $count } image
   *[other] { $count } images
}
chat-output = Output
# The title of the live text under a shell command that has not finished; its output is read off the session's terminal.
chat-output-running = Running…
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
chat-copy-link = Copy Link
chat-copy-path = Copy Path
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
chat-step-script = Script
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
chat-steps-ran-scripts = { $count ->
    [one] ran { $count } script
   *[other] ran { $count } scripts
}
chat-steps-used-chrome = { $count ->
    [one] used { -brand-chrome } { $count } time
   *[other] used { -brand-chrome } { $count } times
}

## Chrome steps

# Rows for what a Claude Code session did in the user's Chrome through the Claude in Chrome extension.
# $site is a web address such as example.com.
chat-browser-open = Opened { $site }
chat-browser-open-page = Opened a page
chat-browser-new-tab = New tab
# $site is a web address such as example.com.
chat-browser-new-tab-site = New tab: { $site }
chat-browser-close-tab = Closed tab
chat-browser-tabs = Checked open tabs
chat-browser-click = Clicked
# $what is the session's own short description of the click.
chat-browser-click-what = Clicked: { $what }
chat-browser-type = Typed text
# $what is the session's own short description of what it typed and where.
chat-browser-type-what = Typed text: { $what }
# $key is the name of a key such as Enter.
chat-browser-key = Pressed { $key }
chat-browser-key-none = Pressed a key
chat-browser-scroll = Scrolled the page
chat-browser-scroll-up = Scrolled up
chat-browser-scroll-down = Scrolled down
chat-browser-scroll-left = Scrolled left
chat-browser-scroll-right = Scrolled right
chat-browser-hover = Moved the pointer
chat-browser-drag = Dragged
chat-browser-screenshot = Took a screenshot
chat-browser-zoom = Zoomed in on the page
chat-browser-wait = Waited
chat-browser-read-page = Read the page layout
chat-browser-read-text = Read the page
# $what is what the session looked for.
chat-browser-find = Looked for “{ $what }”
chat-browser-find-none = Looked for an element
chat-browser-console = Read console
# $count is how many errors the console held.
chat-browser-console-errors = { $count ->
    [one] Read console (1 error)
   *[other] Read console ({ $count } errors)
}
chat-browser-network = Read network requests
chat-browser-script = Ran a script
chat-browser-fill = Filled in a form field
chat-browser-upload = Uploaded a file
chat-browser-resize = Resized the window
chat-browser-switch = Switched browser
chat-browser-shortcut = Used a shortcut
chat-browser-recording = Made a recording
chat-browser-other = Used { -brand-chrome }
# A batch of steps run in one call. $count is how many.
chat-browser-batch = { $count ->
    [one] 1 browser step
   *[other] { $count } browser steps
}
chat-browser-not-connected = { -brand-chrome } isn't connected
chat-browser-not-connected-hint = Open { -brand-chrome } and check that the Claude in Chrome extension is on and signed in to the same account.
chat-browser-needs-permission = Waiting for permission in the { -brand-chrome } extension
chat-browser-needs-permission-hint = Allow the site in the Claude in Chrome extension, then ask again.
chat-browser-denied = Permission was refused in { -brand-chrome }
chat-browser-denied-hint = The site was not allowed in the Claude in Chrome extension.
# Under a step that took a screenshot Chat does not hold: an older one, or one too large to read.
chat-browser-shot-missing = Screenshot not kept

## Tools

chat-reply-placeholder = Reply…
chat-message-codex-placeholder = Message { -brand-codex }…
chat-message-antigravity-placeholder = Message { -brand-antigravity }…
chat-antigravity-draft-placeholder = Write a draft for { -brand-antigravity }…
chat-antigravity-send-tip = Open this in { -brand-antigravity } as a draft
chat-antigravity-draft-note = Opened in { -brand-antigravity } as a draft. Press Send there.
chat-antigravity-missing = { -brand-antigravity } isn't installed on this Mac.
chat-antigravity-remote-placeholder = Write a message to paste into { -brand-antigravity } Remote Control…
chat-antigravity-remote-send-tip = Copy this and open the conversation in { -brand-antigravity } Remote Control
chat-antigravity-remote-note = Opened in { -brand-antigravity } Remote Control. Your message is copied: paste it there and press Send.
chat-panel-antigravity = { -brand-antigravity } Remote Control
chat-panel-antigravity-subtitle = Remote Control on antigravity.google.com
chat-panel-opening-antigravity = Opening Remote Control…
chat-panel-antigravity-hint = Use password or phone sign-in; passkeys don't work here.
chat-antigravity-remote-missing = Turn on Remote Control in { -brand-antigravity } Settings > Application.
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
# The status lines under a message Notchling keeps until the session takes it.
chat-outbox-queued = Queued
chat-outbox-retry = Couldn't reach the session, trying again
# The session said it can't wake, because its conversation is open in another terminal or in Claude Desktop. Claude Code only lets one place hold a conversation. The message stays here and goes in once it is closed there.
chat-outbox-wake-elsewhere = Open somewhere else, so it can't wake. Close it there, trying again
# $why is what Claude Code said when the session couldn't wake, on one line and cut when long.
chat-outbox-wake = Couldn't wake the session: { $why }. Trying again
# The session couldn't wake and gave no reason.
chat-outbox-wake-unknown = Couldn't wake the session. Trying again
# The session's status could not be read yet (the Mac may be busy). Notchling keeps looking; nothing is wrong yet.
chat-outbox-checking = Checking the session…
# The session's status could not be read for a long while. The message stays queued and goes on when it can be read; the terminal shows the session itself.
chat-outbox-unreadable = Can't read the session's status
chat-outbox-dialog = Waiting for a dialog
# The session's Claude login expired, so a message to it cannot go in until you sign in with /login in its terminal. The message stays here.
chat-outbox-login-needed = Sign in needed
# The banner above the message box when the session's Claude login expired.
chat-login-needed-title = Sign in needed
chat-login-needed-body = This session's Claude login expired. Sign in with /login in its terminal. Your message is kept.
# The message was typed and the session asked you something because of it, such as /model asking before it switches.
chat-outbox-answer = Needs your answer
# The session asked something because of the message, and after a long wait Notchling still could not see it answered or closed.
chat-outbox-answer-unseen = No answer seen in time
chat-outbox-turn = Waiting for the turn to end
# $count is how many background tasks the session has running; the message goes in when they end.
chat-outbox-busy-tasks = { $count ->
    [one] Waiting: { $count } background task running
   *[other] Waiting: { $count } background tasks running
}
# $count is how many background shell commands the session has running.
chat-outbox-busy-commands = { $count ->
    [one] Waiting: { $count } background command running
   *[other] Waiting: { $count } background commands running
}
chat-outbox-busy-replying = Waiting: { -brand-claude } is replying
chat-outbox-busy-unconfirmed = Waiting: an earlier message is unconfirmed
# $tool is the tool's name (Claude Code, Codex or Antigravity).
chat-outbox-wait-queue = Waiting for { $tool } to read the message before
# A message behind one that is unsure, in the box, held by Edit, or in the tool's queue.
chat-outbox-wait-unsure = Waiting for the message before
# The message is open in the reply box and the postman will not take it.
chat-outbox-editing-row = Editing
chat-outbox-sending = Sending…
# $tool is the tool's name.
chat-outbox-in-queue = In { $tool }'s queue
chat-outbox-sent = Sent
chat-outbox-maybe = May already be in the session
# $time is minutes and seconds, such as 1:42: when the messages behind an unsure one go on.
chat-outbox-hold-left = Next messages go in { $time }
# $count is how many messages Notchling joined into one before sending.
chat-outbox-joined = { $count ->
    [one] { $count } message
   *[other] { $count } messages
}
chat-outbox-tool-claude = { -brand-claude-code }
chat-outbox-tool-codex = { -brand-codex }
chat-outbox-tool-antigravity = { -brand-antigravity }
# $text is what the session's terminal box holds, on one line and cut when long.
chat-outbox-box = Box has other text: { $text }
# The session's terminal box already holds this very message, typed earlier and not sent.
chat-outbox-box-same = This message is already in the box
chat-outbox-stopped = Session stopped
chat-outbox-gone = Session not found
# The buttons on those messages.
chat-outbox-edit = Edit
chat-outbox-remove = Remove
chat-outbox-retry-send = Retry
chat-outbox-mark-sent = Mark as Sent
# The button that deletes the text in the session's terminal box, then sends the message.
chat-outbox-clear-send = Clear Box and Send
chat-outbox-resume-send = Resume and Send
# Opens the session's own terminal, where its dialog is drawn.
chat-outbox-show-terminal = Show Terminal
# The line above the reply box while a queued message is being edited.
chat-outbox-editing = Editing a queued message
# The note when a queued message was taken before its edit went in.
chat-outbox-already-sent = Already sent
# The note when a queued message got more text (another message joined it) while it was being edited. The box then shows its current text.
chat-outbox-changed = It changed while you edited it. Check the text, then press Return again.
# The same note when the box was reset to the message's current text, so the typed edit is gone.
chat-outbox-changed-shown = It changed while you edited it. Your edit was not saved; the box shows its current text.
# Shown when the daemon did not answer in time because the Mac is busy. Nothing was lost; asking again works.
chat-daemon-busy = Notchling is busy. Try again.
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
# Shown when an answer to a question or permission got no reply because the Mac is busy.
chat-busy-may-have-reached = Notchling is busy. Your answer may still have reached the session: look before answering again.
# Replaces "Answer it in the terminal" when the question card is already on screen.
chat-answer-above = Answer it above
chat-not-answered = Not answered
# $step is the question's number and $total how many questions there are.
chat-step-of = { NUMBER($step) } of { NUMBER($total) }
chat-pick-any = Pick any that apply
chat-claude-asks = { -brand-claude } asks
# $label is the option of the session's dialog that was picked.
chat-dialog-chose = Answered: { $label }
# $label is the option that was picked in a dialog the session no longer shows.
chat-dialog-chose-gone = Already answered in the terminal
chat-dialog-dismiss = Dismiss
chat-dialog-dismissed = Dismissed in the terminal
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
# $description is what the command does, in Claude's words.
chat-wants-to-run = { -brand-claude } wants to run: { $description }
# $tool is the tool's name and $description what it does, in Claude's words.
chat-wants-to-use-for = { -brand-claude } wants to use { $tool }: { $description }
chat-show-all = Show all
chat-show-less = Show less

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
# A running background shell command, with how long it has run. $n is whole minutes, $hours and $days are whole hours and days.
chat-shell-running-minutes = Running { NUMBER($n) } min
chat-shell-running-hours = Running { NUMBER($hours) } h { NUMBER($minutes) } min
chat-shell-running-days = Running { NUMBER($days) } d { NUMBER($hours) } h
# The button under a running shell command that ends the processes writing its output.
chat-shell-stop = Stop
# The hint under a running shell command after Stop could not end every process writing its output.
chat-shell-left = Couldn't stop everything. Ask the agent, or end it in Terminal.
# The hint under a running shell command that is probably waiting for nothing.
chat-shell-stuck-itself = Looks stuck: it waits for a process that is itself
chat-shell-stuck-loop = Looks stuck: a wait loop that has run over 30 min
# The hint under a running shell command that has used no CPU and written no output for a while. $n is whole minutes.
chat-shell-no-progress = No progress for { NUMBER($n) } min

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

## Details Panel, Branches

# $files is the file count line; a branch without a folder has no uncommitted files to count.
chat-changes-committed-only = { $files } · Committed work only
chat-git-this-session = this session
# $count is the number of uncommitted files, with a plus when git cut its own list.
chat-git-uncommitted = { $count } uncommitted
# $age is how long ago, already written.
chat-git-last-ago = last { $age } ago
chat-git-last-now = last just now
# $upstream is the branch being followed, or its remote when the names match.
chat-git-even-with = even with { $upstream }
# $count is a commit count written already.
chat-git-to-push = { $count } to push
chat-git-only-behind = { $count } behind
chat-git-push-and-behind = { $ahead } to push, { $behind } behind
chat-git-no-upstream = no upstream
chat-git-upstream-gone = upstream gone
chat-git-base-ahead = { $count } ahead
chat-git-base-ahead-behind = { $ahead } ahead, { $behind } behind
# $age is how long it has been running, already written.
chat-git-running-for = running { $age }
# $count is how many more sessions or agents work in the same folder.
chat-git-more-matches = +{ $count }
chat-git-needs-input = needs input
chat-git-folder-missing = folder missing
chat-git-no-common = no common history
# $default is the main branch's name; $count picks the form; $text is the same number written plainly.
chat-git-default-and = { $count ->
    [one] { $default } and { $text } branch
   *[other] { $default } and { $text } branches
}
chat-git-other-branches = { $count ->
    [one] { $text } other branch
   *[other] { $text } other branches
}
# $rest is the other branches line that follows.
chat-git-more-worktrees = { $count ->
    [one] { $text } more worktree · { $rest }
   *[other] { $text } more worktrees · { $rest }
}
# Alone when no other branch is left to count.
chat-git-more-worktrees-only = { $count ->
    [one] { $text } more worktree
   *[other] { $text } more worktrees
}
chat-branches-title = Branches
chat-branches-tab-working = Working
chat-branches-tab-not-merged = Not Merged
chat-branches-tab-merged = Merged
# $tab is the tab's name and $count the number of branches in it.
chat-branches-tab-count = { $tab } ({ $count })
chat-branches-filter = Filter Branches
chat-branches-worktrees = Worktrees
chat-branches-no-worktree = No Worktree
chat-branches-main-checkout = main checkout
chat-branches-local = local
chat-branches-github-only = GitHub only
# $remote is the remote's name.
chat-branches-remote-only = { $remote } only
# $name is the session's title and $state says running, needs input or idle.
chat-branches-session = “{ $name }” { $state }
chat-branches-state-running = running
chat-branches-state-idle = idle
# $age is how long ago, already written; $subject is the commit's first line.
chat-branches-last-commit = last commit { $age } ago
chat-branches-last-commit-subject = last commit { $age } ago “{ $subject }”
chat-branches-last-commit-now = last commit just now
chat-branches-last-commit-now-subject = last commit just now “{ $subject }”
# $base is the branch it is merged into.
chat-branches-merged-into = merged into { $base }
chat-branches-not-merged = not merged
chat-branches-detached = detached
# $age is how long ago the remotes were fetched, already written.
chat-branches-fetched = Remote branches as of { $age } ago
chat-branches-fetched-now = Remote branches as of just now
chat-branches-never-fetched = Never fetched
chat-branches-reading = Reading the branches…
chat-branches-cant-read = Can't read the branches.
chat-branches-no-other-worktrees = No other worktrees.
chat-branches-empty = No branches here.
chat-branches-no-match = No branches match.
chat-branches-truncated = Not all branches are shown.
chat-branches-changes = Changes
chat-branches-open-session = Open Session

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
chat-panel-show-folder = Show this page's folder in Finder
chat-panel-close = Close
chat-panel-copy-path = Copy Path
chat-panel-drop-open = Drop to Open
chat-panel-drop-folder = Folders can't be opened here. Drop a file.
# $line and $column say where the JSON stops making sense.
chat-panel-json-invalid = Not valid JSON (line { $line }, column { $column }). Showing the text as it is.
# $count is how many of the file's last lines are shown.
chat-panel-jsonl-shown = Showing the last { $count } lines
chat-panel-load-more = Load More
chat-panel-loading = Loading…
chat-panel-line-cut = This line is cut at 256 KB.
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
chat-sheet-no-accounts-lane = No accounts to make a lane with
chat-sheet-no-accounts-switch = No accounts to switch
chat-sheet-main-login = Main login
chat-sheet-lane = Lane
chat-sheet-opens-lane = Opens a lane
chat-sheet-in-use = In use
# A switch in New Session for a Claude Code session: start it with Claude in Chrome turned on.
chat-sheet-use-chrome = Use { -brand-chrome }
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

## Ask cards

# The line above an Ask card: who asks. A session's own question and one of its subagents' read differently.
chat-ask-main = This session asks
chat-ask-unknown = This session or one of its subagents
chat-ask-subagent = A subagent
chat-ask-codex = { -brand-codex } job
chat-ask-antigravity = { -brand-antigravity } job
# $head and $tail are two parts of the card's header line.
chat-ask-join = { $head } · { $tail }
# How long a question has waited. $n is whole minutes or whole hours; min stands for minutes and h for hours.
chat-ask-waited-now = just now
chat-ask-waited-minutes = waited { NUMBER($n) } min
chat-ask-waited-hours = waited { NUMBER($n) } h
# Who answered a question.
chat-ask-by-mac = Mac
chat-ask-by-phone = Phone
chat-ask-by-main = main session
# The one line a closed Ask card keeps. $by is who answered and $answer is the answer, cut when long.
chat-ask-answered = Answered
chat-ask-answered-by = Answered · { $by }
chat-ask-answered-with = Answered · { $by }: { $answer }
chat-ask-dismissed = Dismissed
chat-ask-expired = No answer in time
chat-ask-gone = Stopped waiting
# The note when another answer was taken first. $by is who answered.
chat-ask-already = Already answered
chat-ask-already-by = Already answered · { $by }
# The box and buttons on an open Ask card.
chat-ask-type-answer = Type an answer…
chat-ask-send = Send
chat-ask-dismiss = Dismiss

# The /chrome command and the Use Chrome menu item: what Chat shows while it turns Chrome on or off for a session.
chat-chrome-use = Use { -brand-chrome }
chat-chrome-suggest-on = Turn on { -brand-chrome } for this session
chat-chrome-suggest-off = Turn off { -brand-chrome } for this session
chat-chrome-title = { -brand-chrome } for This Session
chat-chrome-turning-on = Turning on { -brand-chrome }… the session restarts if it has to, and the conversation is kept
chat-chrome-turning-off = Turning off { -brand-chrome }… the session restarts if it has to, and the conversation is kept
chat-chrome-restarting-on = Turning on { -brand-chrome }… restarting the session, the conversation is kept
chat-chrome-restarting-off = Turning off { -brand-chrome }… restarting the session, the conversation is kept
chat-chrome-connecting = Connecting to { -brand-chrome }…
chat-chrome-wait-turn = Waiting for the turn to end
chat-chrome-wait-box = Waiting for the message box to empty
chat-chrome-wait-question = Waiting for the open question to be answered
chat-chrome-already-on = { -brand-chrome } is on for this session
chat-chrome-is-on = { -brand-chrome } is on
chat-chrome-is-off = { -brand-chrome } is off
chat-chrome-already-off = { -brand-chrome } is already off for this session
chat-chrome-needs-you = { -brand-chrome } needs a hand
chat-chrome-status = Status
chat-chrome-status-on = On for this session
chat-chrome-status-off = Off for this session
chat-chrome-status-unknown = Not known yet
chat-chrome-tasks = { $count ->
    [one] A background task is running. Restarting the session stops it.
   *[other] { $count } background tasks are running. Restarting the session stops them.
}
chat-chrome-tasks-unknown = Notchling can't tell whether this session has background work. Restarting the session stops any.
chat-chrome-restartable = { -brand-chrome } can only be changed for sessions Notchling can restart.
chat-chrome-no-extension = The Claude in { -brand-chrome } extension isn't installed. Install it from the { -brand-chrome } Web Store, then sign in with the Claude account this session uses.
chat-chrome-not-connected = The extension isn't connected. Open { -brand-chrome }, check that the Claude in { -brand-chrome } extension is on and signed in to the same Claude account as this session, then try again.
chat-chrome-mcp-off = Claude in { -brand-chrome } is turned off in this session's tools. Open the terminal and use /chrome there to turn it on.
chat-chrome-unavailable = This version of Claude Code has no /chrome command. Update Claude Code, then try again.
chat-chrome-busy-turn = The turn didn't end in time. Try again when it has.
chat-chrome-busy-box = The message box still holds text. Send or clear it, then try again.
chat-chrome-busy-question = A question is still open in the session. Answer it, then try again.
# $text is Claude Code's or Notchling's own words for what went wrong.
chat-chrome-failed = { $text }
chat-chrome-help-title = Good to Know
chat-chrome-help-install = Install the Claude in { -brand-chrome } extension from the { -brand-chrome } Web Store.
chat-chrome-help-account = Sign in to the extension with the same Claude account this session uses.
chat-chrome-help-profiles = Several { -brand-chrome } profiles with the extension can confuse it: keep it in one profile.
chat-chrome-try-again = Try Again
chat-chrome-turn-on = Turn On
chat-chrome-turn-on-anyway = Turn On Anyway
chat-chrome-turn-off = Turn Off
chat-chrome-turn-off-anyway = Turn Off Anyway
chat-chrome-open-terminal = Open Terminal

# The Reload Session menu item and its card: restarts a session's process with the same conversation, to pick up new tools, settings, skills and hooks.
chat-reload-item = Reload Session
chat-reload-title = Reload Session
chat-reload-lead = Reloads the session to pick up new tools. The conversation is re-read.
chat-reload-cost = Reloads the session to pick up new tools. The conversation is re-read: { $tokens } tokens, about { $cost } on { $model }.
chat-reload-tokens = Reloads the session to pick up new tools. The conversation is re-read: { $tokens } tokens.
chat-reload-reload = Reload
chat-reload-cancel = Cancel
chat-reload-cancel-reload = Cancel Reload
chat-reload-close = Close
chat-reload-try-again = Try Again
chat-reload-copy = Copy Command
chat-reload-wait-turn = Waiting for the turn to end…
chat-reload-wait-dialog = Waiting for the open question to be answered…
chat-reload-wait-tasks = { $count ->
    [one] 1 helper running: reload when it finishes?
   *[other] { $count } helpers running: reload when they finish?
}
chat-reload-wait-note = The session reloads on its own once it can. Cancel to leave it as it is.
chat-reload-row = Reloading…
chat-reload-starting = Starting the reload…
chat-reload-cancelling = Cancelling the reload…
chat-reload-working = Reloading… the conversation is kept
chat-reload-done = Session reloaded
chat-reload-done-new = Session reloaded. It continues as a new session id.
chat-reload-busy-turn = The turn didn't end in time. Try again when it has.
chat-reload-busy-dialog = A question is still open in the session. Answer it, then try again.
chat-reload-busy-tasks = Background work was still running. Try again when it has finished.
chat-reload-tasks-unknown = Notchling can't tell whether this session has background work. Reloading the session stops any.
chat-reload-failed = { $text }
chat-reload-terminal = This session runs in a terminal Notchling didn't start, so it can't restart it. Quit the session there, then run this in the same folder:
chat-reload-terminal-command = claude --resume { $id }

# The tooltip of the ⋯ button in Chat's session header: it opens the same menu as a right click on the session's row.
chat-session-menu-tip = Session Menu

# Strip above the reply box when the session's agent is stopped on a card. $title is what the card asks.
chat-blocked-strip = Waiting for your answer: { $title }. Replies here go to the agent, not the card.
# Button on that strip: opens the card in the Inbox.
chat-blocked-open = Open Card
