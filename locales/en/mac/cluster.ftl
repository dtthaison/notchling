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
# Reason in the join token error when the daemon answers without a token.
cluster-token-none = the daemon sent no token
cluster-change-failed = Couldn't change the node: { $error }

## Sidebar

# Shown in the reply box of a session on a node that is offline; $name is the node's name.
cluster-node-offline = { $name } is offline
# Line under a session in the Sidebar when its node is offline; $name is the node's name.
cluster-node-line-offline = { $name } · Offline

## Logins

# One tool's login on a node, in a row of them; $tool is the tool's name and $state one of the four words below.
cluster-login = { $tool }: { $state }
cluster-login-signed-in = Signed In
cluster-login-signed-out = Signed Out
cluster-login-not-installed = Not Installed
# The node has not said whether the tool is signed in.
cluster-login-unknown = Unknown

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
cluster-vault-unreachable = Can't reach the vault on the other machine. Nothing was saved or removed.
# Shown when the machine a form or a removal was opened for is no longer the one this Mac would send it to.
cluster-vault-moved = The vault changed while this was open, so nothing was saved or removed. Open it again.
# Tab label while this Mac can't tell which machine holds the vault.
cluster-vault-tab-unknown = Vault on Another Machine
