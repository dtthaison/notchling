### The phone: notifications, and the line the background service shows while the app watches.

# Not shown yet.
notifications-needs-input-title = Session needs input

# Not shown yet.
# $name is the session name.
notifications-needs-input-body = { $name } is waiting.

# The name of the Android notification channel for sessions that need an answer. Shown in the system's notification settings.
notifications-channel-needs-you = Sessions that need you

# The name of the Android notification channel for sessions that finished.
notifications-channel-finished = Finished sessions

# The name of the Android notification channel for new messages from Claude.
notifications-channel-messages = New messages from Claude

# The text of the grouped notification that stands for several new ones.
notifications-summary-body = Something new

# The text of a notification when Claude waits for an answer and has no question to show.
notifications-needs-body = Needs your answer

# The text of a notification when a session finished.
notifications-finished-body = Done

# The line of the permanent notification while the app watches and no session is working.
notifications-watching = Watching

# The line of the permanent notification while the app watches sessions that work or wait.
# $count is the number of those sessions.
notifications-watching-sessions = { $count ->
    [one] Watching · { $count } session
   *[other] Watching · { $count } sessions
}
