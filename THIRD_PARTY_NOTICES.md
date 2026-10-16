# Third-party notices

Original Attow Network code is licensed under MIT OR Apache-2.0; see [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE). The MIT notice identifies Attow Network contributors. The Apache text is the standard Apache License 2.0. These project licenses do not replace a third party's applicable notices.

## Supplied research package

The user supplied the ChronosOS research and implementation package dated 9 October 2026 as the design input for this repository. Its original files, manifest and checksums are retained under `docs/chronos` with their original naming and provenance. The included reference kernel is separately licensed under its retained MIT and Apache texts, including its original ChronosOS contributor notice. It is a reference model, not evidence of implemented production capabilities in the Attow Network workspace.

Public documentation here summarizes the supplied design and records the user's public naming override. Original source snapshots and their checksums must not be rewritten merely to rename the project. The package's cited research is prior work, not copied implementation or a grant to reuse arbitrary source from those projects.

## Dependency and fixture policy

The initial foundation used the Rust standard library only. Stages 2–10 introduce
the following reviewed direct dependencies. Declared future workspace candidates
remain inactive until a consuming crate needs them. Review the actual lockfile
and enabled feature graph before distribution.

| Component | Exact version | Upstream license |
|---|---|---|
| BLAKE3 | 1.8.7 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception |
| minicbor | 2.3.0 | BlueOak-1.0.0 |
| rusqlite | 0.40.2 | MIT |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |
| tempfile (adapter runtime and tests) | 3.27.0 | MIT OR Apache-2.0 |
| proptest (tests) | 1.11.0 | MIT OR Apache-2.0 |
| rpds | 1.2.1 | MIT |
| getrandom | 0.4.3 | MIT OR Apache-2.0 |
| Wasmtime | 48.0.6 | Apache-2.0 WITH LLVM-exception |
| wasmparser | 0.254.2 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wat (embedded demo parser and tests) | 1.261.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| anyhow | 1.0.104 | MIT OR Apache-2.0 |
| Tokio | 1.53.2 | MIT |
| serde / serde_json (runtime and benchmark tooling) | 1.0.229 / 1.0.151 | MIT OR Apache-2.0 |
| Iroh | 1.3.0 | MIT OR Apache-2.0; retained BSD contribution notice |
| ed25519-dalek | 3.0.0 | BSD-3-Clause |
| SHA-2 (release integrity tooling) | 0.10.9 | MIT OR Apache-2.0 |

The registry records and archive checksums are in
[Stage 2 dependency evidence](docs/research/stage2-dependencies.json). This
inventory is not a substitute for retaining the upstream license files in a
distributed artifact. Candidate production generates a resolved CycloneDX SBOM,
an all-source inventory and exact nested dependency notices. There is no registry
publication implied by producing a local candidate.
The persistent-map dependency is recorded separately in
[Stage 3 dependency evidence](docs/research/stage3-dependencies.json).
Authority identity randomness is recorded in
[Stage 4 dependency evidence](docs/research/stage4-dependencies.json).
The WASM runtime, its matching binary parser, test WAT parser and error library
are recorded in [Stage 5 dependency evidence](docs/research/stage5-dependencies.json).
The parser version follows Wasmtime's resolved dependency, while the newer WAT
tooling has its own separate parser version in the lockfile.
Scheduler and benchmark dependencies are recorded in
[Stage 6 dependency evidence](docs/research/stage6-dependencies.json).
Adapter feature changes and newly resolved Rust transitives are recorded in
[Stage 7 Rust dependency evidence](docs/research/stage7-dependencies.json),
including the complete registry license alternatives for mio, signal-hook-registry
and wasi. Existing direct dependency versions remain pinned.

Transport and cryptographic additions are recorded in
[Stage 8 dependency evidence](docs/research/stage8-dependencies.json) and
[retained transport notices](docs/notices/stage8/README.md). The
[release recovery inventory](docs/notices/stage10/README.md) documents missing
upstream workspace license files, immutable source recovery and explicit
declaration-only exceptions. The generator verifies cached registry archives
against the lockfile and retained notice bytes against the exact archive members.
The [dependency policy](docs/dependency-policy.md) records permitted expressions
and the unresolved Linux-only advisory; a Windows candidate is not evidence that
another target satisfies its safety gates.

## Optional Stage 7 browser runtime

The original owned page and sidecar use Node 24.21.0, Playwright and
playwright-core 1.64.0, and Chromium 156.0.8078.4 (Playwright revision 1248).
Playwright is licensed under Apache-2.0, with its upstream Microsoft/Puppeteer
attribution retained. Node and Chromium include additional component licenses;
they are not covered solely by the project's licenses or Playwright's license.

Unmodified Node, Playwright/core and Chromium headless-shell notices, including
the Playwright bundle notices and complete Chromium aggregate, are retained in
[the Stage 7 notice inventory](docs/notices/stage7/README.md). The
[Stage 7 provenance record](docs/research/stage7-browser-provenance.json) records
their original sources and hashes, the Node archive digest, npm integrity values
and the official Playwright source commit/browser revision. The runtime binaries
are installed locally rather than committed as fixture data.

SQLite is used through the existing bundled rusqlite dependency for owned database
backup and provider durability. Its authors dedicate the deliverable library and
documentation to the public domain; build tooling can have separate terms.
[SQLite's upstream copyright statement](https://sqlite.org/copyright.html)

The browser page, sidecar and provider scenarios are original project fixtures;
they contain no copied third-party website or real provider response. Browser
automation, online database backup and reconciliation are prior techniques, not
novelty or patent-clearance claims.

For copied fixtures or code, retain the original notice and document the exact upstream revision/path, original URL, SHA-256 and any extraction or modification steps. Do not infer permission from public visibility, a paper citation, a nearby project's license or a successful license scanner. No upstream real-schema corpus is bundled.
