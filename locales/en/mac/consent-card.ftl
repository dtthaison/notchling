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
# What the request does: turn on Machines.
consent-card-switch-machines = Turn on Machines
# What turning on Machines unlocks, on the consent card.
consent-card-switch-machines-unlocks = Notchling may connect to your machines over ssh and ask your network about them.
# Said on every switch request: the yes lasts until Notchling restarts.
consent-card-switch-until-restart = It turns off again when Notchling restarts.

## Session requests

# What the request does: start a session. $tool is the tool's name, $folder the folder it starts in.
consent-card-fact-session-start = Start a { $tool } session in { $folder }
# What the request does: send a message to a session. $name is the session's name, $folder the folder it works in.
consent-card-fact-session-say = Send a message to { $name } ({ $folder })
# What the request does: stop a session. $name is the session's name, $folder the folder it works in.
consent-card-fact-session-stop = Stop { $name } ({ $folder })
# What the request does: interrupt the turn a session is working on. $name is the session's name, $folder the folder it works in.
consent-card-fact-session-interrupt = Interrupt { $name } ({ $folder })
# What the request does: remove a stopped session from the list. $name is the session's name, $folder the folder it works in.
consent-card-fact-session-remove = Remove { $name } ({ $folder })
# What the request does: answer the question a session is asking. $name is the session's name, $folder the folder it works in.
consent-card-fact-session-choose = Answer the question of { $name } ({ $folder })
# What the request does: run a slash command in a session. $name is the session's name, $folder the folder it works in.
consent-card-fact-session-run = Run a command in { $name } ({ $folder })
# What the request does: pick an option in the numbered dialog a session shows. $name is the session's name, $folder the folder it works in.
consent-card-fact-session-pick = Pick an option in the dialog of { $name } ({ $folder })
# What the request does: close the dialog a session shows without picking an option. $name is the session's name, $folder the folder it works in.
consent-card-fact-session-dismiss = Dismiss the dialog of { $name } ({ $folder })
# What the request does: turn Chrome on for a session. $name is the session's name, $folder the folder it works in.
consent-card-fact-session-browse-on = Turn Chrome on for { $name } ({ $folder })
# What the request does: turn Chrome off for a session. $name is the session's name, $folder the folder it works in.
consent-card-fact-session-browse-off = Turn Chrome off for { $name } ({ $folder })
# What the request does: change an option of an Antigravity session. $name is the session's name, $folder the folder it works in.
consent-card-fact-session-tune = Change an option of { $name } ({ $folder })
# Said on a start request that picks a model. $model is the model name, such as opus.
consent-card-fact-session-model = Uses the model { $model }
# Said when the session would start in a folder Claude Code does not trust yet.
consent-card-fact-session-trusts = Trusts this folder
# Said on a start request: what the agent may do with the session it starts.
consent-card-fact-session-child = The agent may send it messages and stop it until it ends. It still asks you before any tool it is not allowed to use.
# Heading of the box that holds a message for a session, as the agent wrote it.
consent-card-message-label = The message, as the agent wrote it
# Heading of the box that holds the command an agent wants to run in a session, as the agent wrote it.
consent-card-command-label = The command, as the agent wrote it
# Heading of the box that holds the answers an agent chose for a session's question, each after its question.
consent-card-answers-label = The answers, as the agent chose them
# Heading of the box that holds the dialog a session shows and the option the request would press.
consent-card-dialog-label = The dialog, as the session shows it
# Heading of the box that holds the option an agent wants to change and its new value.
consent-card-option-label = The option, as the agent chose it

## The form that saves an allowed secret

