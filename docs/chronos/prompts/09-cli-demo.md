# Stage 9 — Integrated CLI, inspectable demo and adversarial release gates

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context: all major modules exist. Connect them into a product a developer can install locally and understand in minutes. Demonstrate a common worldline spanning WASM, owned browser, SQLite and two peers, with truthful effect boundaries.

Files: crates/chronos-cli/src/{main,args,commands,display}.rs; root facade/tests; demos/workflow-studio/{README,expected,scenarios}; scripts/check.ps1/check.sh; docs/{getting-started,troubleshooting,operations,performance-method}.md; xtask command modules; CI feature matrix; README fully rewritten around actual implemented behavior.

Pins: use existing workspace dependencies; std CLI parser is sufficient. serde=1.0.229 and serde_json=1.0.151 for stable diagnostics; no new framework without verified exact pin. Browser Node24.21.0/playwright1.64.0 unchanged. Optional test tooling already researched: cargo-nextest0.9.148, cargo-deny0.20.2, cargo-llvm-cov0.9.1; install only needed tools using --locked and record actual tool versions.

CLI contract:
```text
chronos init PATH
chronos run SCENARIO --workers N --json
chronos branch list --json
chronos compare BRANCH_A BRANCH_B --json
chronos promote BRANCH --decision DECISION --generation N --request-id ID
chronos replay RUN --offline --json
chronos verify RUN --json
chronos doctor --json
chronos peer serve --bind 127.0.0.1:0
chronos export RUN --output PATH
chronos import PATH --data-only
```
Define `fn run(args: impl Iterator<Item=OsString>) -> Result<ExitCode,CliError>` or equivalent testable dispatch; output schema version1 and stable exit categories success/conflict/unknown/input/infrastructure. Avoid printing cookies/tokens in errors. Import never grants source authority.

Tests first: actual installed binary runs offline fixture from a directory containing spaces; clean-machine source install; malformed flags; export/import round trip without credentials; path traversal rejection; doctor detects missing optional browser; no-progress receipt remains visibly unknown; process interruption resumes same logical intent; CLI replay with network disabled produces same managed root; no unsafe force flag bypasses authorization.

Steps:
1. Compose crate APIs, not duplicate algorithms in CLI. Show Model/Recorded/Live evidence and pending/confirmed/unknown separately. Help and exit semantics must allow automation to distinguish uncertainty from failure.
2. Create multi-branch checkout and a non-commerce data-change scenario with original owned fixtures. Both exercise common graph semantics. Add server-simulated lost response to the checkout demonstration.
3. Build local inspector output: graph IDs, common nodes, changed values, dependencies, proposed effects and evidence links. Static HTML export is acceptable; no remote telemetry, invented charts or fabricated execution histories.
4. Implement xtask commands `verify-repository`, `demo --record-json PATH`, `release-check --local`, `install-smoke --root PATH` and `bench-report --output PATH`. Each returns nonzero on failed checks. Demo recording captures actual events and output, with declared fixture mode.
5. Run feature matrix: no-default-features, local, sandbox, scheduler, adapters, browser, peer and supported combinations. Provision Node24.21.0, `npm ci --prefix demos/workflow-studio/browser` and `npm --prefix demos/workflow-studio/browser exec -- playwright install chromium` before all-features or dedicated browser gates. Browser tests have a real pinned browser lane; missing prerequisites fail that lane rather than silently skipping. Default offline gates must not require the optional browser because the targets are feature-gated.
6. Run bounded hostile-input and crash suite; produce a verification matrix mapping every public guarantee to a test or an explicit unsupported status. Check policy against final Cargo.lock, preserving necessary transitive licence exceptions with reasons.
7. Rewrite installation, example and README against the real CLI. Verify commands in a clean temporary install root and record OS-specific prerequisite differences. Publish raw benchmark data only from executed measurements.

Licence/prior-art: every copied fixture records URL, immutable commit/version, SHA256 and licence. Original examples should be dual-licensed. Do not claim a novel optimization because the demo calls existing fork/replay APIs.

Non-goals: public website launch, GitHub push, live commerce, paid provider use, fake customer logos or certifications.

Definition of done: full local vertical slice works, supported CI lanes are configured, actual CLI install/demo/replay gates pass and unavailable platforms remain unchecked rather than advertised:
```sh
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo run -p xtask -- demo --record-json target/demo/events.json
cargo run -p xtask -- install-smoke --root target/install-smoke
cargo run -p xtask -- release-check --local
```

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```
