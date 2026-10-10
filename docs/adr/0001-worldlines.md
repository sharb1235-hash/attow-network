# ADR 0001: Explicit worldlines and immutable shared state

Status: accepted as the target design; runtime implementation follows staged gates.

## Context

Alternative executions span programs, browser sessions, databases and workflow systems. Copying a worker does not by itself describe its observations, hidden dependencies or authority. Equal state bytes also do not imply equivalent histories.

## Decision

Represent each logical worldline with typed immutable references to ancestry, code/profile, state, continuation, observations, policy and backend bindings, plus distinct worldline/domain identity. Share immutable objects and only reuse computations with a complete verified key and admitted effect class. Keep resident worker limits separate from logical branch counts.

## Alternatives and consequences

Copying entire processes is backend-specific, expensive and not portable. Mutable per-branch JSON copies obscure versions and read dependencies. Treating equal hashes as interchangeable branches loses authority and sample identity. A portable descriptor makes sharing inspectable but requires explicit serialization, versioning and dependency capture. Arbitrary browser state cloning remains outside the guarantee; adapters must declare restoration fidelity.

Next decisions: [authority](0002-authority.md) and [continuation boundaries](0003-continuations.md). The [architecture](../architecture.md) describes the planned IR; stage 1 only introduces its crate boundaries and initial identifier type.
