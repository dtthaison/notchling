### The cluster: the Cluster button at the foot of the Sidebar, the cluster sheet with one row per node, and the node names on remote sessions in the Sidebar.

## Button and sheet

# Tooltip of the cluster button at the foot of the Sidebar.
cluster-tip = Machines in the cluster
# Title of the cluster sheet and name of the footer button.
cluster-title = Cluster
cluster-done = Done
# Shown on the sheet when the app has no connection to the daemon.
cluster-detached = The daemon isn't running, so the cluster can't be read.
# Shown on the sheet before the daemon has answered.
cluster-loading = Reading the cluster…

## Roles

# Role of the node that runs the cluster: the primary Mac.
cluster-role-primary = Primary
# Role of a node that can take over from the primary.
cluster-role-standby = Standby
# Role of a node that only runs the work it is sent.
cluster-role-worker = Worker
cluster-role-unknown = Member

## Rows

# Names of operating systems as shown on a node.
cluster-os-macos = macOS
cluster-os-linux = Linux
cluster-os-windows = Windows
# An operating system with its processor architecture; $os is its name, $arch for example aarch64.
cluster-os-arch = { $os } { $arch }

# Beside the name of the node this app is on.
cluster-this-mac = This Mac
cluster-online = Online
cluster-offline = Offline
cluster-pending = Waiting for approval
cluster-removed = Removed
# A tool on a node: $tool is its name, $version what is installed.
cluster-tool = { $tool } { $version }
# A tool a node doesn't have; $tool is its name.
cluster-tool-missing = { $tool } not installed
# Load of a node: $cpu is a percent, $used and $total are gigabytes of memory.
cluster-load = CPU { $cpu }% · Memory { $used } of { $total } GB
# The app version a node runs; $version is the number.
cluster-app-version = Notchling { $version }
# Tooltip of the button that opens a node's menu.
cluster-more = More

## Row menu

cluster-can-be-master = Can Be Master
cluster-acting-on = Turn Acting On
cluster-acting-off = Turn Acting Off
cluster-remove = Remove…
cluster-remove-title = Remove This Node?
# Confirmation text; $name is the node's name.
cluster-remove-note = { $name } leaves the cluster and its sessions leave every list. It can join again with a new token.
cluster-remove-confirm = Remove

## Readiness and sets

# Under a Can Be Master machine that can't take sets yet; $reason names the first check that fails.
cluster-not-ready = Not ready: { $reason }
# Reason in that line when the daemon says a machine isn't ready and names no check.
cluster-not-ready-unknown = Notchling couldn't read why
# Under a Can Be Master machine; $age is how long ago the last set arrived, such as 2 minutes ago.
cluster-set-last = Last set { $age }
# The same with the set's size in megabytes; $mb is a whole number.
cluster-set-last-sized = Last set { $age } · { NUMBER($mb) } MB
# Under a Can Be Master machine that holds no set yet.
cluster-set-none = No set yet
# Under a machine that missed sets while a session worked; $age is how long, such as 21m.
cluster-set-behind = Behind: no new set for { $age }
# The same when no set ever arrived.
cluster-set-behind-none = Behind: no set has arrived yet

## Move to a machine

