### The phone: the Inbox.

# Shown in the Inbox.
inbox-needs-you = Needs You

# Shown in the Inbox.
inbox-done = Done

# Shown in the Inbox.
inbox-empty-title = Nothing Needs You

# Shown in the Inbox.
inbox-empty-body = Requests from your Mac show up here.

# Shown in the Inbox.
inbox-unsupported = Notchling on this Mac has no inbox yet.

# Shown in the Inbox.
inbox-open-on-mac = Open on Mac

# Shown in the Inbox.
# $action is the choice made on the Mac, such as Allow.
inbox-answered-on-mac = Answered on the Mac: { $action }

# Shown in the Inbox.
# $action is the choice you made, such as Allow.
inbox-answered-here = You answered: { $action }

# Shown in the Inbox.
inbox-declined = The Mac didn't take that answer. Try again on the Mac.

# Title of the question before the inbox answer that moves the primary here.
inbox-confirm-move-here-title = Move to this machine now?

# Body of that question.
inbox-confirm-move-here-body = Sessions continue here from the newest copy this machine holds. Work the old machine did after that is kept on it.

# Title of the question before an inbox answer that moves the primary although safety can't be shown.
inbox-confirm-move-anyway-title = Move anyway?

# Title of the question before the inbox answer that takes the primary back.
inbox-confirm-take-back-title = Take Back?

# Body of that question.
inbox-confirm-take-back-body = This machine becomes the primary again. Sessions that moved away come back when their turn ends.

# Title of the question before a waiting session continues from this machine's copy.
# $title is the session's name.
inbox-confirm-continue-title = Continue { $title } here?

# Body of that question.
inbox-confirm-continue-body-unnamed = The newer copy on the other machine is kept beside it.

# Title of the section at the top of the Inbox with the items an agent or a step waits on.
inbox-blocking = Waiting on You

# Shown on an item that waits, when it began less than a minute ago.
inbox-stopped-now = Stopped just now

# Shown on an item that waits.
# $count is how many whole minutes it has waited.
inbox-stopped-minutes = { $count ->
    [one] Stopped { $count } minute ago
   *[other] Stopped { $count } minutes ago
}

# Shown on an answered item while the Mac applies the answer.
inbox-outcome-applying = Applying…

# Shown on an answered item once the Mac applied the answer.
inbox-outcome-applied = Applied

# Shown on an answered item the Mac could not apply, when it gave no reason.
inbox-outcome-failed = Failed

# Shown on an answered item the Mac could not apply.
# $reason is the Mac's own explanation.
inbox-outcome-failed-reason = Failed: { $reason }

# Shown on an item that was taken back before it was answered.
inbox-outcome-withdrawn = Withdrawn

# Shown on an item that was taken back before it was answered.
# $reason is the Mac's own explanation.
inbox-outcome-withdrawn-reason = Withdrawn: { $reason }

# Shown on an item that was answered with a note for the agent.
# $note is what the person wrote.
inbox-sent-back = Sent back: { $note }
