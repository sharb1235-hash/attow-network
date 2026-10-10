# Original WASM component fixtures

These small WAT components were authored for Attow Network's Stage 5 tests.
They are original project material under MIT OR Apache-2.0, not copied guest
programs. The standard component canonical ABI syntax follows Wasmtime and the
WebAssembly Component Model; those projects remain separately licensed.

Each fixture imports the three bounded scalar host functions, instantiates one
core guest module, and canonically lifts its `step` export. Tests assemble WAT
with the pinned `wat` crate, then pass the binary to actual Wasmtime admission
and execution. No fixture uses WASI, external endpoints, host credentials,
filesystem access, a clock, or a random source.

| Fixture | Purpose |
| --- | --- |
| `arithmetic.wat` | Add the first state byte and continuation byte. |
| `continuation.wat` | Suspend with explicit continuation 7, request 99, delta 3; resume by adding state, continuation, and recorded observation. |
| `fuel-loop.wat` | Execute an actual infinite guest loop until fuel exhaustion. |
| `output-overflow.wat` | Attempt 32 output bytes against a smaller host output budget. |
| `memory-two-pages.wat` | Declare fixed two-page memory; reject execution with a one-page profile. |
| `multiple-memories.wat` | Exercise admission's single-memory restriction rather than silently granting a per-memory budget twice. |
| `hidden-global.wat` | Depend on a hidden mutable counter; the admitted subset must reject it. |
| `conditional-trap.wat` | Emit partial output then trap for observation 0; a fresh attempt with observation 1 returns only byte 1. |
| `invalid-output-byte.wat` | Attempt to emit 256 as a byte. |
| `invalid-input-channel.wat` | Attempt to access unknown input channel 99. |

The same platform-neutral fixtures run in native Windows and Linux CI jobs.
Their semantics do not depend on copy-on-write memory initialization. Native
platform execution claims come from actual test results, not from fixture
availability. The new-process resume test uses a SQLite-backed object repository
and recreates the Engine, admitted component, Store, and guest instance in the
child; it does not serialize a live Store or Rust future.