# Row menu item that hands the primary over to a Can Be Master machine; $name is its name.
cluster-move-to = Move to { $name } Now…
# Title of the Move sheet; $name is the machine that becomes the primary.
cluster-move-title = Move the Primary to { $name }
# Text of the Move sheet before anything starts; $name is the machine that becomes the primary.
cluster-move-intro = { $name } becomes the primary. Sessions whose code is pushed move with it. The others keep running where they are, and { $name } lists them as remote sessions.
# Text of the Move sheet while the request is out; $name is the machine.
cluster-move-starting = Asking the primary to move to { $name }…
cluster-move-start = Move the Primary
cluster-move-cancel = Cancel
# Button that stops waiting for sessions to finish their turns and stops the working ones now.
cluster-move-now = Move Now
cluster-move-close = Close
# Shown in the Move sheet when Move Now fails; $error is the reason.
cluster-move-now-failed = Couldn't move now: { $error }
# Shown when the Move sheet's Move is pressed but the machine can no longer take the primary.
cluster-move-unavailable = That machine can't take the primary right now.
# Line on the cluster sheet while a move runs; $name is the machine that becomes the primary.
cluster-move-running = Moving the primary to { $name }…
# Button beside that line that opens the Move sheet.
cluster-move-show = Show
# Heading above the sessions that move and the ones that stay.
cluster-move-moving = Moving
cluster-move-staying = Staying
# Steps of a move, in order; $name is the machine that becomes the primary.
cluster-move-step-prepare = Checking { $name }
cluster-move-step-catch-up = Sending { $name } the newest set
cluster-move-step-freeze = Waiting for the sessions to finish their turns
cluster-move-step-final = Taking the final set
cluster-move-step-stop = Stopping the moving sessions here
cluster-move-step-switch = Making { $name } the primary
cluster-move-step-resume = Starting the sessions on { $name }
cluster-move-step-announce = Telling your machines and phones
cluster-move-step-done = { $name } is the primary now
# Shown when a move was undone before the switch; $reason is why.
cluster-move-undone = Couldn't move the primary: { $reason }
# Shown after Move Now; $names is a list of the sessions that were stopped mid-turn.
cluster-move-cut = Stopped mid-turn, so their last step isn't finished: { $names }
# A session that didn't start on the new primary; $name is the session, $why the reason.
cluster-move-failed = { $name } didn't start on the new primary: { $why }

# Sentence in the Move sheet when the move's last step ended without the old primary pausing the moved sessions; $primary is the old primary's name, $names the sessions, $why the reason it gave.
cluster-move-unfenced = { $primary } couldn't pause { $names }. They may run on both machines until it stops them. { $why }
# Stands for the old primary's name in that sentence when the app doesn't know it.
cluster-move-old-primary = The old primary

## Move when the primary can't be reached

# Text of the sheet that moves the primary while it is silent; $name is the machine that becomes the primary, $primary the one that went quiet.
cluster-promote-intro = { $primary } can't be reached. { $name } becomes the primary from its last set. Work { $primary } did after that stays on { $primary } as a side copy.
# Text while the request is out; $name is the machine.
cluster-promote-asking = Asking { $name } to take the primary…
# Button that goes ahead after the daemon named a risk.
cluster-move-anyway = Move Anyway
# Said when the machine asked to take the primary found the old one reachable.
cluster-promote-nothing-moved = Nothing moved.
# Steps of taking the primary, in order; $name is the machine that becomes the primary, $primary the old one.
cluster-promote-step-check = Checking { $name }
cluster-promote-step-reach = Looking for { $primary }
cluster-promote-step-bump = Making { $name } the primary
cluster-promote-step-restore = Restoring the sessions from the last set
cluster-promote-step-resume = Starting the sessions on { $name }
cluster-promote-step-held = Sending the messages that were held
cluster-promote-step-items = Listing what needs a look
# Shown when the machine is the primary but its progress isn't reported to this app.
cluster-promote-step-unknown = Moving the primary…

## Take Back

cluster-take-back = Take Back
cluster-take-back-title = Take Back the Primary?
cluster-take-back-note = Sessions come home after their turns end.
# $error is the reason.
cluster-take-back-failed = Couldn't take the primary back: { $error }

## Side copies

cluster-copies-title = Side Copies
cluster-copies-note = Work a machine did after the set its successor resumed from. Notchling never deletes or merges one.
# Under a side copy; $machine is where it is, $age such as 5 minutes ago.
cluster-copy-line = On { $machine } · { $age }
cluster-copy-open = Open
cluster-copy-hide = Hide
cluster-copy-gone = That copy isn't on this Mac any more.
# $error is the reason.
cluster-copy-hide-failed = Couldn't hide that copy: { $error }

## Waiting sessions

cluster-waiting-title = Waiting Sessions
# $title is the session, $machine the one with newer work in it.
cluster-waits = { $title } waits for { $machine }, which has newer work in it.
cluster-wait = Wait
cluster-continue-copy = Continue From This Copy
cluster-continue-title = Continue From This Copy?
# Question before a waiting session continues here; $title is the session, $machine the one with the newer copy.
cluster-continue-note = Continue { $title } here? The newer copy on { $machine } is kept beside it.
# $error is the reason.
cluster-continue-failed = Couldn't continue it here: { $error }

