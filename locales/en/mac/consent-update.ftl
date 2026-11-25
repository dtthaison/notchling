### The Inbox card that asks you to allow an update of a tool or of Notchling on a machine.
### The maintainer reviews every change to this file, in every language.

## A tool on this Mac

# What the request does: update a tool. $tool is the tool's name; $method is how it was installed; $from is its version now; $to is the version it would get.
consent-update-tool = Update { $tool } ({ $method }) from { $from } to { $to }
# What the request does: update a tool to its newest version. $tool is the tool's name; $method is how it was installed; $from is its version now.
consent-update-tool-latest = Update { $tool } ({ $method }) from { $from } to its newest version
# What the request does: go back to an older version of a tool. $tool is the tool's name; $from is its version now; $to is the version it goes back to.
consent-update-back = Go back { $tool } from { $from } to { $to }
# Said on every tool update: sessions that are running keep the old version.
consent-update-running = Sessions running it keep the old version until they restart.

## On a worker machine

# Names the machine whose tools would be updated. $machine is the machine's name; $organization is its organization.
consent-update-machine = Update the tools on { $machine } in { $organization }
# One tool on a worker machine. $tool is the tool's name; $from is its version there; $to is the version of this Mac.
consent-update-worker-tool = { $tool }: { $from } to { $to }
# Names the Notchling program that would be copied to a machine. $sha is the start of its sha256 checksum.
consent-update-notchling = Copies Notchling (sha256 { $sha })
