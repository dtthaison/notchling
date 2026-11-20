### Machines: the Machines button at the foot of the Sidebar, the Machines sheet (the cluster, its members and the machines that aren't in it), and the node names on remote sessions in the Sidebar.

## Button and sheet

# Tooltip of the Machines button at the foot of the Sidebar.
cluster-tip = Machines
# Title of the Machines sheet and name of the footer button.
cluster-title = Machines
# Heading of the part of the Machines sheet that shows the cluster itself: its name, its state and Turn On.
cluster-section-cluster = Cluster
# Heading above the machines that are members of the cluster.
cluster-section-members = In the Cluster
# Tooltip of the + button in the Machines sheet header; it opens Add Machine.
cluster-add-tip = Add Machine
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
# Menu item of a member; it takes the machine out of the cluster only. The machine stays in Machines.
cluster-remove = Leave Cluster…
cluster-remove-title = Leave the Cluster?
# Confirmation text; $name is the node's name.
cluster-remove-note = { $name } leaves the cluster and its sessions leave every list. It stays in Machines and can join again with a new token.
cluster-remove-confirm = Leave

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
# Button that stops waiting for sessions to finish their turns and stops the working ones now.
cluster-move-now = Move Now
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
# Line on the Machines sheet while a run goes on; $name is the machine, $step the step number, $of how many steps there are.
cluster-add-running = Adding { $name }… (step { NUMBER($step) } of { NUMBER($of) })
# Line on the Machines sheet while a run waits for the machine to appear.
cluster-add-joining = Waiting for { $name } to join…
# Line on the Machines sheet when a run is done; $name is the machine.
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

# Button at the foot of the Machines sheet that opens the Add Machine sheet.
cluster-add-machine = Add Machine…
cluster-add-title = Add Machine
# Title of the sheet once a machine is chosen; $name is its name.
cluster-add-title-named = Add { $name }
cluster-add-intro = Paste an ssh command or type user@host:port. Nothing is installed on the machine.
# Label of the field that takes the machine's address or a pasted ssh command.
cluster-add-address-label = SSH address or command
cluster-add-address-placeholder = ssh -p 22 user@host
# Label of the name field in the Add Machine form. Left empty, the name is the first part of the host.
cluster-add-name-label = Name (Optional)
# Part of the line under the address field that says what was understood; $user is the user.
cluster-add-seen-user = User { $user }
# Part of the line that says what Add Machine understood; $host is the host.
cluster-add-seen-host = Host { $host }
# Same; $port is the port number as text.
cluster-add-seen-port = Port { $port }
# Same; $key is the path of the key file.
cluster-add-seen-key = Key { $key }
# Same; $name is the machine the connection goes through.
cluster-add-seen-jump = Via jump { $name }
# Two parts of that line side by side.
cluster-add-seen-joined = { $first } · { $second }
# Row that shows or hides the extra Add Machine fields.
cluster-add-more = More Options
# Note in More Options where the kind buttons would be, when the pasted command has -J; $machine is the jump machine.
cluster-add-jump-decides = The command decides this address: through { $machine }
# Error when an ssh command names a jump host that Machines doesn't list; $host is that name.
cluster-add-jump-unknown = { $host } isn't a machine in Machines. Add it first, then use its name after -J.
# Button of the Add Machine form: it checks the machine over SSH, then saves it.
cluster-add-save = Add
# Line when a machine was saved; $name is its name.
cluster-add-saved = { $name } is in Machines. It isn't in the cluster yet: use Join Cluster… on it when you want that.
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
cluster-add-running-note = You can close this. The run goes on, and the Machines sheet shows it.
cluster-add-log-label = What it did
# Row on the Machines sheet while a plan waits for Install and Join; $name is the machine.
cluster-add-row-waiting = { $name } is ready to add.
# Row on the Machines sheet while the machine is read; $name is the machine.
cluster-add-row-checking = Checking { $name }…
# Row on the Machines sheet when a run stopped; $name is the machine, $why the reason.
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
# Under a machine in that section: what the last check found. Reads like "Reachable".
cluster-machine-reach-up = Reachable
cluster-machine-reach-down = Not reachable
cluster-machine-reach-unsure = Can't tell if it is reachable
cluster-machine-reach-none = Not checked yet
# Under a machine in that section; $reach is what the check found, such as Reachable, and $age how long ago, such as 5 minutes ago.
cluster-machine-checked = { $reach } · Checked { $age }
# Button on a machine in that section: asks the machine again over SSH.
cluster-machine-check = Check
# Shown on a machine's line while its check runs.
cluster-machine-checking = Checking…
# Menu item of a machine that isn't in the cluster; it takes the machine out of Machines.
cluster-machine-remove = Remove from Machines…
cluster-machine-remove-title = Remove from Machines?
# Confirmation text; $name is the machine's name.
cluster-machine-remove-note = { $name } leaves Machines with its settings. Nothing is changed on the machine itself.
cluster-machine-remove-confirm = Remove
# Refusal when a machine that is in the cluster is picked for removal from Machines; $name is the machine's name.
cluster-machine-remove-member = { $name } is in the cluster. Choose Leave Cluster… on it first.
# Refusal when a machine is picked for removal while its Join Cluster… run is still going; $name is the machine's name.
cluster-machine-remove-busy = { $name } is being set up. Wait for it to finish first.
# Refusal when a machine is picked for removal while the Cluster has not been read yet.
cluster-machine-remove-unread = The Cluster is not read yet. Try again when the sheet shows it.