## Keep on this Mac

# Check in a session's menu; $machine is this Mac's name. On means the session isn't carried over by a Standby.
cluster-keep-on = Keep on { $machine }
# Under that check in the menu.
cluster-keep-note = Other sessions continue on your Standby if this Mac goes away.
# Shown when turning that check on or off failed; $error is the reason.
cluster-keep-failed = Couldn't change that: { $error }

## Install Hook

# Row menu item: puts Notchling's hook and skills into Claude Code's settings on a machine with no app.
cluster-install-hook = Install Hook…
# Row menu items that turn the node's Install switch on or off.
cluster-install-switch-on = Turn Install On
cluster-install-switch-off = Turn Install Off
# Title of the plan shown before installing; $name is the node's name.
cluster-install-title = Install the Hook on { $name }?
# First line of that plan; $name is the node's name.
cluster-install-intro = This changes Claude Code's settings on { $name }, not on this Mac:
# Line of that plan naming the skills it writes; $names is a list of skill names.
cluster-install-skills = Skills: { $names }
cluster-install-confirm = Install
# $name is the node's name.
cluster-install-is-off = Install is off for { $name }. Turn Install On in its menu, then try again.
# $name is the node's name.
cluster-install-already = The hook is already installed on { $name }.
# $name is the node's name, $error why its settings can't be edited.
cluster-install-refused = The settings on { $name } can't be edited: { $error }
# $name is the node's name.
cluster-install-done = Installed the hook on { $name }.
# $name is the node's name.
cluster-install-nothing = Nothing was written on { $name }.
# $name is the node's name, $error the reason.
cluster-install-failed = Couldn't install the hook on { $name }: { $error }

# Reason in the install error when the daemon's answer isn't a plan this app reads.
cluster-install-unread = the plan isn't one this app reads

## Connection

cluster-link-off = Not Connected
cluster-link-no-tailscale = No Tailscale Address
cluster-link-not-listening = Not Listening
cluster-link-on = Connected
cluster-turn-on = Turn On
cluster-turn-on-failed = Couldn't turn the cluster on: { $error }
# Under the connection line while the cluster switch is off; Turn On is the button beside it.
cluster-off-hint = The cluster is off on this Mac. Turn it on to connect to your other machines.

## Create and join

cluster-none = No cluster yet. Create one to share work between your machines.
cluster-name-label = Cluster Name
cluster-create = Create Cluster
cluster-create-failed = Couldn't create the cluster: { $error }
# Above the join token field on the sheet that has no cluster yet.
cluster-join-intro = Or join a cluster that already exists. Copy a join token on its Primary and paste it here.
cluster-join-label = Join Token
cluster-join-placeholder = Paste a join token
cluster-join = Join
cluster-join-failed = Couldn't join the cluster: { $error }
cluster-join-bad-token = That isn't a join token. A join token starts with nlj1.
# Shown while this Mac's join has not been answered yet.
cluster-joining-asking = Joining the cluster. Waiting for the Primary to answer…
# Shown while the Primary has the join request and has not approved it.
cluster-joining-waiting = Waiting for approval on the Primary. Approve this Mac there to finish joining.
# Title of the sheet's body while this Mac is joining.
cluster-joining = Joining…
cluster-copy-token = Copy Join Token
cluster-token-copied = Join token copied. It works once, for 10 minutes.
cluster-token-failed = Couldn't make a join token: { $error }
# Reason in the join token error when the daemon answers without a token.
cluster-token-none = the daemon sent no token
cluster-change-failed = Couldn't change the node: { $error }

## Sidebar

# Shown in the reply box of a session on a node that is offline; $name is the node's name.
cluster-node-offline = { $name } is offline
# Line under a session in the Sidebar when its node is offline; $name is the node's name.
cluster-node-line-offline = { $name } · Offline

## Run on

