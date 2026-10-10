# Stage 1 — Repository, architecture and reproducible green baseline

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context and objective: Start from an empty repository or the initialized research package. Create a buildable workspace whose purpose is obvious immediately. This stage implements foundation and repository checks only. Do not claim the later runtime exists.

Files: root Cargo.toml/Cargo.lock/rust-toolchain.toml; src/lib.rs, src/core/mod.rs, src/sandbox/mod.rs; crates/{chronos-codec,chronos-store,chronos-state,chronos-ledger,chronos-sandbox,chronos-scheduler,chronos-adapters,chronos-protocol,chronos-transport,chronos-cli}/Cargo.toml and src/lib.rs or main.rs; xtask; AGENTS.md; README.md; docs/{architecture,progress,threat-model,release-runbook}.md; docs/adr/0001-worldlines.md through 0005-open-core.md; licenses, CONTRIBUTING, SECURITY, GOVERNANCE, SUPPORT, CODE_OF_CONDUCT, CHANGELOG, THIRD_PARTY_NOTICES; .github CI/issue/PR templates; scripts/check.ps1 and check.sh; .cargo/config.toml; deny.toml; .gitignore. Do not invent maintainer names, reporting email addresses or badge URLs. Create referenced example/model/benchmark targets if listed in manifests.

Dependencies: stage 1 can use std only; declare future exact workspace candidates but do not activate heavy unused dependencies: blake3=1.8.7, minicbor=2.3.0, rusqlite=0.40.2, rpds=1.2.1, wasmtime=48.0.5, iroh=1.3.0, tokio=1.53.2, thiserror=2.0.21, serde=1.0.229, serde_json=1.0.151, ed25519-dalek=3.0.0, getrandom=0.4.3, zeroize=1.9.1, tempfile=3.27.0, proptest=1.11.0. The blueprint may declare additional reviewed tools; inherit only used ones. Workspace resolver=3, version=0.1.0, publish=false. Pin Rust side by side; do not change the global default.

APIs: create an initial codec `pub struct ObjectId(pub [u8;32]);` and facade re-export; define CLI main returning std Result; define xtask `fn verify_repository(root: &Path) -> Result<(), Box<dyn Error>>`. Avoid dummy public APIs that pretend to execute work. Unimplemented CLI commands must fail with an explicit unsupported message.

Create root features exactly: default=["local"]; local=["dep:chronos-store","dep:chronos-ledger"]; sandbox=["dep:chronos-sandbox"]; scheduler=["sandbox","dep:chronos-scheduler"]; adapters=["local","dep:chronos-adapters"]; browser=["adapters","chronos-adapters/browser"]; peer=["dep:chronos-protocol","dep:chronos-transport"]. Codec/state are unconditional facade dependencies, other named crates optional. Child default features are empty; adapters declares browser. Feature-gate facade re-exports. Register ledger_model and worldline_checkout with required-features=["local"]; later sandbox/scheduler/browser/peer integration targets must use matching required-features so default gates compile and execute only their declared subset. A feature is not permission to connect to the internet.

Tests first: repository checker must detect a missing license, floating direct dependency, missing manifest target, accidentally enabled publish and broken internal docs link. Fixtures for the checker are temporary miniature repositories, not mutation of the working repository. Verify CLI help truthfully reports development status.

Numbered implementation steps:
1. Inspect files and toolchain. Create AGENTS.md with invariants, test commands, change discipline, authority/effect rules, dependency policy and no public publishing. Establish the ten-stage progress checklist.
2. Describe Worldline IR, explicit continuation boundary, effect classes, local serializable authority, UNKNOWN recovery, per-domain peer model and OSS/paid boundary. ADRs must explain alternatives and why arbitrary browser state cloning/universal external atomicity are excluded from current guarantees.
3. Create the workspace and root facade. Each child inherits metadata and only needed dependencies; one-way dependencies go from codec to store/state, ledger, sandbox/scheduler/adapters, protocol/transport, CLI composition. Avoid cycles and duplicate implementations. The CLI package must declare `[[bin]] name="chronos", path="src/main.rs"`; package and executable names differ intentionally.
4. Write standard license texts and community files with honest placeholders only where owner decisions are genuinely required. Security reporting directs to a maintainer-configured private channel, not a fabricated address.
5. Write README with one-sentence purpose, ASCII architecture, implementation-status table, source cargo commands and clearly labelled planned checkout example. No live badges without CI.
6. Build checks and CI for Linux x86_64, Windows x86_64 and macOS arm64, using native runners only when available. Resolve official action releases to full commit SHAs through verified upstream metadata and record them in docs/research/ci-pins.json; do not guess SHAs. Pin toolchain exactly; workflow permissions default read-only. CI performs fmt, tests, clippy and docs, with feature lanes added as implemented.
7. Generate Cargo.lock, run the checker and all gates, then record actual local platform and unavailable remote CI execution honestly.

Non-goals: WASM engine, networking, actual adapters, sales copy claiming a shipped OS, deployment or remote repository creation. Architecture breadth must remain, with unsupported runtime behavior clearly marked.

Definition of done: all workspace targets compile, checker negative fixtures pass, documentation accurately separates implemented/planned, and these commands pass:
```sh
cargo generate-lockfile
cargo run -p xtask -- verify-repository
```

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```
