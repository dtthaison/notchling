### The Inbox card that asks you to allow a change to a machine.
### The maintainer reviews every change to this file, in every language.

## A machine

# Names the machine a request would change. $machine is the machine's name; $user and $host are how Notchling reaches it; $organization is the organization it belongs to.
consent-machine-named = Machine { $machine }, reached as { $user }@{ $host }, in { $organization }
# Lists the addresses of a machine on the consent card. $addresses is a comma separated list.
consent-machine-address-list = Addresses: { $addresses }
# Names the ssh key file a machine is reached with. Only the file name is shown, never its content. $key is the file name.
consent-machine-key-file = ssh key file: { $key }
# Said on a request to add one machine to Notchling. $machine is the machine's name.
consent-machine-add = Notchling adds { $machine } to its machines.
# Said on a request to add machines found on another Notchling. $machines is a comma separated list of machine names.
consent-machine-import = Notchling adds these machines: { $machines }
# Said on a request to change the addresses of a machine. $before and $after are comma separated lists.
consent-machine-addresses = Addresses change from { $before } to { $after }
# Said on a request to remove a machine from Notchling. $machine is the machine's name.
consent-machine-remove = Notchling forgets { $machine }. Nothing on the machine changes.
# One step of a machine's setup. $title is the step's name; $display is exactly what it would run.
consent-machine-step = { $title }: { $display }
# Says what administrator access a request needs. $need is one of the four words below.
consent-machine-admin = Administrator access: { $need }
# A word for the administrator access a request needs: it needs none.
consent-machine-need-none = none needed
# A word for the administrator access a request needs: the administrator password of the machine.
consent-machine-need-password = the administrator password of the machine
# A word for the administrator access a request needs: Allow confirms steps that run as root.
consent-machine-need-root = steps run as root
# A word for the administrator access a request needs: it is more than Notchling may use.
consent-machine-need-refused = more than Notchling may use
# Heading of the box where the administrator password is typed on the consent card.
consent-machine-password-label = Administrator password
# Placeholder of the administrator password field on the consent card.
consent-machine-password-placeholder = Type the password
# Why Allow is off on a machine request that needs the administrator password.
consent-machine-password-needed = Type the administrator password or use a Vault entry, then allow this.
# Why Allow is off on a machine request that needs more administrator access than Notchling may use.
consent-machine-admin-refused = This needs more administrator access than Notchling may use, so it can't be allowed here.
# Button on the consent card that lists the Vault entries that may hold the administrator password.
consent-machine-vault-use = Use From Vault
# Button on the consent card that stops using a Vault entry for the administrator password.
consent-machine-vault-stop = Stop Using Vault
# Says which Vault entry gives the administrator password. $name is the entry's name.
consent-machine-vault-using = Using the Vault entry { $name }
