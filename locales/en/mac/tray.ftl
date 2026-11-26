### The menu bar item: its menu's Blocking rows and what its icon tells a screen reader.

## Blocking rows

# Header of the menu bar menu when tasks are stopped waiting for an answer; $count is how many.
tray-blocking-header = { $count ->
    [one] Blocking { $count } Task
   *[other] Blocking { $count } Tasks
}
# One Blocking row in the menu bar menu. $title is what is asked, $who the agent, $waited how long it has waited, such as 3m.
tray-blocking-row = { $title } · { $who } · { $waited }
# One Blocking row in the menu bar menu when the agent is not known. $title is what is asked, $waited how long it has waited, such as 3m.
tray-blocking-row-alone = { $title } · { $waited }
# Item in a Blocking row's submenu that opens the Inbox.
tray-open-in-inbox = Open in Inbox

## Screen reader

# What a screen reader says for the menu bar icon when nothing needs attention.
tray-a11y-idle = Notchling
# What a screen reader says for the menu bar icon while agents work; nothing waits on you.
tray-a11y-working = Notchling, agents working
# What a screen reader says for the menu bar icon when tasks wait on you but none is blocked; $count is how many.
tray-a11y-waiting = { $count ->
    [one] Notchling, { $count } waits on you
   *[other] Notchling, { $count } wait on you
}
# What a screen reader says for the menu bar icon when tasks are blocked; $count is how many.
tray-a11y-blocked = Notchling, { $count } blocked