## Update Machines

# Line in the Machines sheet when members run another Notchling build than this Mac; $count is how many, $names their names.
cluster-update-banner = { $count ->
    [one] { $count } machine runs another Notchling build: { $names }
   *[other] { $count } machines run another Notchling build: { $names }
}
# Button on that line; $count is how many machines.
cluster-update-button-open = { $count ->
    [one] Update { $count } Machine…
   *[other] Update { $count } Machines…
}
# Button that runs the update; $count is how many machines will be updated, $name the machine when there is one.
cluster-update-button-run = { $count ->
    [one] Update { $name }
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
# Dimmed line for a machine that already runs this Mac's build.
cluster-update-up-to-date = Already up to date.
# Dimmed line for this Mac in the update list.
cluster-update-this-mac = This Mac. The others are updated from it.
# Asks for the admin password of a machine; $machine is its name.
cluster-update-password-for = Admin password for { $machine }
cluster-update-password-placeholder = Admin password
# Under a machine whose admin password is in the Vault; $name is the Vault item.
cluster-update-saved-in = Saved in Vault · { $name }
# Button that picks another Vault item for the machine.
cluster-update-change = Change…
# Button that opens the list of Vault items for the machine.
cluster-update-choose = Choose Existing…
# Under the password field; $name is the Vault item Notchling will create.
cluster-update-will-save = Saved in the Vault as { $name }, so it isn't asked again.
# Under the password field when the Vault can't be used.
cluster-update-vault-off = The Vault isn't available. The password is used once and not saved.
# Under the password field when the machine's Vault item was deleted; $name is that item.
cluster-update-link-gone = The Vault item { $name } is gone. Type the password again.
# Note of the Vault item Notchling creates; $machine is the machine's name.
cluster-update-item-note = macOS admin password for { $machine }, used by Notchling updates
# Shown in place of the list of Vault items while it is read.
cluster-update-reading = Reading the Vault…
# Reason the update can't start yet; $names are the machines that need a password.
cluster-update-needs-password = Type the admin password for { $names } to update.
# Reason when every machine is switched off.
cluster-update-pick-one = Choose a machine to update.
# Shown when saving the password in the Vault failed; $error is the reason.
cluster-update-save-failed = Couldn't save the password in the Vault: { $error }. Nothing was changed.
# Second sentence of the update line in the Machines window.
cluster-update-from-cluster = Update them from the Machines sheet in Notchling.

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
# Result on a machine's card when the cluster can't be read just before an install or sign-in; $machine is the machine.
cluster-tool-refresh-failed = Couldn't read the cluster before changing { $machine }. Nothing was changed.
# Progress on a machine's card while Cancel stops a sign-in; $machine is the machine.
cluster-tool-sign-in-stopping = Stopping the sign-in on { $machine }…
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
# How long the sheet waits; $minutes is a number.
cluster-login-waiting = Waiting for you to approve. This stops after { $minutes } minutes.
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

## Machine addresses

# Heading over the list of ways to reach a machine, in Add Machine and Edit Addresses.
cluster-address-list-label = Addresses
# Line under that heading. Notchling tries the addresses from the first down and uses the first that answers.
cluster-address-list-note = Notchling tries them from the top and uses the first that answers. A Tailscale address on a network this Mac isn't on is skipped.
# Placeholder of an address field: an optional user, a host and an optional port.
cluster-address-placeholder = [user@]host[:port]
cluster-address-add = Add Address
cluster-address-up = Up
cluster-address-down = Down
cluster-address-remove = Remove
# Kinds of address, shown on the button that changes the kind.
cluster-address-kind-tailscale-name = Tailscale Name
cluster-address-kind-tailscale-ip = Tailscale IP
cluster-address-kind-lan = LAN
cluster-address-kind-public = Public
cluster-address-kind-jump = Jump
# Button on a Tailscale address that picks which Tailscale network it belongs to; any network means it is never skipped.
cluster-address-network-any = Any Network
# Button on a Tailscale address; $name is the network it belongs to, as Tailscale lists it.
cluster-address-network = Network: { $name }
# Button on a jump address when no machine is chosen yet.
cluster-address-via-none = Choose Machine
# Button on a jump address; $machine is the machine Notchling goes through.
cluster-address-via = Through { $machine }
# Error on an address; $n is its place in the list, $why what is wrong.
cluster-address-row-n = Address { NUMBER($n) }: { $why }
cluster-address-row-control = An address can't hold control characters
cluster-address-row-spaces = An address can't hold spaces
cluster-address-row-empty = Type an address
cluster-address-row-shape = Type the address as [user@]host[:port]
cluster-address-row-bracket = Close the IPv6 address's bracket
cluster-address-row-port = The port is a number from 1 to 65535
cluster-address-row-host = The host is a name or an IP address
cluster-address-row-user = The user is lowercase letters, digits, hyphens and underscores

# Line under a machine in Machines: the way it is reached now.
cluster-machine-via-tailscale = via Tailscale
# Same, on a named network; $network is the Tailscale network.
cluster-machine-via-tailscale-network = via Tailscale · { $network }
# Same, over the local network; $host is the address.
cluster-machine-via-lan = via LAN · { $host }
# Same, over a public address or name; $host is the address.
cluster-machine-via-public = via { $host }
# Same, through another machine; $machine is that machine.
cluster-machine-via-jump = via { $machine }
# Line under a machine whose only usable addresses are on a Tailscale network this Mac isn't on; $wanted is that network and $active the one this Mac is on.
cluster-machine-wrong-network = On { $wanted } · this Mac is on { $active }
# Line under a machine when none of its addresses answered.
cluster-machine-no-address = No address answered
# Button on that line: switches this Mac's Tailscale to the machine's network.
cluster-machine-switch-network = Switch Network
# Title of the question before switching; $name is the network.
cluster-machine-switch-title = Switch Tailscale to { $name }?
# Note in that question; $active is the network this Mac leaves.
cluster-machine-switch-note = This Mac leaves { $active }, so its machines there drop until you switch back. Nothing else changes.
cluster-machine-switch-confirm = Switch
# Shown when the switch failed; $why is the reason.
cluster-machine-switch-failed = Couldn't switch Tailscale: { $why }
# Item in a machine's menu.
cluster-machine-edit-addresses = Edit Addresses…
# Title of the Edit Addresses sheet; $name is the machine.
cluster-address-edit-title = Edit Addresses for { $name }
cluster-address-edit-save = Save
# Shown after Add Machine's check when the machine's addresses are on another Tailscale network; $wanted is that network, $active the one this Mac is on.
cluster-address-add-wrong-network = The machine is on { $wanted }, and this Mac is on { $active }. Switch Network on its row once it is saved, or change the address.
# Shown when saving addresses failed because the machine is gone; $name is the machine.
cluster-address-edit-gone = { $name } is not in Machines any more.

## From Tailscale

# Button in Add Machine: lists the machines on the Tailscale network this Mac is on.
cluster-tailscale-button = From Tailscale…
cluster-tailscale-title = Add from Tailscale
# Line over the list; $network is the Tailscale network this Mac is on.
cluster-tailscale-network = Machines on { $network }
cluster-tailscale-reading = Reading Tailscale…
# Shown when Tailscale couldn't be read; $why is the reason.
cluster-tailscale-failed = Couldn't read Tailscale: { $why }
cluster-tailscale-not-running = Tailscale isn't running on this Mac.
cluster-tailscale-filter-placeholder = Filter
cluster-tailscale-os-any = Any System
# Button that limits the list to machines that are on now.
cluster-tailscale-online-only = Online Only
cluster-tailscale-online-all = Online and Offline
cluster-tailscale-select-all = Select All
cluster-tailscale-select-none = Select None
# Button at the foot of the list; $count is how many machines are chosen.
cluster-tailscale-import = { $count ->
    [one] Import { $count } Machine
   *[other] Import { $count } Machines
}
cluster-tailscale-import-none = Import
cluster-tailscale-online = Online
cluster-tailscale-offline = Offline
# Line under a machine; $os is its system, $ips its Tailscale addresses.
cluster-tailscale-peer-line = { $os } · { $ips }
# Line under a machine when Tailscale doesn't know its system; $ips are its Tailscale addresses.
cluster-tailscale-peer-line-unknown = { $ips }
# Line under a machine in the Tailscale list; $owner is the user who owns it.
cluster-tailscale-peer-owner = Owner: { $owner }
# Same, when the machine is tagged; $owner is the owner Tailscale shows and $tags the tags, such as tag:server.
cluster-tailscale-peer-tagged = { $owner } · { $tags }
# Label over the user field of a chosen machine.
cluster-tailscale-user-label = SSH User
# Line under the list; $count is how many machines are already in Machines and so aren't listed.
cluster-tailscale-hidden = { $count ->
    [one] { $count } machine is already in Machines and isn't listed.
   *[other] { $count } machines are already in Machines and aren't listed.
}
# Line when no machine is left to list.
cluster-tailscale-empty = No machine on this network is left to add.
# Line when the filter matches nothing.
cluster-tailscale-no-match = No machine matches.
# Line at the foot of the list: Notchling lists only the network this Mac is on; $other are the other networks it knows.
cluster-tailscale-other-networks = Switch Tailscale to { $other } to list its machines; API keys for listing both come later.
# Line saying what Import does.
cluster-tailscale-import-note = Import only saves each machine to Machines, with its Tailscale name and address. Nothing is installed on it and it doesn't join the cluster.
# Shown after an import; $count is how many machines were saved.
cluster-tailscale-imported = { $count ->
    [one] { $count } machine is in Machines. Check it to see how it answers.
   *[other] { $count } machines are in Machines. Check them to see how they answer.
}
# Shown after Import when every chosen machine was already in Machines, so none was saved.
cluster-tailscale-imported-none = Nothing was imported: the chosen machines are already in Machines.
# Button in Add from Tailscale that picks which Organization the imported machines belong to; $name is its name.
cluster-tailscale-organization = Organization: { $name }
# Name of the Organization every machine belongs to by default.
cluster-tailscale-personal = Personal
# Title of a machine in Add from Tailscale; $name is its name and $state is Online or Offline.
cluster-tailscale-row-title = { $name } · { $state }

cluster-ssh-key-label = SSH Key
# Button on a machine's row that opens the key picker; $name is the key file's name.
cluster-ssh-key-named = Key: { $name }
cluster-ssh-key-default = Key: Default
cluster-ssh-key-choose = Choose…
cluster-ssh-key-clear = Clear
cluster-ssh-key-placeholder = Default keys and the agent
cluster-ssh-key-picker-prompt = Use as SSH Key
# Line under an SSH Key field.
cluster-ssh-key-note = Empty uses ssh's normal keys and the agent. Only the path is saved, never the key.
cluster-ssh-key-shape = SSH Key: use the full path of a key file, or one starting with ~/
cluster-ssh-key-missing = The key file isn't there.
cluster-ssh-key-not-file = That isn't a file.
cluster-ssh-key-too-open = Other users can read the key file. Run chmod 600 on it.
cluster-ssh-key-unreadable = The key file can't be read.
cluster-ssh-key-public = That is a public key (.pub). ssh needs the private key file, unless the agent holds it.
cluster-ssh-test = Test
cluster-ssh-test-selected = Test Selected
cluster-ssh-testing = Testing…
# Result of Test; $user is the account ssh logged in as.
cluster-ssh-test-connected = Connected as { $user }
# Result of Test when the host's key isn't in known_hosts; $user is the account ssh logged in as.
cluster-ssh-test-new-host = Connected as { $user }, but this host isn't trusted yet. Setup will fail until you trust it.
# Result of Test when a jump host's key isn't in known_hosts; $host is the jump host.
cluster-ssh-test-jump-untrusted = Jump host { $host } isn't trusted
cluster-ssh-test-denied = Wrong user or key (permission denied)
cluster-ssh-test-host-key-changed = Host key changed
cluster-ssh-test-timed-out = Can't reach: timed out
cluster-ssh-test-refused = Can't reach: connection refused
cluster-ssh-test-no-name = Can't reach: name not found
cluster-ssh-test-no-route = Can't reach: no route
cluster-ssh-test-no-answer = Can't reach: no answer
cluster-ssh-test-wrong-network = Not on this Tailscale network
# Result of Test on a machine's address; $wanted is the network the address is on, $active the one this Mac is on.
cluster-ssh-test-wrong-network-named = Not on this Tailscale network: the address is on { $wanted }, and this Mac is on { $active }
# One line of the Edit Addresses sheet's test; $address is the address tested, $result what happened.
cluster-ssh-test-address = { $address }: { $result }
cluster-ssh-key-checking = Checking the key…
cluster-ssh-trust = Trust Host
cluster-ssh-trust-scanning = Reading the host key…
# One host key shown before it is trusted; $kind is the key type, $fingerprint its SHA256 fingerprint.
cluster-ssh-trust-key = { $kind } { $fingerprint }
cluster-ssh-trust-check = Check this matches the machine. Notchling read it over the network and can't check it for you.
cluster-ssh-trust-confirm = Add to Known Hosts
cluster-ssh-trust-saving = Saving…
cluster-ssh-trust-jump = Trust Host doesn't work through a jump host. Connect once from Terminal.
cluster-ssh-trust-nothing = Couldn't read the host key.
# $why is what went wrong writing ~/.ssh/known_hosts.
cluster-ssh-trust-write-failed = Couldn't write known_hosts: { $why }
