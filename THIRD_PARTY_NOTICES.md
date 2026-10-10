# Third-party notices

Original Attow Network code is licensed under MIT OR Apache-2.0; see [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE). The MIT notice identifies Attow Network contributors. The Apache text is the standard Apache License 2.0. These project licenses do not replace a third party's applicable notices.

## Supplied research package

The user supplied the ChronosOS research and implementation package dated 9 October 2026 as the design input for this repository. Its original files, manifest and checksums are retained under `docs/chronos` with their original naming and provenance. The included reference kernel is separately licensed under its retained MIT and Apache texts, including its original ChronosOS contributor notice. It is a reference model, not evidence of implemented production capabilities in the Attow Network workspace.

Public documentation here summarizes the supplied design and records the user's public naming override. Original source snapshots and their checksums must not be rewritten merely to rename the project. The package's cited research is prior work, not copied implementation or a grant to reuse arbitrary source from those projects.

## Dependency and fixture policy

The initial foundation used the Rust standard library only. Stages 2–6 introduce
the following reviewed direct dependencies. Declared future workspace candidates
remain inactive until a consuming crate needs them. Review the actual lockfile
and enabled feature graph before distribution.

| Component | Exact version | Upstream license |
|---|---|---|
| BLAKE3 | 1.8.7 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception |
| minicbor | 2.3.0 | BlueOak-1.0.0 |
| rusqlite | 0.40.2 | MIT |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |
| tempfile (tests) | 3.27.0 | MIT OR Apache-2.0 |
| proptest (tests) | 1.11.0 | MIT OR Apache-2.0 |
| rpds | 1.2.1 | MIT |
| getrandom | 0.4.3 | MIT OR Apache-2.0 |
| Wasmtime | 48.0.5 | Apache-2.0 WITH LLVM-exception |
| wasmparser | 0.254.2 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wat (tests) | 1.261.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| anyhow | 1.0.104 | MIT OR Apache-2.0 |
| Tokio | 1.53.2 | MIT |
| serde / serde_json (benchmark tooling) | 1.0.229 / 1.0.151 | MIT OR Apache-2.0 |

The registry records and archive checksums are in
[Stage 2 dependency evidence](docs/research/stage2-dependencies.json). This
inventory is not a substitute for retaining the upstream license files in a
distributed artifact. Bundled SQLite and transitive dependencies will be included
in the resolved release SBOM and notices; there is no released artifact yet.
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

For copied fixtures or code, retain the original notice and document the exact upstream revision/path, original URL, SHA-256 and any extraction or modification steps. Do not infer permission from public visibility, a paper citation, a nearby project's license or a successful license scanner. No upstream real-schema corpus is claimed as bundled by this foundation document.
