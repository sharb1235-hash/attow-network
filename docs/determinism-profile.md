# Determinism profile

Attow Network executes an admitted WebAssembly component as one bounded step. A
fresh Wasmtime `Store` and instance receive explicit immutable input bytes for
each attempt. Only the engine and compiled code are shared. The current support
level is an in-process backend using Wasmtime's sandbox; hostile-code process
isolation, operating-system memory/CPU quotas and compilation deadlines have not
been implemented or tested.

## Profile identity

`ExecutionProfile` contains fuel, aggregate guest memory and aggregate output
budgets, plus a canonical `profile_hash`. The hash binds the exact runtime and
parser versions, engine feature/configuration contract, executable ABI, host-call
cap, structural admission limits and target operating system/architecture.
Changing a public budget field without recomputing a valid profile is rejected
before execution. The profile itself is a versioned known codec `Object`, so its
identity and graph references follow the same persistence rules as other objects.

The initial runtime is Wasmtime 48.0.5 with locally compiled component binaries.
The parser is wasmparser 0.254.2, derived from Wasmtime's resolved dependency in
`Cargo.lock`, not the newer parser used by test WAT tooling. Features enable
Cranelift, the synchronous component runtime and the pooling allocator. No WASI
implementation is linked. Native compiled artifacts from peers are never
deserialized.

## Supported component subset

Admission accepts binary components no larger than 512 KiB. It validates
structure before compilation, including counts of modules, instances, types,
functions, locals and operators. One core module may be instantiated once;
one additional core instance may bundle the three mediated host functions.
Nested components, component instances, start functions and imported memory,
tables or globals are rejected.

At most one memory and one table exist per worker. Memory must declare equal
minimum and maximum sizes with standard 64 KiB pages, at most 16 MiB in total.
A table must similarly have a fixed size of at most 1,024 function references.
Shared memory, memory64 and custom page sizes are excluded. Mutable core globals
are rejected; linear memory is always recreated for the next step and is not a
checkpoint. Programs must emit every value needed after suspension explicitly.

Fuel consumption and NaN canonicalization are enabled. SIMD, relaxed SIMD,
threads, GC, exceptions, tail calls and asynchronous component features are
excluded. Fixed memory/table sizes make successful post-instantiation growth
unavailable. Allocation failure before entry is an incomplete infrastructure
attempt, never a semantic result. Fuel is a runtime instruction budget, not a
wall-clock deadline.

## Resource accounting

Profiles admit fuel from 1 through 100,000,000, memory from 1 byte through 16 MiB,
and aggregate raw output from 1 byte through 256 KiB. A component whose declared
memory exceeds the profile cannot instantiate. The one-memory admission rule
turns Wasmtime's per-memory limiter into an aggregate linear-memory bound for
that worker. Runtime metadata, compiled code and host buffers are separate
allocations, not part of this guest-memory number.

The host permits at most 1,000,000 scalar calls per step. The shared worker port
requires this exact host-call budget and exact fuel/memory/output values from
the admitted profile. Different fuel/memory/output budgets require a separately
admitted profile; the initial host-call cap is fixed. Inputs are three individually bounded
streams of at most 256 KiB. All four output streams together consume the output
budget. Every byte is checked before append, including channel and byte range,
so lifting a guest-declared list cannot allocate an unbounded host vector. The
output cap leaves room for canonical object and continuation metadata.

The engine pool caps simultaneous component/core instances, memories and tables
at 16 each, with one of each per component. Each guest stack is limited to
512 KiB. The executor obtains one of 16 active-worker permits before loading
input buffers. Compilation admission is serialized by the executor; its cache holds
at most 16 components and 8 MiB of source binaries. These bounds limit accepted
inputs and resident instances. They are not a hard process RSS or compilation
time guarantee.

## Replay and upgrades

Only the explicit input, continuation and recorded-observation streams reach
the guest. No host import reads wall time, randomness, network, filesystem,
environment variables or credentials. An observation request is returned as
data for the compositor; it grants no external authority. The exact scalar ABI
and portable checkpoint format are in [continuation ABI](continuation-abi.md).

Replay must bind the code, profile, complete inputs, continuation, observations,
tenant/security scope and independent sample identity. A caller's `Pure` label
does not establish reusable computation: trusted host admission controls the
allowed effect class and the host prevents pure workers from emitting managed
writes or external requests.

The low-level `Executor::run_step` accepts admitted code and returns proposal
data for a trusted compositor; it has no provider dispatch capability. The shared
`WorkerBackend` path additionally requires a host-registered exact tuple of
code, profile, tenant, security scope, policy, fidelity and effect class. Its
request budgets must match that profile. This registration is a trusted host
decision and cannot be installed through guest output.

Upgrade the runtime or admission contract under a new profile identity. Retain
the old executable/profile to reproduce old runs; do not silently reinterpret
an existing checkpoint under a new engine. Equal outputs on test fixtures do not
establish hardware-independent timing, fuel accounting across versions or
universal floating-point equivalence. No memory copy-on-write performance
assumption is made across Windows and Linux.

Executed commands and platform results belong in the [progress record](progress.md).
The [primary-source record](research/stage5-sources.md) separates Wasmtime's
documented guarantees from this implementation's tested limits.