# Title of the column in New Session that picks the machine a session runs on.
cluster-run-on = Run On
# Title of the column that picks the tool for a session on another machine.
cluster-run-on-tool = Tool
# A shell job: a command run on another machine.
cluster-remote-shell = Shell Job
# Under a tool on another machine; $node is the machine's name.
cluster-uses-login = Uses { $node }'s own login
# Under Shell Job in the tool column; $node is the machine's name.
cluster-shell-detail = Runs a command on { $node }
# Under This Mac in the Run On column.
cluster-this-mac-detail = Where Notchling runs
# Reason a machine can't be picked when Notchling has not read its versions yet; $name is its name.
cluster-run-on-unread = Notchling hasn't read { $name }'s versions yet
cluster-code-title = Code
cluster-code-send = Send This Repository
# Under Send This Repository; $node is the machine's name.
cluster-code-send-detail = Copies this repository to { $node }
cluster-code-folder = Use a Folder There
# Under Use a Folder There; $node is the machine's name.
cluster-code-folder-detail = A folder that is already on { $node }
# Label of the field for a folder on another machine; $node is its name.
cluster-folder-there = Folder on { $node }
cluster-folder-placeholder = /home/me/project
cluster-command-label = Command
cluster-command-placeholder = make test
cluster-first-message-label = First Message
cluster-first-message-placeholder = What should it do?
# Footer of New Session for a machine: $tool, $node and $folder are names.
cluster-run-on-chosen = { $tool } on { $node } in { $folder }
# Footer of New Session when a machine is picked and no folder yet; $node is its name.
cluster-run-on-needs-folder = Choose a folder for { $node }.
# Footer after a job started on a machine with a warning; $node is its name, $warning the text.
cluster-started-with-warning = Started on { $node }. { $warning }
cluster-sent-done = Done

## Sessions on another machine

# Why a button does nothing for a session on another machine.
cluster-remote-not-here = Not available for a session on another machine yet.
cluster-bring-back = Bring Back Branch
cluster-bring-back-tip = Copies this job's work to a branch in the repository on this Mac
# Shown after Bring Back Branch worked; $branch is the branch made.
cluster-brought-back = Brought back as { $branch }.
cluster-bring-back-failed = Couldn't bring the branch back: { $error }
cluster-bring-back-busy = Bringing the branch back…
cluster-bring-back-no-branch = Couldn't bring the branch back: the machine named no branch.
cluster-mirror-waiting = Waiting for the conversation from the other machine…

## Shell jobs

cluster-shell-stop = Stop
cluster-shell-stopping = Stopping…
cluster-shell-running = Running
cluster-shell-ended = Finished
cluster-shell-waiting = Waiting for the log from the other machine…
cluster-shell-empty = No output yet.
cluster-shell-no-reply = A shell job takes no replies.

## Logins

# One tool's login on a node, in a row of them; $tool is the tool's name and $state one of the four words below.
cluster-login = { $tool }: { $state }
cluster-login-signed-in = Signed In
cluster-login-signed-out = Signed Out
cluster-login-not-installed = Not Installed
# The node has not said whether the tool is signed in.
cluster-login-unknown = Unknown
# After a node's tool logins, for a machine that runs the cluster's jobs. $state is one of the two words below.
cluster-worker-login = Worker Login: { $state }
# The cluster has sent this machine its own Claude Code login for jobs.
cluster-worker-login-on = On
# No cluster login is on this machine; jobs use the machine's own Claude Code login.
cluster-worker-login-off = Off

## Secrets

# A row menu item that opens the list of vault secrets a node may use.
cluster-secrets = Secrets…
# Title of the secrets editor; $name is the node's name.
cluster-secrets-title = Secrets on { $name }
cluster-secrets-about = A job on this machine can carry only the secrets checked here. Each is sent to the machine when the job starts and deleted when it ends.
cluster-secrets-any = Any Secret
cluster-secrets-any-about = Every secret in the vault, by name, on this machine. Your checked names are kept for when this is off.
cluster-secrets-reading = Reading the vault…
cluster-secrets-empty = The vault has no secrets yet.
# Beside a checked name that is no longer saved in the vault.
cluster-secrets-gone = Not in the vault
cluster-secrets-save = Save
cluster-secrets-cancel = Cancel
cluster-secrets-failed = Couldn't change the secrets: { $error }
cluster-secrets-vault-failed = Couldn't read the vault: { $error }
# Shown when Save finds the machine can no longer take secrets (removed, inactive or a client).
cluster-secrets-node-gone = This machine can't take secrets any more, so nothing was saved.
# Under the list when the most secrets are checked; $max is the limit.
cluster-secrets-cap = At most { $max } secrets. Uncheck one to choose another.
cluster-secrets-none = No secrets
cluster-secrets-any-short = Any secret
# Line on a node's row; $count is how many secrets it may carry.
cluster-secrets-count = { $count ->
    [one] { NUMBER($count) } secret
   *[other] { NUMBER($count) } secrets
}
# Label of the picker of secrets in New Session on a Worker.
cluster-secrets-picker = Secrets
cluster-secrets-picker-none = This machine carries no secrets. Choose some in the cluster's Secrets… menu.
# Under the picker while another machine is the primary; $name is the vault's machine.
cluster-secrets-wait = New secrets wait for { $name }

