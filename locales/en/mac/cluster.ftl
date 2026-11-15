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

## Create and join

cluster-none = No cluster yet. Create one to share work between your machines.
cluster-name-label = Cluster Name
cluster-create = Create Cluster
cluster-create-failed = Couldn't create the cluster: { $error }
cluster-copy-token = Copy Join Token
cluster-token-copied = Join token copied. It works once, for 10 minutes.
cluster-token-failed = Couldn't make a join token: { $error }
cluster-change-failed = Couldn't change the node: { $error }

## Sidebar

# Shown in the reply box of a session on a node that is offline; $name is the node's name.
cluster-node-offline = { $name } is offline
