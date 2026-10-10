# ADR 0006: Isolated proposals and explicit durable admission

Status: implemented in Stage 3.

## Context

Logical alternatives need cheap fork, repeatable identity and complete read
dependencies without duplicating a process or spendable authority. Equality of
visible values can hide an absent-present-deleted ABA change. A serialized branch
with unretained object references is not a durable checkpoint.

## Decision

Share immutable `HashTrieMapSync` roots and give each branch private reads and an
ordered overlay. Record absent reads and blind-write expectations. Keep deletion
tombstones with positive monotonically assigned versions and use a conservative
whole-namespace epoch for all scans. Canonicalize durable snapshots independently
of trie iteration. Bind branch IDs to the domain, base root and caller nonce.

Distinguish in-memory `freeze` from transactional `admit`. Admission persists base
and descriptor, verifies the full declared reference graph and pins the descriptor
in one trusted backend transaction. Reopen verifies hashes and expected domain.
Data-only import explicitly discards source history and identity. The state crate
defines ports; the facade composes them with SQLite without a production state-to-
store dependency.

Portable metadata includes code/profile, continuation, observations, policy,
bindings, security scope and ancestry. The standard-library worker port accepts
bounded canonical step envelopes. A worker handle is not a checkpoint, a caller's
effect label is not permission for pure reuse, and infrastructure failure is an
error rather than a successful deterministic outcome.

Join reports conflicting shared writes, read/write overlap and scan/write overlap.
A compatible join is still a proposal requiring later authority validation.

## Alternatives and consequences

Copying mutable dictionaries per fork would waste memory and obscure isolation.
Deleting tombstones would allow absence ABA to look unchanged. Tracking only keys
returned by a scan would miss phantom inserts. Treating a repeatable merge as
authorization would miss business invariants and external uncertainty. Treating
freeze as durable without a transaction would leave dangling or collectable roots.

The flat durable snapshot keeps the initial format inspectable but makes apply,
first loading and serialization proportional to state size, with sorting overhead.
It is not a disk-backed persistent tree. Whole-namespace epochs intentionally
produce conservative conflicts. Tombstones count toward the finite cell budget;
safe compaction requires a later retention/version protocol. No native stack,
physical SQL clone, arbitrary browser restoration, CRDT convergence or copied
spendable rights is promised.

Persistent collections and branchable snapshots are established prior art. See
[worldline representation and references](../worldline-ir.md) and the earlier
[authority decision](0002-authority.md) for the serializability boundary.
