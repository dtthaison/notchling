### The phone: the Sessions tab.

# A session that is working on a turn. Shown in the Chat screen and the Sessions tab.
sessions-state-running = Working

# A session that finished its turn. Shown in the Sessions tab.
sessions-state-done = Done

# Shown in the Sessions tab.
sessions-state-needs-input = Needs Input

# Shown in the Sessions tab.
sessions-state-exited = Ended

# Shown in the Sessions tab.
sessions-empty-title = No sessions yet

# Shown in the Sessions tab.
sessions-empty-body = Claude Code sessions on your Mac show up here.

# Shown in the Sessions tab.
sessions-search-hint = Search sessions

# Shown in the Sessions tab.
# $query is what you typed.
sessions-search-no-match = No sessions match “{ $query }”.

# Shown in the Sessions tab.
sessions-new-session = New session

# Shown in the Sessions tab.
sessions-new-session-folder = Folder

# Shown in the Sessions tab.
sessions-new-session-model = Model

# Shown in the Sessions tab.
sessions-model-default = Default

# Shown in the Sessions tab.
sessions-new-session-no-projects = No recent folders on the Mac.

# Shown in the Sessions tab.
sessions-new-session-confirm-title = Start a new session?

# Shown in the Sessions tab.
# $folder is the folder path.
sessions-new-session-confirm-body = Claude will run in { $folder } on the Mac.

# Shown in the Sessions tab.
sessions-new-session-open = Start

# Shown in the Sessions tab.
sessions-new-session-started = The Mac started a new session.

# Shown in the Sessions tab.
sessions-new-session-failed = Couldn't start a new session. Please try again.

# Shown in the Sessions tab.
# $count is the number of subagents.
sessions-running-subagents = { $count ->
    [one] { NUMBER($count) } subagent running
   *[other] { NUMBER($count) } subagents running
}

# Shown in the Sessions tab.
# $count is the number of subagents.
sessions-subagents-show = { $count ->
    [one] Show { NUMBER($count) } subagent
   *[other] Show { NUMBER($count) } subagents
}

# Shown in the Sessions tab.
# $count is the number of subagents.
sessions-subagents-hide = { $count ->
    [one] Hide { NUMBER($count) } subagent
   *[other] Hide { NUMBER($count) } subagents
}

# Shown in the Sessions tab.
sessions-just-now = Just now

# Shown in the Sessions tab.
# $n is minutes.
sessions-minutes-short = { NUMBER($n) } min

# Shown in the Sessions tab.
# $n is hours.
sessions-hours-short = { NUMBER($n) } h

# Shown in the Sessions tab.
# $n is days.
sessions-days-short = { NUMBER($n) } d

# Shown in the Sessions tab.
sessions-computer-no-sessions = No sessions

# Shown in the Sessions tab.
sessions-all-projects = All projects

# Shown in the Sessions tab.
sessions-archive = Archive

# Shown in the Sessions tab.
# $name is the session name.
sessions-archived-toast = Archived { $name }

# Shown in the Sessions tab.
sessions-undo = Undo

# Shown in the Sessions tab.
sessions-filtered-empty = No sessions in this project.

# Shown in the Sessions tab.
sessions-takeover-short = Take over

# Read aloud by a screen reader for the heading of a group in the session list. $state is the group's name, $count is how many sessions it holds.
sessions-group-label = { $count ->
    [one] { $state }, { NUMBER($count) } session
   *[other] { $state }, { NUMBER($count) } sessions
}
