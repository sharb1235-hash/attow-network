# Stage 5 — Constrained WASM execution and portable continuations

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context: the kernel can persist state and promote proposals. Add a bounded compute backend with explicit step checkpoints. A checkpoint is data in the Worldline IR; it is not a promise to clone arbitrary live Wasmtime Stores.

Files: crates/chronos-sandbox/src/{lib,profile,admission,worker,host,continuation}.rs; wit/chronos-v1/{types,observation,effects,continuation,world}.wit; fixtures/original/wasm; sandbox tests; docs/{determinism-profile,continuation-abi}.md. Record profile hash in every evaluation.

Pins/features: wasmtime={version="=48.0.5",default-features=false,features=["std","runtime","cranelift","component-model","wat","anyhow","pooling-allocator"]}; existing codec/state; anyhow=1.0.104, thiserror=2.0.21, tempfile=3.27.0. Do not add wasmtime-wasi automatically. This compatibility lane was locally probed on Rust1.95.0. Wasmtime49.0.2 requires a newer compiler. Version48 TypedFunc::call performs post-return automatically; do not call the deprecated no-op post_return. A trapped Store may be unusable for subsequent component entry: discard it.

API contract:
```rust
pub struct ExecutionProfile { pub fuel:u64, pub memory_bytes:usize, pub output_bytes:usize, pub profile_hash:ObjectId }
pub struct StepInput { pub code:ObjectId, pub state:ObjectId, pub continuation:ObjectId, pub observation:Option<ObjectId> }
pub enum StepExit { Done { result:ObjectId, delta:ObjectId }, Suspended { continuation:ObjectId, request:ObjectId, delta:ObjectId } }
pub struct Executor { /* shared Engine/code cache; no shared guest state */ }
impl Executor {
    pub fn admit(&self, wasm:&[u8]) -> Result<AdmittedCode, SandboxError>;
    pub fn run_step(&self, code:&AdmittedCode, input:StepInput, profile:&ExecutionProfile) -> Result<StepExit, SandboxError>;
}
```
Define the WIT record/variant equivalents and a small bounded typed import/export boundary. If code validation needs wasmparser, first resolve the version already used by Wasmtime48 from Cargo.lock, pin that exact version, check licence and document it; do not guess a parser API or leave it floating.

Tests first: arithmetic component; explicit suspend/resume across a new process; sibling continuation isolation; unknown import denied; network/clock/random environment access absent; infinite loop fuel trap; oversized memory/output rejection; malformed WASM; hidden mutable-global reliance rejected by admitted contract or caught by fresh-step replay; recorded input replay hash equality; trapped worker discarded before next attempt. Add Windows and Linux fixtures without assuming memory_init_cow behavior matches.

Steps:
1. Define supported WASM/component subset and continuation schema. No in-flight import or unexported mutable program state may be needed after a checkpoint. Resume reconstructs code/state from explicit data.
2. Configure consume_fuel, canonical NaNs, SIMD/relaxed-SIMD exclusion and selected component features. Do not call wasm_threads(false) when the compile feature that exposes it is absent. Validate admitted binaries accordingly.
3. Fix/preallocate guest memory and bound tables/instances. StoreLimits memory_size is per memory; calculate aggregate worker limits separately. Bound host-call output and compilation resources. Emergency host termination yields incomplete attempt, never a committed semantic result.
4. Create a fresh Store for each step/attempt; share only immutable compiled code and Engine. Persist a suspended continuation before releasing resources. No generic Rust Future serialization.
5. Link only typed mediated state/observation/effect interfaces. Requests return data to the kernel; they do not get sockets or credentials. Supply time/randomness as explicit recorded values when requested.
6. Keep untrusted peers' compiled native artifacts out of unsafe deserialization. Compile validated WASM locally. Implement process isolation for hostile execution or honestly limit the first support level until that boundary is tested.
7. Document deterministic-profile scope, runtime upgrade replay policy and all measured overhead separately from fork metadata cost.

Licence/prior-art: Wasmtime uses Apache-2.0 WITH LLVM-exception; preserve notices. Continuations, deterministic replay and WASM isolation are established. Source comparison is research, not permission to copy incompatible implementation code.

Non-goals: arbitrary process checkpoint, browser clone, hardware-independent wall-clock determinism, unrestricted WASI and remote precompiled artifact loading.

Definition of done: real component and suspend/resume tests pass with no external access; exact commands:
```sh
cargo test -p chronos-sandbox --locked
cargo test --workspace --all-targets --features sandbox --locked
```

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```
