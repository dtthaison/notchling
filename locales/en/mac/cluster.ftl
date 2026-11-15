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