## Vault on another machine

cluster-vault-tab-local = This Mac
# Tab that shows the vault of the machine that holds it; $name is its name.
cluster-vault-tab-remote = Vault on { $name }
cluster-vault-offline = The vault is on { $name }, which is offline
cluster-vault-unknown = This Mac doesn't know which machine holds the vault.
cluster-vault-unreachable = Can't reach the vault. Nothing was saved or removed.
# Shown when the machine a form or a removal was opened for is no longer the one this Mac would send it to.
cluster-vault-moved = The vault changed while this was open, so nothing was saved or removed. Open it again.
# Tab label while this Mac can't tell which machine holds the vault.
cluster-vault-tab-unknown = Vault on Another Machine

## Add Machine

# How-to after ssh refused the key; $target is user@host.
cluster-add-howto-key = Add this Mac's key with ssh-copy-id { $target } in Terminal, then Check Again.
# How-to when the machine's SSH key isn't known yet; $target is user@host.
cluster-add-howto-host-key = Run ssh { $target } once in Terminal and compare the key it shows, then Check Again.
# How-to when the machine didn't answer at all.
cluster-add-howto-reach = Check the address and that the machine is on and on your network, then Check Again.
# Reason when the check got no answer from the machine at all.
cluster-add-no-answer = The machine gave no answer.
# Reason when macOS there is too old; $version is what it runs.
cluster-add-unsupported-mac = Notchling needs macOS 13 or newer; this machine runs { $version }.
# Reason when the chip is not one Notchling is built for; $arch is what the machine reports.
cluster-add-unsupported-arch = Notchling runs on x86_64 and arm64 machines only; this one reports { $arch }.
# Reason when the machine is already in another cluster; $detail is the machine's own line.
cluster-add-other-cluster = The machine says: { $detail }. Leave that cluster first.
# Reason when nothing is left to set up and the machine isn't in the cluster.
cluster-add-nothing = Nothing is left to set up, but { $name } isn't in the cluster yet.
# Reason when machines.json can't be changed.
cluster-add-save-failed = Couldn't save machines.json.
# Reason when machines.json can't be read.
cluster-add-registry-unreadable = machines.json can't be read, so nothing can be added.
# Reason when another machine already has the name; $name is the name.
cluster-add-name-taken = A machine named { $name } is already in Machines with another address.
# Shown by Install and Join when the plan can't run; $name is the machine.
cluster-add-blocked-tailscale = Tailscale has to be connected on { $name } before Install and Join.
# Shown by Install and Join when the node account is not allowed; $name is the machine.
cluster-add-blocked-worker-user = The account on { $name } must not be an admin. Fix it first, then Check Again.
# Shown when Install and Join is pressed without the admin password.
cluster-add-needs-password = Type the admin password or use a Vault entry first.
# Shown when the machine changed after the plan was shown.
cluster-add-changed = The plan changed after it was shown. Check Again.
# Shown when a button is pressed that the flow can't take now.
cluster-add-not-now = That can't be done at this step.
# Shown when a plan expired or ran already.
cluster-add-plan-gone = The plan is no longer valid. Check Again.
# Line under Can Be Master when it is off by default on Linux.
cluster-add-master-linux = Needs an encrypted disk; most VPSes can't
cluster-add-master-filevault-off = FileVault is off
cluster-add-master-filevault-unknown = Can't tell whether FileVault is on
cluster-add-master-primary-unknown = Can't tell the primary's account
cluster-add-master-other-account = Needs the same user and home as the primary
# Reason a step stopped the run; $step is the step's title, $why what went wrong.
cluster-add-run-failed = { $step }: { $why }
# Reason the run stopped at a step that still isn't done; $step is its title, $why the check's line.
cluster-add-step-not-done = { $step } still isn't done: { $why }
# Reason when the machine never showed up in the cluster after the join; $name is its name.
cluster-add-join-timeout = { $name } didn't show up in the cluster in time. Look at the primary's Inbox, then Check Again.
# Line on the Cluster sheet while a run goes on; $name is the machine, $step the step number, $of how many steps there are.
cluster-add-running = Adding { $name }… (step { NUMBER($step) } of { NUMBER($of) })
# Line on the Cluster sheet while a run waits for the machine to appear.
cluster-add-joining = Waiting for { $name } to join…
# Line on the Cluster sheet when a run is done; $name is the machine.
cluster-add-done = { $name } is in the cluster.
# Why a step is in the plan, one line each.
cluster-add-why-worker-user = Notchling shouldn't run as root.
cluster-add-why-stays-awake = A machine that sleeps drops out of the cluster.
cluster-add-why-git = Jobs and sessions need Git.
cluster-add-why-tailscale = Machines reach each other over Tailscale.
cluster-add-why-binary = The Notchling program has to be on the machine.
cluster-add-why-service = The service keeps Notchling running after a restart.
cluster-add-why-app-closed = The service can't join while the app is open.
cluster-add-why-reach = The machine has to reach the primary over the tailnet.
cluster-add-why-joined = It joins this cluster without asking again, because you started it here.
cluster-add-why-claude = Sessions and jobs run Claude Code.
cluster-add-why-other = Needed to join the cluster.
# Reason when Join Cluster is pressed for a machine Machines doesn't list; $name is the name.
cluster-add-no-machine = Machines has no machine named { $name }.

