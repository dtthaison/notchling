# What an Inbox row says once a question an agent asked is answered. $by is Mac, Phone or main session; $answer is the choice picked.
inbox-question-answered-with = Answered · { $by }: { $answer }
inbox-question-answered-in-chat = Answered in Chat
inbox-question-dismissed = Dismissed
inbox-question-by-main = main session
# Shown under the Inbox when you allowed a machine's switch request but that machine has left the cluster, so the switch could not be turned on.
inbox-worker-switch-gone = That machine is no longer in the cluster.
# Title of a request from another machine in the cluster to turn on one of its switches. $switch is what it asks, such as "Turn on Act on Sessions"; $machine is the start of that machine's id.
inbox-worker-switch-title = { $switch } on machine { $machine }

## Banners for requests that wait on the user

# Body of a banner posted again while a request still waits. $waited is how long it has waited, such as 1m.
inbox-banner-pulse-body = Stopped { $waited }
# Title of a banner after three minutes of waiting. $who is the agent and where it runs; $waited is how long it has waited, such as 3m.
inbox-banner-stronger-title = { $who } has been stopped { $waited }
# Title of a banner one minute before a request ends. $title is what is asked.
inbox-banner-last-call-title = Ends in 1 min: { $title }
# Title of a banner when a request ended unanswered. $title is what is asked.
inbox-banner-expired-title = Expired: { $title }
# Name of the agent in a banner when its session is not known.
inbox-banner-who-alone = An agent
# Shown on an Inbox row when a banner's button was pressed after the request had already ended.
inbox-banner-already-ended = This request already ended.

## Warning when nothing can alert

# Shown at the top of the Inbox when macOS notifications are off and the notch is hidden, so nothing can tell you an agent is stopped.
inbox-no-channel = Notifications are off and the notch is hidden. Nothing can tell you an agent is stopped.

## The Inbox's triage view

# Title of the Inbox panel.
inbox-title = Inbox
# Heading of the section above the tabs that lists agents stopped until you answer.
inbox-blocking-title = Blocking
# Count next to the Blocking heading. $count is how many tasks are stopped.
inbox-blocking-count = { $count ->
    [one] 1 task
   *[other] { $count } tasks
}
# Line under the Blocking heading.
inbox-blocking-hint = Answer to let them go on.
# Title of the Inbox when no agent is stopped and nothing is open.
inbox-calm-title = Nothing Is Blocked
# Line under the title of the calm Inbox.
inbox-calm-body = Every agent can go on.
# Line of the calm Inbox. $title is the request answered last; $result says how, such as Allowed; $ago is how long ago.
inbox-last-answer = Last answer: { $title }, { $result }, { $ago } ago
# Tab of the Inbox: requests that wait for an answer and do not stop an agent.
inbox-tab-waiting = Waiting
# Tab of the Inbox: answers that are being applied right now.
inbox-tab-running = Running
# Tab of the Inbox: answers that failed, requests that ran out, requests withdrawn.
inbox-tab-didnt-work = Didn't Work
# Tab of the Inbox: notices that need no answer.
inbox-tab-info = Info
# Tab of the Inbox: everything closed.
inbox-tab-done = Done
# Line under the Waiting tab when it is empty.
inbox-empty-waiting = Nothing waits for you.
# Line under the Running tab when it is empty.
inbox-empty-running = Nothing is being applied.
# Line under the Didn't Work tab when it is empty.
inbox-empty-didnt-work = Nothing failed.
# Line under the Info tab when it is empty.
inbox-empty-info = No news.
# Line under the Done tab when it is empty.
inbox-empty-done = Nothing is done yet.
# Chip on a Blocking row: how long the agent has been stopped. $age is a short time such as 3m.
inbox-stopped-for = stopped { $age }
# Pill on an open row: time left. $left is a short time such as 12:00.
inbox-pill-ends-in = ends in { $left }
# Pill on an open row that never ends.
inbox-pill-no-end = no end
# Pill on a row whose answer is being applied.
inbox-pill-applying = Applying
# Pill on a row whose answer was applied.
inbox-pill-applied = Applied
# Pill on a row whose answer did not work.
inbox-pill-failed = Failed
# Pill on a row whose answer did not work. $why is the reason.
inbox-pill-failed-why = Failed: { $why }
# Pill on a row the agent no longer waits for.
inbox-pill-withdrawn = Withdrawn
# Pill on a row that was denied with a note for the agent.
inbox-pill-sent-back = Sent back
# Line of a row that was denied with a note. $note is what you wrote.
inbox-sent-back-note = Sent back: { $note }
# Chip on a row: the session and its branch. $session is the session's name; $branch is its branch.
inbox-chip-session = { $session } · { $branch }
# Button on a Didn't Work row: moves it to Done.
inbox-clear = Clear
# Button on a Blocking row: opens the card in the Inbox.
inbox-open-card = Open Card
# Line under the field where you write a note for the agent.
inbox-change-hint = The agent gets your note and may ask again; that needs a new yes.
# Counter next to the note field, shown past 250 characters. $count is the characters written; $max is the most allowed.
inbox-change-count = { $count }/{ $max }
# Line at the bottom of the Inbox that lists the keys.
inbox-keys-hint = ↑ ↓ move · Return opens · ⌘D denies · Allow needs a click
# Shown under a row when Open on a Machines request is chosen.
inbox-machines-hint = Open Machines from the menu bar.

# Result words shown on a closed row: what was done with the request.
inbox-decided-continued = Continued
inbox-decided-cancelled = Cancelled
inbox-decided-done = Done
inbox-decided-saved = Saved
inbox-decided-sent = Sent
inbox-decided-discarded = Discarded
inbox-decided-moved = Moved
inbox-decided-taken-back = Taken Back
inbox-decided-kept = Kept
inbox-decided-continued-here = Continued Here
inbox-decided-waiting = Waiting
inbox-decided-stopped = Stopped
inbox-decided-kept-waiting = Kept Waiting
inbox-decided-answered = Answered
inbox-decided-expired = Expired
inbox-decided-withdrawn = Withdrawn
# Who answered a request, from the Mac the app runs on.
inbox-by-mac = Mac
# Who answered a request, from the phone.
inbox-by-phone = Phone
