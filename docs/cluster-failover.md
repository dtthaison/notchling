# When the laptop is gone

> **Status, 2026-11-15: don't follow this yet.** The pieces that decide when to move are built and tested, but the parts you would use are not: the fence in the daemon (Task 8, so no session is paused or denied yet), the promote, Take Back and Continue ops with their inbox items (Task 10), the phone's Machines popup (Task 12), and the app's Move to, Take Back and side copies (Task 13), and the cluster core and simulator that tie them together (Task 7). They wait on the cluster plan D. What works today: the Automatic Failover setting (Settings > Cluster), the fence hook installed with Install Hook, and Install Hook for a headless Standby. The record behind all this is [XXXX-cluster-failover-and-fencing](decisions/XXXX-cluster-failover-and-fencing.md), still Proposed.

Your cluster has one primary (the laptop, m5 here) and Standbys. If m5 is asleep in a bag, crashed or off the network, its sessions stop. This is what you do, and what Notchling does for you. A machine with Can Be Master on, a snapshot of m5's sessions and a working link can take over; m1 is the one that does it by itself.

## The rule

A session never runs tool calls on two machines at once. Before every tool call, Notchling's hook checks a small file that says whether this machine may still run that session. If the session moved, the call is denied and says where to open it. If Notchling can't tell, the call waits up to 20 seconds and is then denied. Nothing m5 did is lost: work it did after the snapshot is kept as a copy on m5.

## Automatic Failover

Settings > Cluster, on a Mac in a cluster. Off by default.

- **Off**: nothing moves by itself. You move m5's work by hand (below).
- **After 30 Minutes** or **After 1 Hour**: when m1 can't reach m5 for that long, m1 takes over m5's sessions. m5 pauses the sessions that would move 2 minutes before, on its own, so they never run on both.

The row says "Automatic failover: not ready" until m1 has confirmed it is ready. After you change the setting it takes a few seconds before it is ready again. An agent can't turn it on without asking you in the inbox.

## From the phone

When the phone can't reach m5, it falls back to another machine and the Machines popup lists the machines that are online:

- **Can resume old sessions · copy from 14:02**: a Can Be Master machine with a snapshot. Pick it to move there.
- **Controls jobs only**: ready, but with no snapshot. It starts new sessions and runs jobs.
- **Can Be Master is off**: listed, but you can't pick it.

Picking one asks "Move to m1 now? Sessions continue there from 14:02. Work m5 did after that is kept on m5." After a minute without m5, the inbox also offers **Move to m1 Now**.

- If m5 is reachable through another machine, nothing moves: "m5 is reachable through m1; nothing moved". Your commands keep working through that machine.
- If it isn't, the move asks you to confirm the risk: if m5 is still running, its sessions keep going until it hears of this. Choose **Move Anyway** only if you accept that.

Moving the primary needs the phone's `acting` switch and the bridge on, as for any command.

## After the move

- Sessions that were running continue on m1 from the snapshot. m1 never sends them a message by itself.
- If a turn was cut in the middle of something that may have reached outside (a `git push`, a published page), you get an item: "m5 dropped while `git push` was running. Check, then Continue." Look at what happened first. **Continue** sends the session one reply, "Continue"; **Done** closes it. Nothing is rerun for you.
- The inbox says "m1 is primary now".

## When m5 comes back

m5 hears the newer term and steps down. Its inbox says **m1 is primary · Take Back / Keep m1**.

- Its next tool call in a session that moved is denied: "This session continues on m1. Open it there."
- **Keep m1** leaves things as they are.
- **Take Back** brings the primary home to m5. Its side copies are moved aside first, then the sessions come home after their turns end.

## Side copies

Work m5 did after the snapshot m1 resumed from stays on m5 as a side copy: the transcript, and the code in its folder. The cluster view lists each one: the session, the machine, since when. **Open** shows the transcript read only in Chat. **Hide** takes it off the list. Notchling never deletes or merges a side copy.

## If m5 comes back while m1 is off

m5 learns the newer term from a Worker but can't reach m1.

- A session m1 never touched continues on m5 at once.
- A session m1 touched, or one m5 can't be sure about, **waits**: "`<title>` waits for m1, which has newer work in it." Its tool calls are denied until you choose. **Wait for m1** keeps it waiting. **Continue From This Copy** asks "Continue `<title>` here? The newer copy on m1 is kept beside it." and makes this copy the one that continues. When m1 returns, its newer work is kept as a side copy.

## Not covered

- Codex CLI sessions you start outside Notchling can't be fenced and don't move.
- Antigravity and shell sessions never move.
- There is no phone push. Failover and the items above reach the phone in its inbox when it connects.
- A machine that was the failover target and is gone for good keeps m5's sessions paused after the deadline until it is released. A "Forget This Machine" for that is an open question.