## Add Machine sheet

# Button in the cluster sheet's header that opens the Add Machine sheet.
cluster-add-machine = Add Machine…
cluster-add-title = Add Machine
# Title of the sheet once a machine is chosen; $name is its name.
cluster-add-title-named = Add { $name }
cluster-add-intro = Type the machine's SSH address. Check only reads from the machine; nothing is installed until you press Install and Join.
cluster-add-address-label = SSH Address
cluster-add-address-placeholder = user@host
cluster-add-name-label = Name
# Line under the address field; $target is user@host as it will be used.
cluster-add-connects-as = Connects as { $target }
cluster-add-check = Check
cluster-add-check-again = Check Again
cluster-add-cancel = Cancel
cluster-add-close = Close
cluster-add-finish = Done
cluster-add-install = Install and Join
# Button while Tailscale waits for you to sign in; it opens Tailscale's sign-in page in the browser.
cluster-add-open-sign-in = Open Sign-In Page
# Spinner line while the machine is read; $target is user@host.
cluster-add-checking = Checking { $target }…
# Line over the plan; $name is the machine.
cluster-add-plan-intro = These steps run on { $name }, one at a time. Nothing has run yet.
cluster-add-show-commands = Show Commands
cluster-add-hide-commands = Hide Commands
# Tag on a plan step that needs the admin password.
cluster-add-admin-tag = Admin
# Tag on a plan step that you do yourself.
cluster-add-howto-tag = You do this
# Label over the admin password field.
cluster-add-password-label = Admin Password
# Placeholder of the admin password field; $target is user@host.
cluster-add-password-placeholder = Admin Password for { $target }
# Warning when admin steps run as root over ssh; $host is the machine's address.
cluster-add-root-warning = Admin steps run as root over SSH on { $host }.
cluster-add-master-label = Can Be Master
cluster-add-acting-label = Let it run sessions and jobs
cluster-add-choices-label = After it joins
# Line over steps you do yourself once the machine has joined; the join doesn't wait for them.
cluster-add-after-note = These don't stop the join. Do them yourself once it is in.
cluster-add-running-note = You can close this. The run goes on, and the Cluster sheet shows it.
cluster-add-log-label = What it did
# Row on the cluster sheet while a plan waits for Install and Join; $name is the machine.
cluster-add-row-waiting = { $name } is ready to add.
# Row on the cluster sheet while the machine is read; $name is the machine.
cluster-add-row-checking = Checking { $name }…
# Row on the cluster sheet when a run stopped; $name is the machine, $why the reason.
cluster-add-row-stopped = Adding { $name } stopped: { $why }
cluster-add-row-show = Show
cluster-add-row-dismiss = Dismiss
# Shown on the sheet when this build can't add machines, such as a render over fixtures.
cluster-add-unavailable = Adding machines isn't available here.

