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
