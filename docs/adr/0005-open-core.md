# ADR 0005: Open correctness and optional managed operations

Status: accepted project policy; no paid service is supplied by foundation.

## Context

A portable execution contract depends on users being able to inspect authority behavior, verify histories, export their data and operate without mandatory hosted coordination.

## Decision

License original project code as MIT OR Apache-2.0. Keep the correctness kernel, local execution, portable worldline format, verification, core adapters, peer protocol and self-hosting path open. Managed fleet scheduling, relay operations, enterprise administration, retention management and support may be separate services, without making paid access necessary for correct promotion or export.

Use Attow Network for public product, repository and package naming. Retain ChronosOS only as the internal working name and in original source snapshots. This naming choice does not establish trademark or patent clearance. Keep packages unpublished until explicit release authorization.

## Alternatives and consequences

A proprietary promotion gateway would make trust and independent operation dependent on one vendor. A mandatory hosted broker would reduce local utility and introduce an availability requirement. Keeping all possible operating services in the initial scope would delay the testable local kernel. The chosen boundary preserves independent verification while leaving operational offerings to later work; it makes no claim of an existing commercial product or adoption.

See the [release runbook](../release-runbook.md), [governance](../../GOVERNANCE.md) and [licenses](../../LICENSE-MIT).