## Not in the Cluster

# Heading of the section that lists machines from Machines that aren't members.
cluster-not-joined-title = Not in the Cluster
cluster-not-joined-join = Join Cluster…
# Under a machine in that section; $target is user@host, $os the system such as macOS or Linux.
cluster-not-joined-line = { $target } · { $os }
# Under a machine in that section when its system isn't known yet; $target is user@host.
cluster-not-joined-line-unknown = { $target }

## Update Machines

# Line in the Cluster sheet when members run another Notchling build than this Mac; $count is how many, $names their names.
cluster-update-banner = { $count ->
    [one] { $count } machine runs another Notchling build: { $names }
   *[other] { $count } machines run another Notchling build: { $names }
}
# Button on that line; $count is how many machines.
cluster-update-button-open = { $count ->
    [one] Update { $count } Machine…
   *[other] Update { $count } Machines…
}
# Button that runs the update; $count is how many machines will be updated.
cluster-update-button-run = { $count ->
    [one] Update { $count } Machine
   *[other] Update { $count } Machines
}
cluster-update-not-now = Not Now
cluster-update-anyway = Update Anyway
cluster-update-title = Update Machines
cluster-update-intro = Each machine gets this Mac's Notchling, one at a time. A running Claude Code session keeps running through the restart.
cluster-update-ready = Ready to update.
# Under a machine that has a reason to wait but was chosen anyway; $why is that reason.
cluster-update-anyway-chosen = { $why }. It will update anyway.
# Under a member that isn't in Machines; $name is its name.
cluster-update-not-in-machines = { $name } isn't in Machines, so it can't be updated from here.
cluster-update-nothing = No machine can be updated now.
cluster-update-running = Updating…
cluster-update-continue = Continue
# Asks for the admin password of a step; $step is the step's title.
cluster-update-password-for = Admin password for { $step }
cluster-update-password-placeholder = Admin password
# Second sentence of the update line in the Machines window.
cluster-update-from-cluster = Update them from the Cluster sheet in Notchling.

## Update Tools

# Button on a version warning; $tool is Claude Code, Codex or Antigravity, $machine the machine, $version the version this Mac checked.
cluster-tool-update-button = Update { $tool } on { $machine } to { $version }
# The same button when this Mac's version is older than the machine's.
cluster-tool-update-down = Move { $tool } on { $machine } down to { $version }
# Row menu item that updates every tool that differs from this Mac's.
cluster-update-tools = Update Tools
# Under a warning while the machine runs a job; $machine is its name, $count the jobs.
cluster-tool-busy = { $count ->
    [one] { $machine } is running a job. Update its tools when it is done.
   *[other] { $machine } is running { $count } jobs. Update its tools when they are done.
}
# Progress on a machine's card while its tools are read; $machine is its name.
cluster-tool-reading = Reading the tools on { $machine }…
# Progress on a machine's card; $tools are the tools, $machine its name.
cluster-tool-updating = Updating { $tools } on { $machine }…
cluster-tool-nothing = No tool on this machine can be updated now.
cluster-tool-dismiss = Dismiss

## Install And Sign In Tools

