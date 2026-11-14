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
chat-show-output = Show Output
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
chat-copied = Copied
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
