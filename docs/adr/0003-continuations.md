# ADR 0003: Explicit portable continuation boundaries

Status: accepted as the target design; executors are planned after foundation.

## Context

A live guest store, browser process or host stack can retain hidden mutable state. Copying only visible application fields and claiming equivalent execution would omit that state. Different backends provide different restoration fidelity.

## Decision

Define a bounded step ABI with explicit input state, continuation, recorded inputs, typed requests and output. Suspend only with no in-flight import and no hidden logical state. Reinstantiate admitted WASM steps between calls and bind the full execution profile into identity. Infrastructure failures remain incomplete attempts rather than deterministic outputs.

Adapters declare materialization and restoration guarantees. Browser fixtures initially use only explicitly supported state; restoring cookies and storage does not imply a cloned JavaScript heap, connection or remote session. Receipt-dependent computation resumes from a recorded effect barrier.

## Alternatives and consequences

Live process/VM snapshots may be useful backend implementations but are neither universally portable nor automatically effect-safe. Wasmtime initialization copy-on-write does not provide live Store cloning. Transparent arbitrary browser cloning would require guarantees unavailable from ordinary context APIs. Explicit boundaries place work on application authors, while making replay and admission requirements testable.

See [worldlines](0001-worldlines.md) and the [architecture](../architecture.md) for the surrounding state and effect contract.
