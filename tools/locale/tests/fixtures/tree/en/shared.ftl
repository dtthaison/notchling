### Words both apps show alike.

-brand-notchling = Notchling
-brand-claude = Claude

# The language's own name, as its speakers write it.
language-name = English

## Counts

# Title of the Subagents section while some still run. $count is how many run.
session-subagents-running = { $count ->
    [one] { $count } subagent running
   *[other] { $count } subagents running
}

## Items

# A label with a number sign. $count is how many.
item-number-sign = # of items: { $count }

## States

# Shown while a session works.
state-working = Working

# Shown when the app is not running.
state-not-running = { -brand-notchling } isn't running
