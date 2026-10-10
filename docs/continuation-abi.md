# Continuation ABI

The public WIT package is `attow:worldline@1.0.0` under
[`wit/attow-v1`](../wit/attow-v1/world.wit). Its `types` interface describes
`ExecutionProfile`, `StepInput` and the `Done`/`Suspended` result variants as
portable records. Each object ID is exactly 32 bytes. These descriptors express
durable data; the executable guest boundary uses scalar channels so guest-sized
lists cannot allocate during host lifting.

## Executable boundary

The root component may import only these typed functions:

| Function | Behavior |
| --- | --- |
| `input-len(source: u32) -> u32` | Returns the explicit stream's byte length. |
| `input-byte(source: u32, index: u32) -> u32` | Returns a byte; invalid indices fail the attempt. |
| `output-byte(target: u32, value: u32)` | Appends one checked byte within aggregate output/call limits. |

The sole export is `step() -> u32`: `0` means `Done`, `1` means `Suspended`, and
other values are invalid. Input channel `0` is managed-state data, `1` is the
continuation body, and `2` is recorded-observation data (empty when absent).
Output channel `0` is result, `1` is next continuation, `2` is a proposed request,
and `3` is a managed delta. Unknown channels and values above 255 are errors.
Every read, length query and write consumes a host call.

These streams are application data, not an authority API. Emitting delta bytes
does not commit state; emitting a request does not call a provider. The trusted
compositor interprets the typed result and submits eligible proposals to the
owning authority. The interface exposes no credentials or ambient host resource
handles. The composing application must exclude secrets from explicit data.

## Canonical objects

Data streams use known codec objects with payload
`["attow-wasm-data", 1, bytes]` and an empty reference set. A checkpoint uses
`["attow-wasm-continuation", 1, code_id, profile_id, bytes]`, with exactly the code
and profile IDs in its reference set. Each raw stream is limited to 256 KiB.
Code uses `["attow-wasm-code", 1, binary]` with no references. Profiles use
`["attow-wasm-profile", 1, runtime_contract, fuel, memory_bytes, output_bytes]`,
also with no references. The runtime contract binds the exact configuration and
limits described in the [profile](determinism-profile.md). The codec hash includes
kind, schema, length and canonical bytes.

Before resumption, the host verifies the object hash/schema, exact references,
code/profile binding and resource budgets. It compiles or resolves the admitted
code and creates a new instance; it never restores a native stack, live Store,
Rust Future, hidden mutable global or in-flight import. The guest must reconstruct
its logical state from explicit input bytes. Sibling continuations can share
immutable ancestors but have independent next-step data and guest memory.

The repository port commits output objects and an evaluation envelope in one
transaction before a step returns success. The evaluation payload is
`["attow-wasm-evaluation", 1, code, profile, state, old_continuation,
observation_or_null, exit_tag, primary, request_or_null, delta]`; its references
include every named object. `primary` is the result for exit `0` and the next
continuation for exit `1`. Pinning this envelope retains the exact inputs,
profile and outputs. A suspended checkpoint therefore
outlives release of the worker. A persistence failure is an error, not a successful
suspension. Garbage collection follows the checkpoint's code/profile references.

## Completion and failure

The runtime returns `Done { result, delta }` or
`Suspended { continuation, request, delta }` only after validating the returned
shape and persisting its objects. Completed and suspended evaluations carry the
profile identity through their durable objects and shared worker outcome.
Fuel exhaustion, host-call/output limits, invalid input, invalid exit codes and
traps are incomplete attempts. They must not be cached as deterministic success
or promoted as effects.

Every attempt discards its Store after it returns or traps. Wasmtime 48 performs
component post-return work as part of `TypedFunc::call`; the backend does not
invoke the deprecated no-op `post_return`. Process termination provides no final
semantic outcome unless the successful output transaction was already durable.
This API does not imply generic process checkpoint or hostile-code process
containment. See the [determinism profile](determinism-profile.md).
