### Every word that grants or refuses something: the buttons and titles of each prompt that allows, denies, approves, pairs, signs in, trusts a folder, installs the hook or turns a switch back on.
### The maintainer reviews every change to this file, in every language.

# Shown in the Chat screen.
consent-allow = Allow

# Shown in the Chat screen.
consent-deny = Deny

# Item in a Blocking row's submenu in the menu bar menu: send the agent a note instead of allowing or denying.
consent-tray-change = Change…

# Shown in the Inbox: the third answer of a request. It opens a field where the user writes what the agent should change.
consent-change = Change…

# Placeholder of the field that opens with Change…. The user tells the agent what to do differently.
consent-change-placeholder = Say what to change

# Button that sends the note and denies the request, so the agent can ask again with the change.
consent-change-send = Send

# Shown on a card the agent asked again after a note. $note is what the user wrote.
consent-asked-again = Asked again after your note: { $note }

# Shown in pairing with a Mac.
consent-pair = Pair

# Shown in the Chat screen.
consent-stop-and-take-over = Stop and take over

# Shown in the Chat screen.
consent-stop-anyway = Stop anyway

## The notch's permission card

# Title of the card in the notch that asks to allow a tool.
consent-notch-permission-needed = Permission needed

# Line of the card in the notch that says what is asked. $tool is the tool Claude wants to use.
consent-notch-wants-to-use = { -brand-claude } wants to use { $tool }

# Option of the notch's permission card: allow this one time. The line under it is the next message.
consent-notch-allow = Allow

# Line under the Allow option of the notch's permission card.
consent-notch-allow-detail = This time only

# Option of the notch's permission card: allow and stop asking for the same thing. The line under it is the next message.
consent-notch-always = Always allow this

# Line under the Always allow option of the notch's permission card.
consent-notch-always-detail = Don't ask again for this

# Option of the notch's permission card: refuse. The line under it is the next message.
consent-notch-deny = Deny

# Line under the Deny option of the notch's permission card.
consent-notch-deny-detail = { -brand-claude } tries something else

# Button of a blocking card in the notch for a secret: allow the use this one time.
consent-notch-allow-once = Allow Once

# Line of the notch for two seconds after the user allowed a blocked task.
consent-notch-allowed = Allowed

# Line of the notch for two seconds after the user denied a blocked task.
consent-notch-denied = Denied

# Line of the notch for two seconds after the user sent the agent a note instead of allowing or denying.
consent-notch-changed = Changed

# Button of a blocking card in the notch: send the agent a note instead of allowing or denying.
consent-notch-change = Change…

# Button of a blocking card in the notch: leave the question for later and close the notch.
consent-notch-later = Later

# Button of a blocking card in the notch: open the full card in the Inbox.
consent-notch-open = Open

# Button of a blocking card in the notch that asks for a secret: open the form that takes it.
consent-notch-open-form = Open Form

## The cards that ask the user's yes for an agent's request

# What a card asks when an agent wants a setting changed. $title is the setting's name as Settings shows it.
consent-setting-change = Change { $title }

# What a card asks when an agent wants a secret saved in the Vault, new or replacing one of the same name. $name is the secret's name.
consent-secret-request = Save the secret { $name } in the Vault

## The buttons on a banner (a system notification) for a request that waits on the user

# Button on a banner: allow what the agent asks.
consent-banner-allow = Allow

# Button on a banner: refuse what the agent asks.
consent-banner-deny = Deny

# Button on a banner: answer the agent with a note instead of allowing or denying.
consent-banner-change = Change…

# Button on a banner: open the request in Notchling.
consent-banner-open = Open

# Button on a banner: reply to the agent's question.
consent-banner-reply = Reply…

# Button on a banner: dismiss the agent's question without answering.
consent-banner-dismiss = Dismiss

# Button on a banner: open the form that asks for a secret.
consent-banner-open-form = Open Form

# Button on a banner: cancel the request for a secret.
consent-banner-cancel = Cancel

# Button on the notch's stopped card: stop holding the notch open and answer later.
consent-banner-later = Later

# Button next to Send that closes the field where you write a note for the agent, and sends nothing.
consent-change-cancel = Cancel

# Result word on a closed row: the request was allowed.
consent-decided-allowed = Allowed
# Result word on a closed row: the request was allowed one time.
consent-decided-allowed-once = Allowed Once
# Result word on a closed row: the request is always allowed.
consent-decided-allowed-always = Always Allowed
# Result word on a closed row: the request was refused.
consent-decided-denied = Denied
