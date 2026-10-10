# Attow Network

Explore alternative executions, share immutable state and safe computations, and
promote selected effects through one accountable authority per domain.

Attow Network is an open-source Rust prototype with a working local CLI. It joins
bounded WASM workers, SQLite snapshots, optional owned Chromium sessions and two
private authenticated peers in an inspectable workflow. A durable gateway sends
only authorized intents to the owned provider simulator. A lost response stays
**Unknown** until reconciliation supplies evidence.

## Try the local workflow

Install Rust **1.95.0** and the native C/C++ build tools, then run from this checkout:

```sh
cargo +1.95.0 install --path crates/chronos-cli --locked --root target/install
```

Add `target/install/bin` to PATH, or invoke its absolute `attow` path (`attow.exe`
on Windows). Run from a new directory, including paths with spaces:

```sh
attow init "my workflow"
cd "my workflow"
attow run data-change --workers 2 --json
attow branch list --json
attow replay RUN_ID --offline --json
attow verify RUN_ID --json
```

Replace `RUN_ID` with `data.run_id` from the actual run output. The non-commerce
scenario evaluates five alternatives using real WASM and SQL workers, exchanges
immutable data over two private peers and promotes the selected managed change.
The checkout scenario adds a simulated external effect:

```sh
attow run checkout --workers 2 --pause-after-unknown --json
attow run checkout --workers 2 --resume RUN_ID --json
```

The first command intentionally exits **3** after a committed simulator response
is lost. The second reconciles the same logical operation and reruns receipt-bound
work before promoting a confirmed response. Other exit categories are 0 success,
2 conflict, 4 invalid input and 5 infrastructure failure. JSON envelopes use schema 1.

See [getting started](docs/getting-started.md) for complete source installation,
optional browser provisioning and PowerShell paths. Default installation needs no
Node or browser. Windows x64 is the locally validated platform; Linux/macOS jobs
are configured but not advertised as tested release artifacts.

## Inspect and exchange evidence

`attow compare BRANCH_A BRANCH_B --json` reports actual branch differences. The
local inspector shows graph identities, shared nodes, dependencies, proposed
effects and evidence. Run records distinguish Model observations from actual
owned-environment browser/SQL execution and confirmed/unknown provider outcomes.

```sh
attow export RUN_ID --output run.attow
attow import /absolute/path/to/run.attow --data-only
```

Import in another initialized workspace. Bundles contain bounded immutable data;
they confer no authority or credentials. Offline replay checks recorded execution
without running workers, imported code or provider calls. Native verification also
rebuilds the local authority history. Portable verification checks consistency of
the exported evidence and does not attest to an external provider's truth.

## What is implemented

| Area | Implementation |
| --- | --- |
| Portable state | Versioned canonical objects, BLAKE3 identity, shared worldline roots, tracked reads/writes/scans |
| Durable storage | SQLite transactions, verified graphs, retention pins, collection and backups |
| Authority | Atomic promotion, decisions, reserved rights, idempotent requests, durable outbox and crash recovery |
| Execution | Admitted WASM components, explicit continuations, bounded scheduling and exact pure reuse |
| Adapters | Owned SQLite and optional pinned Chromium fixtures; provider gateway and independent loopback simulator |
| Peers | Private Iroh/noq endpoints, scoped durable graph transfer, authenticated proposals and external fencing port |
| Developer tools | Installed CLI, static inspector, data-only bundles, offline replay, actual demo/install/release checks |

See the [verification matrix](docs/verification-matrix.md) and
[stage evidence](docs/progress.md). The initial release candidate is prepared in
Stage 10; signed distribution readiness requires an authentic configured identity.
Packages remain `publish = false`; no registry install command is promised.

## Boundaries

Logical forks share data; they do not duplicate spendable authority. Browser
restoration covers declared owned storage, not arbitrary process state. The
in-process WASM runtime and browser interception do not constitute an OS security
sandbox. The demo fence excludes processes on one host; production cross-host
fencing is an external integration requirement. There is no automatic multiwriter
failover, universal provider rollback or atomic purchase/payment transaction.

No real purchase or payment provider is contacted by the demonstrations. No
throughput, novelty, patent clearance, customer or production-readiness claim is
inferred from passing local tests. [Performance methodology](docs/performance-method.md)
keeps measured raw samples separate from demo event counts.

## Develop

```sh
cargo +1.95.0 run -p xtask --locked -- verify-repository
cargo +1.95.0 test --workspace --all-targets --locked
cargo +1.95.0 run -p xtask --locked -- demo --record-json target/demo/events.json
cargo +1.95.0 run -p xtask --locked -- install-smoke --root target/install-smoke
cargo +1.95.0 run -p xtask --locked -- release-check --local
```

The demo and release-check commands require the pinned optional browser
runtime and policy tool described in the [workflow demo](demos/workflow-studio/README.md)
and [dependency policy](docs/dependency-policy.md). See
[architecture](docs/architecture.md), [operations](docs/operations.md),
[troubleshooting](docs/troubleshooting.md), [security](SECURITY.md) and
[contributing](CONTRIBUTING.md).

Original code is [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE).
[Third-party notices](THIRD_PARTY_NOTICES.md) retain separate obligations.
ChronosOS is the internal name; public packages are `attow-*` and the executable
is `attow`. Internal `crates/chronos-*` paths and the preserved research package
retain original names for traceability.
