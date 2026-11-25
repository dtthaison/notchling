### The Inbox card that asks you to allow a switch of this Mac's Tailscale network.
### The maintainer reviews every change to this file, in every language.

## The Tailscale network

# What the request does: switch this Mac to another Tailscale network. $from and $to are network names.
consent-network-switch = Switch this Mac's Tailscale network from { $from } to { $to }
# Says how many machines lose their link when the network changes. $count is a number.
consent-network-drops = { $count ->
    [one] { $count } machine will lose its connection
   *[other] { $count } machines will lose their connection
}
# One organization's machines that lose their connection when the network changes. $organization is the organization's name; $machines is the machine names, joined by commas.
consent-network-drops-in = { $organization }: { $machines }
# Warns that cluster members lose this Mac when the network changes. $members is how many members there are; $failover is the failover setting.
consent-network-cluster = { $members ->
    [one] { $members } cluster member loses this Mac. Failover is { $failover }.
   *[other] { $members } cluster members lose this Mac. Failover is { $failover }.
}
# Warns that the phone may lose the bridge when the network changes.
consent-network-phone = Your phone may lose the bridge if it uses Tailscale.
