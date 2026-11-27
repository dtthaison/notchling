### The notch: its list of agents, a peek, a session's detail and the cards that ask a question or for a permission.
### The notch has fixed sizes, so every text in it is cut with an ellipsis when it does not fit.

## Tabs and the list

# Tab of the notch's list: sessions that wait for the user.
notch-tab-input = Input

# Tab of the notch's list: sessions that are working.
notch-tab-working = Working

# Tab of the notch's list: sessions that finished.
notch-tab-done = Done

# Shown in the notch's list when no session is running at all.
notch-empty-no-agents = No { -brand-claude } agents running

# Shown in the Input tab when no session waits for the user.
notch-empty-input = Nothing needs input

# Shown in the Working tab when no session is working.
notch-empty-working = Nothing working

# Shown in the Done tab when no session finished.
notch-empty-completed = Nothing completed

# State of a session that waits for an answer while it was working a moment ago.
notch-needs-input = Needs input

# Second line of a row whose session has a message of the user's that did not go in and waits for a decision in Chat.
notch-message-needs-you = Message needs you

## Rows

# Second line of a working row when the session shows no activity yet.
notch-thinking = Thinking…

# Second line of a row whose session asks for a permission. $tool is the tool and $summary what it would do.
notch-wants-to-use = Wants to use { $tool }: { $summary }

# Second line of a row whose session asks a question. $question is the question.
notch-asks = Asks: { $question }

# Line of a row that shows the user's last message. $prompt is that message.
notch-you = You: { $prompt }

# Right end of a row: where the session runs and how long ago it last changed. $host is the place and $age the age, for example 5m.
notch-row-host = { $host } · { $age }

# Short name of the place a background session runs in, at the right end of a row.
notch-host-background = bg

# Age of a row that changed less than a minute ago.
notch-age-now = now

# Age of a row in minutes. $n is the number of minutes.
notch-age-minutes = { NUMBER($n) }m

# Age of a row in hours. $n is the number of hours.
notch-age-hours = { NUMBER($n) }h

# Age of a row in days. $n is the number of days.
notch-age-days = { NUMBER($n) }d

## Peek and detail

# Link at the right of a peek: opens the session's detail.
notch-open = Open ›

# Shown in a session's detail when it has no messages.
notch-no-messages = No messages yet

# Under a session's detail when the session has exited.
notch-exited = This session has exited.

# Under a session's detail when the notch can't type into the session.
notch-read-only = Read only here. Reply where the session runs.

# Line under the session's name in its detail. $label is its state, $host where it runs and $age how long ago it changed.
notch-detail-sub = { $label } · { $host } · { $age }

## Cards

# Tab of a question card with several questions, for a question that has no title of its own. $n is its place.
notch-question-tab = Question { NUMBER($n) }

# Next to the title of a question card with several questions. $n is the question shown and $total how many there are.
notch-question-step = { NUMBER($n) } of { NUMBER($total) }

# Title of a question card.
notch-claude-asks = { -brand-claude } asks

# Line under a question that lets the user choose more than one answer.
notch-pick-any = Pick any that apply

# Placeholder of the box where the user types an answer of their own.
notch-type-else = Type something else…

# Button of a question card with several questions: go to the next question.
notch-next = Next

# Button of a question card: send the answers.
notch-submit = Submit

# Shown under a card when the setting that lets the notch answer is off.
notch-answer-off = Answering from the notch is off in Settings.

## Blocking cards in the notch

# Name of the open blocking card in the ? map that labels the parts of a window.
notch-blocking-part = Notch Blocking

# Title of the red header of the notch while one task is stopped and waits for an answer.
notch-blocking-title = Blocking

# Title of the red header of the notch while several tasks are stopped. $n is how many.
notch-blocking-title-count = Blocking { NUMBER($n) }

# Title of the red header in the last minute before the request ends.
notch-blocking-ends-soon = Ends soon

# How long a task has been stopped. $time is a short time like 2m or 0:04.
notch-blocking-stopped = stopped { $time }

# How long is left in the last minute before a request ends. $time is minutes and seconds like 0:52.
notch-blocking-ends-in = ends in { $time }

# Line of a blocking card that says what stopped. $who is the agent, session and branch, joined.
notch-blocking-who = Stopped: { $who }

# Position of the shown card among the blocked tasks. $n is its place and $total how many there are.
notch-blocking-step = { NUMBER($n) } of { NUMBER($total) }

# Line of the notch for the two seconds after a blocked task was answered somewhere.
notch-blocking-answered = Answered

# Line of the notch for the four seconds after a blocked task's request ran out.
notch-blocking-expired = Expired

# Line under the clock of a last call. The agent gives up when the clock reaches zero.
notch-blocking-left = left to answer, then the agent gives up

# Spoken label of a button on a blocking card. $action is the button's word and $title what is asked.
notch-blocking-button-for = { $action } { $title }

# Spoken label of the stepper's buttons. $label is Previous or Next blocked task and $step the place it leads to.
notch-blocking-stepper-label = { $label }, { $step }

# Tooltip of the button that shows the previous blocked task.
notch-blocking-previous = Previous blocked task

# Tooltip of the button that shows the next blocked task.
notch-blocking-next = Next blocked task

# Line under a blocking card when the note could not be sent. The note stays in the field.
notch-blocking-note-kept = The note did not go through. It is still here.

## The picture behind the notch in a render

# Title of the made-up window behind the notch in a render.
notch-backdrop-title = { -brand-notchling }

# Title of the made-up terminal behind the notch in a render.
notch-backdrop-terminal = Terminal

# The made-up prompt line of the terminal behind the notch in a render.
notch-backdrop-prompt = ~/work/notchling $

# Second line of a row, and the title of a card, for a question an agent asked with the ask command.
notch-ask-question = Question
