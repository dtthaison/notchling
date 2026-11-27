# SDD ledger — plan: docs/superpowers/plans/2026-11-13-cluster-v1-f-push.md

Spec: docs/superpowers/specs/2026-11-13-notchling-cluster-design.md (Scope v1 step 5; review issue 11)

## Tasks
docs: Task 1: decision record docs/decisions/XXXX-phone-push.md written by the docs lane (status Proposed; index row added). Branch agent/cluster-v1-f-decision from origin/agent-run-oss. No review round (docs only, own check).
docs: Ruling R1: the record adds a "Considered" section between Decision and Consequences, as the plan asks; the template has none. Costs one extra heading if wrong.
docs: Ruling R2: the record flags that APNs limits a collapse id to 64 bytes and leaves the check to Task 2/3 implementers; it does not decide a hashing scheme. Costs one line in the plan if wrong.
merge-1116: Plan F is dropped (maintainer, 2026-11-15: no FCM, no phone push in v1). The merge of agent/cluster-v1-f-decision into merge/run-oss-1116 therefore sets docs/decisions/XXXX-phone-push.md to status Dropped (it was Proposed on the branch), adds "Dropped" to the decisions README status list and marks the index row Dropped. The record stays for the reasoning; no code was built from it.