# Said on the Vault form when the request ran out of time before the secret was saved.
consent-card-form-expired = This request ended before you saved. Nothing was saved.
# Said on the Vault form when the secret changed after the card was shown, so the yes no longer fits.
consent-card-form-changed = This secret changed after you were asked. Nothing was saved. Deny the request and ask again.
# Said on the Vault form when the Vault is off, so nothing can be saved.
consent-card-form-vault-off = The Vault is off. Turn it on in Settings, then save again.
# Said on the Vault form when the request was withdrawn or already answered, so nothing can be saved.
consent-card-form-withdrawn = This request was withdrawn or already answered. Nothing was saved.
# Said on the Vault form when the name typed already belongs to another secret, so nothing was saved and the form stays open to pick another name.
consent-card-form-name-taken = A secret with that name already exists. Choose another name.
# Tooltip on the small Copy button beside a secret's name, in the Vault list and on the Vault form. It copies the name only, never the value.
consent-card-form-copy-name = Copy Name
# Tooltip on the small Copy button beside a saved username in the Vault list. It copies the username.
consent-card-vault-copy-username = Copy Username
# Tooltip on the small Copy button beside a saved password in the Vault list. It copies the password and clears the Clipboard a minute later.
consent-card-vault-copy-password = Copy Password
# Tooltip on the small Copy button beside a saved token in the Vault list. It copies the token and clears the Clipboard a minute later.
consent-card-vault-copy-token = Copy Token
# Tooltip on the eye button that shows a hidden password or token in the Vault, in the list and on the form.
consent-card-vault-show = Show
# Tooltip on the eye button that hides a shown password or token in the Vault, in the list and on the form.
consent-card-vault-hide = Hide
# Small label before a saved username in the Vault list.
consent-card-vault-label-username = Username
# Small label before a saved password in the Vault list.
consent-card-vault-label-password = Password
# Small label before a saved token in the Vault list.
consent-card-vault-label-token = Token
# Said under a Vault entry when its value could not be read to show or copy.
consent-card-vault-unreadable = The value couldn't be read.
# Said under a Vault entry when its value was changed or replaced while it was being read to show or copy, so nothing was shown or copied.
consent-card-vault-changed = This secret just changed. Try again.
# Said under a Vault entry when a password or token could not be put on the Clipboard as a hidden copy, so nothing was copied.
consent-card-vault-copy-failed = The value couldn't be copied.
# Names a shell command job on a card: it is the tool of a job.
consent-card-job-tool-shell = Shell
# What the request does: send a job to another machine. $tool is the tool's name, $machine the machine, $folder the folder it works in.
consent-card-fact-job-dispatch = Send a { $tool } job to { $machine } in { $folder }
# Said when the machine is online. $machine is the machine, $organization its organization.
consent-card-fact-job-machine-online = { $machine } is online in { $organization }
# Said when the machine is offline. $machine is the machine, $organization its organization.
consent-card-fact-job-machine-offline = { $machine } is offline in { $organization }, so the job waits until it is back
# Said when the job uses a model. $model is the model, $effort the effort level.
consent-card-fact-job-model = Uses the model { $model }
# Said when the job gets secrets. $names is a comma separated list of the secret names.
consent-card-fact-job-secrets = Gets the secrets { $names } on that machine
# What the request does: read the output of a job. $job is the job's id, $machine the machine, $folder the folder.
consent-card-fact-job-tail = Read the output of job { $job } on { $machine } ({ $folder })
# What the request does: bring back the result of a job. $job is the job's id, $machine the machine, $folder the folder.
consent-card-fact-job-return = Bring back the result of job { $job } on { $machine } ({ $folder })
# What the request does: stop a job. $job is the job's id, $machine the machine, $folder the folder.
consent-card-fact-job-stop = Stop job { $job } on { $machine } ({ $folder })
# Says which cluster a request changes. $cluster is the cluster's name.
consent-card-fact-cluster = In the cluster { $cluster }
# Says which machine a cluster request is about. $machine is its name, $address where it is reached, $organization its organization.
consent-card-fact-cluster-machine = The machine { $machine } at { $address } in { $organization }
# Same, when the machine's address is not known. $machine is its name, $organization its organization.
consent-card-fact-cluster-machine-no-address = The machine { $machine } in { $organization }
# What the request does: create a cluster. $name is the new cluster's name, $organization its organization.
consent-card-fact-cluster-create = Create a cluster named { $name } in { $organization }
# What the request does: make a code that another machine uses to join the cluster.
consent-card-fact-cluster-token = Make a join code that works for 10 minutes. Only the agent that asked gets it, once
# What the request does: invite a machine. $name is the machine, $node its id, $role the role (Master or Worker), $acting whether it may act for agents (On or Off).
consent-card-fact-cluster-invite = Invite { $name } ({ $node }) as a { $role }. Acting for agents there: { $acting }
# What the request does: change a machine's role. $before and $after are the old and new role.
consent-card-fact-cluster-role = Role changes from { $before } to { $after }
# What the request does: change whether a machine takes delegated jobs. $before and $after are On or Off.
consent-card-fact-cluster-delegation = Taking delegated jobs changes from { $before } to { $after }
# What the request does: change the cluster's failover. $before and $after are the old and new setting.
consent-card-fact-cluster-failover = Failover changes from { $before } to { $after }
# What the request does: change whether a session is a main session. $session is the start of its id, $before and $after are On or Off.
consent-card-fact-cluster-main = Main session { $session } changes from { $before } to { $after }
# What the request does: let a machine use a secret. $name is the secret's name.
consent-card-fact-cluster-allow-secret = Lets it use the secret { $name }
# What the request does: let a machine use no secrets.
consent-card-fact-cluster-allow-none = Lets it use no secrets
# What the request does: remove a machine from the cluster. $machine is its name.
consent-card-fact-cluster-remove = Remove { $machine } from the cluster
# What the request does: make a machine the cluster's primary. $machine is its name.
consent-card-fact-cluster-move-home = Make { $machine } the primary of the cluster
# What the request does: finish moving the cluster's primary.
consent-card-fact-cluster-move-now = Finish moving the primary now
# Says which account a request changes. $tool is the tool, $account the account as a masked email, $organization its organization.
consent-card-fact-account = The { $tool } account { $account } in { $organization }
# What the request does: switch the account new sessions use. $tool is the tool, $account the new account (a masked email), $before the account in use now (a masked email).
consent-card-fact-account-switch = New { $tool } sessions use { $account } instead of { $before }
# What the request does: switch the account when none is in use. $tool is the tool, $account the new account (a masked email).
consent-card-fact-account-switch-first = New { $tool } sessions use { $account }
# What switching the Codex account does to what is running.
consent-card-fact-account-switch-codex = If the ChatGPT app or Codex's background server is running, it restarts and its running tasks stop. Open Codex sessions keep the old account until they start again
# What switching the Antigravity account does to what is running.
consent-card-fact-account-switch-antigravity = If Antigravity is running, it quits and opens again on the other account. Agents running in it stop
# What the request does: sign in to a new account. $tool is the tool, $organization the organization the account joins.
consent-card-fact-account-add = Sign in to a new { $tool } account in your browser. It joins { $organization }
# What the request does: remove an account. $tool is the tool, $account the account (a masked email).
consent-card-fact-account-remove = Remove the { $tool } account { $account } and its saved sign-in from Notchling
# What the request does: move sessions from lanes to the main login. $count is how many sessions move, $account the account the main login signs in as (a masked email).
consent-card-fact-account-move-all = Sessions moved from lanes to the main login, signed in as { $account }: { $count }
# What the request leaves behind: sessions outside Notchling in a lane that closes. $count is how many.
consent-card-fact-account-signed-out = Sessions in that lane outside Notchling that are signed out: { $count }
# What the request does: move one session to another login. $name is the session's name, $folder its folder, $account the account it moves to (a masked email).
consent-card-fact-account-move-session = Move { $name } ({ $folder }) to the login of { $account }. It stops and resumes there
# What the request does: open a lane. $account is the lane's account (a masked email).
consent-card-fact-lane-create = Open a lane for { $account }, so sessions can run on it beside the main login
# What the request does: close a lane. $account is the lane's account (a masked email).
consent-card-fact-lane-close = Close the lane of { $account }
# What the request does: change the account of a lane. $before is the old account, $account the new one (masked emails).
consent-card-fact-lane-switch = The lane of { $before } signs in as { $account } instead
# What the request touches: sessions running in a lane. $count is how many.
consent-card-fact-lane-sessions = Sessions running in that lane now: { $count }
# A role in a cluster.
consent-card-word-master = Master
# A role in a cluster.
consent-card-word-worker = Worker
# A setting that is on.
consent-card-word-on = On
# A setting that is off.
consent-card-word-off = Off
