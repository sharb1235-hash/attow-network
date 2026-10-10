# Stage 5 execution sources

Primary documentation and the exact locally resolved crate sources were reviewed
10 October 2026 UTC. These support implementation choices; executed project
tests are recorded separately in [progress](../progress.md).

| Source | Supported claim and consequence |
| --- | --- |
| [Wasmtime deterministic execution](https://docs.wasmtime.dev/examples-deterministic-wasm-execution.html) | Deterministic imports, NaN canonicalization, excluding relaxed SIMD, fixed memory/table sizes and fuel-based interruption address documented sources of variability. Attow adopts this constrained approach. |
| [Wasmtime security](https://docs.wasmtime.dev/security.html) | Guest access to external resources comes through the linked host interface. The backend links only checked scalar data functions and no WASI implementation. Runtime sandboxing remains distinct from separately tested process containment. |
| [Wasmtime 48.0.5 Config](https://docs.rs/wasmtime/48.0.5/wasmtime/struct.Config.html) | Exact API availability and feature gates were checked in the downloaded 48.0.5 source. In particular, `wasm_threads` is unavailable without its Cargo feature; the configuration uses supported feature-bit controls instead. |
| [wasmparser 0.254.2](https://docs.rs/wasmparser/0.254.2/wasmparser/) | The parser version was selected from the resolved Wasmtime 48.0.5 dependency in `Cargo.lock`. Its local manifest records `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. Admission reads the actual version's section readers before compilation. |
| [WebAssembly component model WIT](https://github.com/WebAssembly/component-model/blob/main/design/mvp/WIT.md) | WIT describes the typed interface and portable record/variant forms. Attow's scalar execution functions are intentionally narrower than arbitrary list lifting. |

The two exact-version docs.rs pages could not be opened through the web lookup
tool during this review; claims about their APIs were verified against the
downloaded crates rather than an inferred latest API. Resolved versions, registry
checksums and license metadata are recorded in
[`stage5-dependencies.json`](stage5-dependencies.json). No third-party source was
copied into the original runtime or fixtures.

Explicit continuations, fuel metering, deterministic replay and WebAssembly
isolation are established techniques. This stage makes no novelty claim and no
claim of tested operating-system isolation, arbitrary process cloning, universal
cross-hardware timing determinism or access to a real provider.
