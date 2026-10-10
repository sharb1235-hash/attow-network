# ChronosOS — build the operating layer for possible executions

Research and implementation package • 9 October 2026

**My recommendation is to make the worldline the unit of software execution.** A worldline contains a program's state, continuation, observations, dependencies and proposed effects. ChronosOS accepts a graph of alternatives, shares reusable work across that graph, materializes only the resources needed, and promotes selected outcomes through a durable authority.

That moves the product above agent frameworks, browser automation, database branches and workflow engines. Those systems become backends. The application developer works with `fork`, `observe`, `compare`, `promote` and `replay` across all of them. The ambition is to eliminate much of the custom orchestration, retry, reconciliation and duplicated execution that applications currently assemble themselves.

The research bet goes further than branching: **compile entire populations of possible executions.** Equivalent pure computations can share one evaluation while distinct histories, permissions and independent model samples remain separate. Changed observations invalidate the affected graph. Real effects pass through explicit authority boundaries. The specification calls this an effect-indexed graph optimization problem and gives concrete correctness obligations and experiments.

This is a broad product architecture, with software engineering, data operations, enterprise workflows, research and commerce as potential applications. Checkout is one difficult demonstration of its semantics. It is not the proposed market boundary.

## Read and use the package

| File | What it delivers |
|---|---|
| [Technical specification](01-technical-specification.md) | Category thesis; current research; mathematical model; Rust types; fork mechanics; storage; WASM; exact promotion protocol; effects; QUIC; security; scaling; open-core strategy |
| [Production repository blueprint](02-repository-blueprint.md) | Full target tree, pinned Cargo.toml, crate/API boundaries, feature wiring, fixtures, CI and release matrix |
| [Repository README](03-README.md) | Repository-ready README with ASCII architecture, actual reference commands and complete `parallel_checkout` source, plus clearly marked target installation |
| [Ten build prompts](04-codex-build-prompts.md) | Complete sequence from scaffold to signed release candidate; each stage includes context, APIs, exact dependencies, tests, algorithms, docs, non-goals and commands |
| [Autonomous execution instructions](05-autonomous-execution.md) | Working PowerShell setup/background scripts, real Codex flags, logs, resume checks and explicit release prerequisites |
| [Prior art and naming](06-prior-art-and-research.md) | Concrete research and patent leads, claim distinctions, search limits and the existing adjacent ChronosOS name use |
| [Reference kernel](reference-kernel/README.md) | Runnable Rust semantics model with 13 passing integration tests |
| [Runtime compatibility evidence](validation/dependencies.md) | Verified package pins and a separate compiled Wasmtime/SQLite/QUIC probe |

Individual self-contained prompts are in `prompts/`. The combined prompt document contains the same text. The execution scripts initialize a new empty target; they do not overwrite an existing project or launch an agent merely by being downloaded.

## What was actually built and checked

The reference kernel implements shared logical snapshots, isolated branch overlays, tracked reads, tombstones, conservative scan conflict detection, atomic local promotion, one pending intent per logical effect slot, authority identity and limited effect bookkeeping. It passed **13 tests**, formatting and warning-free Clippy on this Windows machine. Two independent SQLite connections competed for ten effect slots; each slot had one accepted winner and no loser writes. [Machine-readable reference record](validation/reference-results.json).

The five-branch example produced:

```text
branches=5 promoted=1 conflicts=4 pending_effects=1 external_calls=0
Local intent chosen. No external seat has been booked.
```

Run it from this package directory:

```sh
cargo test --manifest-path reference-kernel/Cargo.toml --locked
cargo run --manifest-path reference-kernel/Cargo.toml --locked --example parallel_checkout
```

The separate compatibility probe compiled and ran **Rust 1.95.0 / Wasmtime 48.0.5 / Iroh 1.3.0 / SQLite 3.53.2**, exercising constrained WASM and component calls, fuel exhaustion, SQLite transactions/backup, hashing/signatures, persistent-map independence and direct loopback QUIC. It used no public relay or discovery service. [Probe source and instructions](validation/runtime-probe/README.md).

These are functional results, not performance measurements. Full browser forking, production isolation, semantic ledger replay, provider integrations, distributed authority failover and million-agent operation have not been demonstrated by this package. The complete implementation is specified by the ten build stages. The reference has deliberate limitations, including no promotion-request deduplication and no complete effect-history replay; the production prompts explicitly add them.

## The launch claim to earn

The demonstration should show one objective becoming a graph of alternatives across a browser, database and program; shared work reducing actual computation; interruption and recovery; comparison of outcomes; and a selected effect moving visibly through pending, unknown and confirmed states.

That is a potentially compelling new product experience. Its headline and economic value depend on measured capability and adoption, not on declaring current infrastructure obsolete. The kernel should be open and independently useful; the commercial service sells hosted workers, reliable relay operations, fleet scheduling and enterprise administration. Correctness, portable export and self-hosting remain available in the open foundation.

The technical frontier is crowded. StateFork, Planarian, Shepherd, durable workflow systems and decades of transaction research establish substantial prior work. This package identifies a research direction and an implementation path; it does not establish unprecedented invention, patentability or freedom to operate. **ChronosOS is also already an adjacent product name**, so treat it as a working title.

No public repository, registry package, production service or signed production release was created. The final build prompt produces a release candidate and requires authentic signing and platform evidence before it can be called release-ready. All current files are local research, design and reference artifacts.
