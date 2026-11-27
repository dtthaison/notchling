# When the laptop is gone

> **Status, 2026-11-16: you move by hand; nothing moves by itself, and in v1 nothing will.**
>
> **Works now:** Move to <name> Now from the phone's Machines popup and from the app (a handover when the primary is online, a promote when it isn't), the fence in the daemon (a session that moved is denied its next tool call on the old primary), Take Back and Keep on the old primary, side copies (Open, Hide) and the inbox items for all of it.
>
> **Automatic failover is not in v1 (the maintainer decided on 2026-11-16).** Settings > Cluster says "Not available yet" in the Automatic Failover row and offers no choice. There is no lease, no arming and no automatic promote, so the primary never pauses sessions ahead of a deadline and a Standby never promotes by itself. Move by hand with Move to <name> Now.
>
> **Not built yet (the maintainer hasn't decided):**
> - **Waiting sessions never appear.** The detection of an old primary that comes back while the newer one is off isn't built, so nothing raises "waits for m1", and Continue From This Copy can't succeed while the newer primary is off (see below).
> - **Resume is refused.** A move or promote that would resume sessions is refused with "Moving the primary needs the resume proof first" until the maintainer records the resume proof (M5).
>
> Don't count on this for a real failure until M5 is recorded and the live check (checkpoint 2 of the plan) has passed. The record behind all this is [XXXX-cluster-failover-and-fencing](decisions/XXXX-cluster-failover-and-fencing.md), still Proposed.

Your cluster has one primary (the laptop, m5 here) and Standbys. If m5 is asleep in a bag, crashed or off the network, its sessions stop. This is what you do, and what Notchling does for you. A machine with Can Be Master on, a snapshot of m5's sessions and a working link can take over.

## The rule

A session never runs tool calls on two machines at once. Before every tool call, Notchling's hook checks a small file (`cluster/fence.json`, written by the daemon) that says whether this machine may still run that session. If the session moved, the call is denied and says where to open it. If Notchling can't tell, the call waits up to 20 seconds and is then denied. Nothing m5 did is lost: work it did after the snapshot is kept as a copy on m5.

Today the file holds the "this session continues on another machine" answer (after a promote or a handover, on the machine that lost the sessions). The "pause before a deadline" answer needs the lease, which isn't built.

## Automatic Failover

Not available in v1. Settings > Cluster, on a Mac in a cluster, shows the row with "Not available yet" and the way to move by hand; there is nothing to choose. Nothing moves by itself: you move m5's work with Move to <name> Now, in the cluster view or from the phone's Machines popup (below). The daemon still keeps the `clusterFailover` value for when automatic failover is built, but nothing reads it.

## From the phone

When the phone can't reach m5, it falls back to another machine. Two inbox items offer the way out: **Move to m1 Now** after a minute without m5, or, on a machine that can't be primary, **Choose a Machine**. Either opens the Machines popup, titled "Choose a Machine": "Your main machine can't be reached. Pick the machine that should take over." It lists the machines that are online:

- **Can resume old sessions · copy from 14:02**: a Can Be Master machine with a snapshot. Pick it to move there.
- **Controls jobs only**: ready, but the phone doesn't know of a snapshot on it. It starts new sessions and runs jobs. The phone only knows a copy's age when the machine it reached holds it, so a machine that may have a copy can read "Controls jobs only"; it stays pickable and the Mac decides.
- **Not ready to take over**: Can Be Master is on, but a readiness check fails. Listed, not pickable.
- **Can Be Master is off**: listed, but you can't pick it.

Picking one asks "Move to m1 now? Sessions continue there from 14:02. Work m5 did after that is kept on m5." (for a machine with no copy: "m1 holds no copy of your sessions, so it will only control jobs. Work done on the old machine stays there.")

- If m5 is reachable through another machine, nothing moves: "The old machine is reachable through another machine; nothing moved." Your commands keep working through that machine.
- If it isn't, the Mac asks you to confirm the risk, "Move anyway?": if m5 is still running, its sessions keep going until it hears of this. Choose **Move Anyway** only if you accept that.

Moving the primary needs the phone's `acting` switch and the bridge on, as for any command. The other phone questions (Take Back, Continue From This Copy) ask first as well.

## From the app

In the cluster view (Chat's footer), a Can Be Master Standby's row has **Move to <name> Now**.

- If the primary is online, this is the handover: it waits for working sessions to end their turns, then makes that machine the primary.
- If the primary can't be reached, it is the promote. A sheet says "m5 can't be reached. m1 becomes the primary from its last set. Work m5 did after that stays on m5 as a side copy." and shows its steps as they run (Checking, Looking for m5, Making m1 the primary, Restoring the sessions from the last set, Starting the sessions on m1, Sending the messages that were held, Listing what needs a look). **Move Anyway** appears only if the Mac named a risk.
- The Move sheet warns when the old primary couldn't pause the moved sessions after the switch ("They may run on both machines until it stops them").

## After the move

- Sessions that were running continue on m1 from the snapshot. m1 never sends them a message by itself.
- If a turn was cut in the middle of something that may have reached outside (a `git push`, a published page), you get an item: "m5 dropped while `git push` was running. Check, then Continue." Look at what happened first. **Continue** sends the session one reply, "Continue"; **Done** closes it. Nothing is rerun for you.
- The inbox says "m1 is primary now".

## When m5 comes back

m5 hears the newer term and steps down. Its inbox says **m1 is primary · Take Back / Keep m1**.

- Its next tool call in a session that moved is denied: "This session continues on m1. Open it there." A running Codex turn in such a session is interrupted.
- **Keep m1** leaves things as they are.
- **Take Back** (also on m5's own row in the cluster view) brings the primary home to m5. Its side copies are moved aside first (never overwritten, never deleted), then the sessions come home after their turns end. If it fails, an item says why.

## Side copies

Work m5 did after the snapshot m1 resumed from stays on m5 as a side copy: the transcript, and the code in its folder. The cluster view has a **Side Copies** card: the session, the machine, since when. **Open** shows the transcript read only in Chat (only for a copy on this Mac). **Hide** takes it off the list everywhere. Notchling never deletes or merges a side copy, and Hide only hides: nothing on disk changes, and there is no way to show a hidden one again.

## If m5 comes back while m1 is off

Not detected yet. This is the case Continue From This Copy is for, and what is built is the half after the detection:

- A Worker can't tell m5 about the newer term. So m5 stays primary, at its old term, until a Can Be Master machine reaches it. Until then the Workers refuse m5's calls with `stale_term`. Whether a Worker should be able to tell m5 is an open decision for the maintainer (R95 in the plan's ledger); signed terms would settle it.
- A session m1 touched, or one m5 can't be sure about, is meant to **wait**: "`<title>` waits for m1, which has newer work in it." with **Wait** (keeps it waiting) and **Continue From This Copy** ("Continue `<title>` here? The newer copy on m1 is kept beside it."). Nothing raises that item yet, and Continue From This Copy sends its claim to the primary, which is the machine that is off in this case, so it fails and the session keeps waiting. Both need the unbuilt part of the cluster core (Task 7).

## Not covered

- Codex CLI sessions you start outside Notchling can't be fenced and don't move.
- Antigravity and shell sessions never move.
- There is no phone push. Failover and the items above reach the phone in its inbox when it connects.
- A machine that was the failover target and is gone for good keeps m5's sessions paused after the deadline until it is released. A "Forget This Machine" for that is an open question. (It matters only once the lease exists.)
- Claims and side copies are never pruned; a cluster with thousands of moved sessions would grow the fence file toward its 64 KB limit and show a notice.
