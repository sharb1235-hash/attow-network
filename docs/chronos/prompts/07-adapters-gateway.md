# Stage 7 — Browser and SQL adapters with an enforceable effect boundary

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context: graph scheduling is operational. Add heterogeneous execution through a common contract. This stage demonstrates breadth across local browser, database and WASM without claiming that remote SaaS state can be cloned.

Files: crates/chronos-adapters/src/{lib,contract,sql,browser,gateway,provider_simulator,evidence}.rs; tests/adapter_conformance.rs; demos/workflow-studio/app (owned local fixture), browser/{package.json,package-lock.json,sidecar.mjs}; fixtures/original/{sql,browser,effects}; docs/{provider-contract,browser-fidelity,adapter-security}.md; browser CI lane.

Pins: existing rusqlite=0.40.2 bundled,backup; tokio=1.53.2 add process/io-util only if used; serde=1.0.229 derive; serde_json=1.0.151. Browser sidecar Node24.21.0 LTS and playwright=1.64.0 exact with npm lockfile/browser revision. Registry metadata was checked 2026-10-09; verify official archives/integrities at setup. Do not install browser globally or execute downloaded shell scripts. Kernel stays Rust; the optional browser bridge is an adapter process, not kernel code.

API:
```rust
pub enum Fidelity { RecordedReplay, OwnedEnvironment, Model, Live }
pub struct AdapterCapabilities { pub fidelity:Fidelity, pub snapshot:bool, pub restore:bool, pub isolated_remote_state:bool, pub idempotent_effects:bool }
pub trait Adapter {
    fn capabilities(&self) -> AdapterCapabilities;
    fn snapshot(&mut self) -> Result<ObjectId, AdapterError>;
    fn restore(&mut self, root:ObjectId) -> Result<(), AdapterError>;
    fn observe(&mut self, request:ObservationRequest) -> Result<Observation, AdapterError>;
}
pub trait EffectProvider {
    fn dispatch(&mut self, ticket:&DispatchTicket) -> Result<ProviderEvidence, ProviderError>;
    fn reconcile(&mut self, id:ObjectId) -> Result<ProviderEvidence, ProviderError>;
}
```
Use bounded length-prefixed sidecar messages with request ID and method allowlist. EffectProvider implementations are callable only through a gateway holding a valid durable ticket; speculative workers never hold purchase credentials.

Tests first: cookies/localStorage/IndexedDB/sessionStorage individually tested against supported adapter behavior; browser context separation; unsupported hidden JS stack restore rejected; service-worker and direct network bypass attempts blocked by declared containment or explicitly unsupported in fixture-only mode; SQLite backup includes WAL commits; SQL nondeterministic functions/unsupported writes rejected; five checkout branches produce one gateway intent; provider accepted-but-response-lost becomes Unknown then reconciles to Confirmed without new logical purchase; delayed NotFound cannot be treated as definitive rejection; expired idempotency retention blocks unsafe retry.

Steps:
1. Define typed adapter manifest with version, fidelity, supported storage, deterministic subset, effect semantics and limits. Unknown capabilities default unsupported. Store observations with provenance and capture time. Implement the WorkerBackend trait owned by chronos-state for speculative execution; keep EffectProvider/gateway separate so the scheduler cannot dispatch irreversible effects.
2. Implement owned SQLite snapshot via online backup for physical materialization, or managed logical root for the supported subset. Do not copy a live database main file. Track query dependencies conservatively; reject unsupported SQL rather than invent read sets.
3. Launch optional browser sidecar against only the owned loopback fixture by default. Snapshot supported storage into encrypted/owner-scoped references. Session storage requires explicit handling; DOMSnapshot is not stack checkpointing. Pin browser executable revision.
4. Build a provider simulator whose server owns inventory/holds, idempotency state and receipts independently from the client. Inject commit-before-response loss, duplicate requests, hold expiry, rejection and partial multi-provider outcome. The simulator is an original fixture, not a real airline integration.
5. Gateway verifies payload hash, domain, rights, provider contract and authorization before send. Keep durable ticket/state transitions atomic with ledger. No generic HTTP buffering that supplies invented real success to dependent computations.
6. Define response binding: modelled replies carry Model fidelity; actual receipt invalidates dependent graph nodes and reruns them before authoritative dependent transitions. Reconciliation is a separate authorized observation.
7. Document OS-level egress enforcement for any supported hostile browser deployment. If not implemented/tested on a platform, keep mode fixture-only and fail closed for live credentials. Cookie copying never implies independent remote accounts.

Licence/prior-art: retain Playwright/Chromium and SQLite notices; third-party fixtures need source commit, hash and licence. Browser automation, backups and compensation are prior art. No claim that an intercepted click is a distributed transaction.

Non-goals: real purchases, arbitrary websites, CAPTCHA evasion, universal browser snapshot, reversible payment or cloud credentials.

Definition of done: Rust and real owned-browser conformance run, simulator failure windows reconcile correctly, network scope is verified, and:
```sh
node --version
npm ci --prefix demos/workflow-studio/browser
npm --prefix demos/workflow-studio/browser exec -- playwright install chromium
cargo test --test adapter_conformance --features browser --locked
cargo test --workspace --all-targets --features adapters --locked
```

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```
