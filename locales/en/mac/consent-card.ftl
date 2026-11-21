### The Inbox card that asks you to allow what an agent asked for: the facts Notchling worked out, the agent's own words, and Allow or Deny.
### The maintainer reviews every change to this file, in every language.

## Who asks

# Names the tool that asked, on the consent card in the Inbox.
consent-card-tool-claude = Claude Code
# Names the tool that asked, on the consent card in the Inbox.
consent-card-tool-codex = Codex
# Names the tool that asked, on the consent card in the Inbox.
consent-card-tool-antigravity = Antigravity
# Names who asked when it is not one of the tools Notchling knows, on the consent card in the Inbox.
consent-card-tool-other = An agent

# Who asked, when Notchling checked that the session is the tool's own. $tool is the tool; $session is a short session id.
consent-card-who-verified = { $tool } · verified session { $session }
# Who asked, when Notchling could not check the session. $tool is the tool; $session is a short session id.
consent-card-who-unverified = { $tool } · session { $session }, not verified
# Who asked, when it is a program outside any agent session. $app is the app it runs under, such as Terminal.
consent-card-who-outside-app = A program outside any agent session · in { $app }
# Who asked, when it is a program outside any agent session and Notchling does not know the app it runs under.
consent-card-who-outside = A program outside any agent session

## What it asks

# What the request does: change one setting. $title is the setting's name.
consent-card-fact-setting = Change { $title }
# What the request does: add a secret to the Vault. $name is the secret's name.
consent-card-fact-secret-add = Add the secret { $name }
# What the request does: change a secret that is already in the Vault. $name is the secret's name.
consent-card-fact-secret-update = Update the secret { $name }
# What a setting is now. $value is the current value.
consent-card-fact-before = Now: { $value }
# What a setting would become. $value is the new value.
consent-card-fact-after = Would become: { $value }
# A value that is not set.
consent-card-unset = not set

## Details

# Label of the row that says who asked, on the consent card.
consent-card-label-who = Asked by
# Label of the row that says which machine the request is on, on the consent card.
consent-card-label-node = Machine
# Label of the row that says which folder the request is for, on the consent card.
consent-card-label-folder = Folder
# Label of the row that says which organization the request belongs to, on the consent card.
consent-card-label-organization = Organization
# Label of the row that says what the yes covers, on the consent card.
consent-card-label-scope = Covers
# What the yes covers: one use of this request, then it is used up.
consent-card-scope-once = One time only; used up when it is applied

## The agent's words

# Heading of the box that holds words the agent wrote itself. Notchling has not checked them.
consent-card-untrusted-label = The agent says
# Line under the heading of the box that holds the agent's own words.
consent-card-untrusted-note = Written by the agent. Notchling has not checked it.
# Heading of the box that holds the new value of a text setting, as the agent wrote it.
consent-card-new-value-label = The new value, as the agent wrote it

## Time and state

# How long is left to answer. $time is a duration such as 14m 03s.
consent-card-expires-in = Ends in { $time }
# The request ran out of time, so it can no longer be allowed.
consent-card-ended = Ended. It can no longer be allowed.
# Shown on a secret request while the Vault is off. Allow stays off until the Vault is on.
consent-card-vault-off = The Vault is off. Turn it on in Settings first, then allow this.

## Switch requests

# What the request does: turn on Act on Sessions, the switch that lets Notchling act on agent sessions.
consent-card-switch-acting = Turn on Act on Sessions
# What turning on Act on Sessions unlocks, on the consent card.
consent-card-switch-acting-unlocks = Notchling may answer, reply to and stop your sessions for agents.
# What the request does: turn on the Phone Bridge.
consent-card-switch-bridge = Turn on the Phone Bridge
# What turning on the Phone Bridge unlocks, on the consent card.
consent-card-switch-bridge-unlocks = Paired phones may see and answer your sessions.
# What the request does: turn on Accounts, the switch that lets Notchling sign in and switch accounts.
consent-card-switch-accounts = Turn on Accounts
# What turning on Accounts unlocks, on the consent card.
consent-card-switch-accounts-unlocks = Notchling may sign in and switch your accounts.
# What the request does: turn on the Vault.
consent-card-switch-vault = Turn on the Vault
# What turning on the Vault unlocks, on the consent card. It is a warning.
consent-card-switch-vault-unlocks = This opens every saved secret to the agents that may use them.
# What the request does: turn on Install, the switch that lets Notchling install hooks and skills.
consent-card-switch-install = Turn on Install
# What turning on Install unlocks, on the consent card.
consent-card-switch-install-unlocks = Notchling may install and remove its hooks and skills.
# What the request does: turn on the cluster.
consent-card-switch-cluster = Turn on the cluster
# What turning on the cluster unlocks, on the consent card.
consent-card-switch-cluster-unlocks = This Mac may link to your other machines.
# Said on every switch request: the yes lasts until Notchling restarts.
consent-card-switch-until-restart = It turns off again when Notchling restarts.

## The form that saves an allowed secret

# Said on the Vault form when the request ran out of time before the secret was saved.
consent-card-form-expired = This request ended before you saved. Nothing was saved.
# Said on the Vault form when the secret changed after the card was shown, so the yes no longer fits.
consent-card-form-changed = This secret changed after you were asked. Nothing was saved. Deny the request and ask again.
# Said on the Vault form when the Vault is off, so nothing can be saved.
consent-card-form-vault-off = The Vault is off. Turn it on in Settings, then save again.
# Said on the Vault form when the request was withdrawn or already answered, so nothing can be saved.
consent-card-form-withdrawn = This request was withdrawn or already answered. Nothing was saved.