# Button on a machine's card for a tool it doesn't have; $tool is Codex or Antigravity, $machine the machine.
cluster-tool-install-button = Install { $tool } on { $machine }
# Button on a machine's card for a tool that isn't signed in; $machine is the machine.
cluster-tool-sign-in-button = Sign In on { $machine }
# Under an install button while the machine runs a job; $machine is its name, $count the jobs.
cluster-tool-install-busy = { $count ->
    [one] { $machine } is running a job. Install tools when it is done.
   *[other] { $machine } is running { $count } jobs. Install tools when they are done.
}
# Button on a machine's card when a tool's sign-in can't be told; it reads the sign-in again. $machine is the machine.
cluster-tool-check-button = Check Sign-In on { $machine }
# Under a sign-in button while the machine runs a job; $machine is its name, $count the jobs.
cluster-tool-sign-in-busy = { $count ->
    [one] { $machine } is running a job. Sign in when it is done.
   *[other] { $machine } is running { $count } jobs. Sign in when they are done.
}
# Result on a machine's card after Cancel in the sign-in sheet; $tool is the tool, $machine the machine.
cluster-tool-sign-in-cancelled = Signing in { $tool } on { $machine } was cancelled.
# Progress on a machine's card; $tool is the tool, $machine the machine.
cluster-tool-installing = Installing { $tool } on { $machine }…
cluster-tool-signing-in = Signing in { $tool } on { $machine }…
cluster-tool-checking-login = Checking the { $tool } sign-in on { $machine }…
cluster-tool-install-nothing = Nothing can be installed on this machine now.
cluster-tool-sign-in-nothing = No sign-in is needed on this machine now.

## Sign In Sheet

# Title of the sheet that shows a one-time code; $tool is Codex, $machine the machine.
cluster-login-title = Sign In { $tool } on { $machine }
# While the machine starts its sign-in and has not shown a link yet.
cluster-login-starting = Starting the sign-in on { $machine }…
# Steps under the link and code; $machine is the machine that waits.
cluster-login-steps = Open the link in your browser on this Mac, sign in with the account this Mac uses, and enter the code. { $machine } signs itself in when you approve.
# Label above the sign-in link.
cluster-login-link-label = Link
# Label above the one-time code.
cluster-login-code-label = One-Time Code
# Warning in the sheet: signing in again replaces the sign-in the machine has now. $machine is the machine.
cluster-login-replaces = Signing in here replaces the sign-in { $machine } has now.
# Note under the code; it is shown only in this sheet.
cluster-login-private = The link and code are shown only here. Notchling doesn't keep them.
# Button that opens the sign-in link in the browser.
cluster-login-open = Open Link
# Button that stops the sign-in.
cluster-login-cancel = Cancel
# How long the sheet waits; $minutes is a number.
cluster-login-waiting = Waiting for you to approve. This stops after { $minutes } minutes.
# Button that closes the sheet without checking.
cluster-login-close = Close
# Button that reads the sign-in again.
cluster-login-check = Check Again

## Admin password from the Vault

# Button beside an admin password field; it opens the list of Vault entries.
cluster-vault-use = Use From Vault
# Item in that list that opens the Vault's own form for a new entry.
cluster-vault-new = New Entry…
# Shown in the list while the Vault is read.
cluster-vault-reading = Reading the Vault…
# Shown in the list when the Vault has no entries.
cluster-vault-empty = The Vault has no entries yet.
# Shown in the list when the Vault can't be read; $error is the reason.
cluster-vault-read-failed = Couldn't read the Vault: { $error }
# Detail of a chosen entry that is no longer in the Vault.
cluster-vault-gone = Not in the Vault
# Line under the field when an entry is chosen; $name is the entry's name.
cluster-vault-using = Using { $name } from the Vault. A password typed here is used instead.
# Button that stops using the chosen entry.
cluster-vault-stop = Don't Use Vault
# Kind shown beside an entry in the list.
cluster-vault-kind-login = Login
cluster-vault-kind-token = Token
# Shown in the list when the Vault switch is off.
cluster-vault-off = The Vault is off. Turn it on in Settings to use an entry.
# Title of the approval window for Notchling's own sudo step; $names are the entries, $machine the machine.
cluster-vault-approve-title = Allow Notchling to use { $names } for sudo on { $machine }?
# Line under that title in the approval window.
cluster-vault-approve-note = Notchling reads this password only for the sudo step, sends it to the machine over ssh and never shows it.
# Shown on the sheet when the chosen entry can't be saved on the machine; $error is the reason.
cluster-vault-save-failed = Couldn't keep that entry for the machine: { $error }
