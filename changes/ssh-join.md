---
kind: feature
---
Add Machine in the Cluster sheet adds a machine by address (`user@host`) over SSH: Notchling checks it, shows each step with its commands first, installs Notchling and its service, signs in Tailscale on Linux when you ask, and joins it to the cluster with no Add click in the Inbox. A Mac added this way runs Notchling's daemon as a system service with no app, so it stays online with nobody logged in. Join Cluster on a machine already in Machines (an overnight worker) does the same for the node steps only and leaves the worker alone. When this Mac's build changes, the Cluster sheet and Machines offer Update N Machines, one machine at a time.
