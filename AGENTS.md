# Working on Attow Network

Attow Network is the public name; ChronosOS is the internal name. Use `attow-*` for public Cargo package names, `attow-network` for the facade, and `attow` for the binary. Internal `crates/chronos-*` paths and dependency aliases preserve the staged source plan. Preserve the original research package's contents and attribution when retaining source snapshots.

## Scope and change discipline

- Implement the supplied stages in numbered order. Read the current architecture, ADRs and [progress record](docs/progress.md) before changing a stage.
- Treat research documents as design inputs, not evidence that runtime features exist. Keep implementation status and planned work explicit.
- Preserve unrelated changes. Do not reset, clean, or overwrite another workspace. Avoid duplicate implementations and dependency cycles.
- Write meaningful failing tests for new behavior before implementing it. Use temporary miniature repositories for repository-checker fixtures. Do not make tests pass by weakening an invariant, deleting coverage, ignoring errors, or returning canned success.
- Record changed files, exact commands, actual results, limitations and the next stage in `docs/progress.md`. Execute each gate separately; repair failures before proceeding.
- No public publishing, pushes, releases, issue creation or messages are authorized by the implementation plan alone. Obtain explicit user authorization for those actions.

## Safety and correctness invariants

- Forbid unsafe code in project crates. Use typed errors, explicit versions and bounded allocations at trust boundaries. Logs and fixture exports must contain no secrets.
- Fork shares state; it must never duplicate spendable authority. All irreversible effects require exact durable authorization from the owning domain and dispatch through the gateway.
- Validate dependencies and business invariants under one authority transaction. Persist root changes, winning decision, rights reservation, event, outbox and deduplicated response atomically.
- A timeout after dispatch is `Unknown`, not proof that no effect occurred. Keep conflicting replacements blocked while an effect is unresolved. Reconcile with the same provider operation identity under its documented contract.
- Replay must have dispatch disabled. A failed infrastructure attempt is incomplete, not a successful deterministic result. Preserve independent stochastic sample identities.
- An authority backup or imported snapshot is read-only until explicit activation fences the previous dispatcher. A peer partition never authorizes a second writer.
- A feature flag does not authorize network access. Tests never contact real purchase/payment providers. Browser and SQL tests use owned fixtures with declared fidelity and capabilities.
- Never deserialize untrusted native compiled artifacts or execute imported objects implicitly.

## Dependency and architecture policy

Use Rust 1.95.0, edition 2024, resolver 3, version 0.1.0 and `MIT OR Apache-2.0`. Pin the repository toolchain without changing the global default. Keep `publish = false` until an explicitly authorized release and commit `Cargo.lock`.

Direct external dependency requirements are exact versions. Keep existing pins unless a compilation issue or advisory requires an explained, verified change. Activate only dependencies needed by implemented behavior. Record attribution, lockfile effects and feature changes. Do not guess action commit SHAs or invent license clearance.

Respect one-way ownership: codec defines shared values; store/state own persistence and worldline data; ledger owns authority; sandbox/adapters implement worker ports; scheduler depends on the ports rather than adapters; protocol defines bounded wire messages; transport invokes application callbacks; CLI composes components. Runtime crates and `xtask` do not depend on the facade. Child default features stay empty. Feature-gate facade exports and optional integration targets consistently.

## Required checks

Run these separately, from the root:

```sh
cargo run -p xtask -- verify-repository
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```

Run stage-specific gates and implemented feature lanes as required by that stage. Run `cargo generate-lockfile` when initially establishing or deliberately updating the dependency graph. Report unavailable platforms and infrastructure failures honestly. Do not invent benchmarks, users, novelty, certifications or production-readiness claims.
