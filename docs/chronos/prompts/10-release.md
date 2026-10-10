# Stage 10 — Verifiable release candidate, signed artifacts and recorded launch demo

You are implementing ChronosOS, a working-title open-core worldline execution fabric in Rust. Applications submit alternative executions; the kernel shares immutable state and safe computations, schedules bounded workers, records observations, and promotes selected effects through an authoritative gateway. Existing agents, browsers, databases and workflow systems are backends. Preserve this broad abstraction.

Work only in the current repository. Read AGENTS.md and current architecture/ADRs/progress if present. The supplied research package may be under docs/chronos; this prompt contains the module contract and must also work without that package. Preserve unrelated user changes. Do not reset, clean or overwrite another workspace. Run stages sequentially. If a prerequisite is absent, create the minimal compatible prerequisite explicitly and document it; do not silently skip required behavior.

Global contract: Rust 1.95.0, edition 2024, MIT OR Apache-2.0 for original code, exact direct dependencies, committed Cargo.lock. Keep publish=false until an explicitly authorized release. Existing pins stay fixed unless compilation or an advisory requires an explained, verified change. No invented benchmarks, users, certifications, first-ever claims or patent clearance. Attribute research and copied material accurately; do not copy incompatible source. Treat ChronosOS as a name with a known adjacent-use conflict. Never contact a real purchase/payment provider during tests. No public publish, push, issue creation or messages are authorized by this prompt.

Write meaningful failing tests before implementation, then make them pass. Never satisfy a gate by deleting a test, weakening an invariant, ignoring an error or returning a canned success. Keep planned features visibly separate from implemented ones. Use typed errors, bounded allocation, explicit versions, no unsafe code in project crates and no secrets in logs. A failed infrastructure attempt is not a successful deterministic result.

At completion update docs/progress.md with changed files, exact commands, actual results, limitations and next stage. Execute each gate separately; stop on failure and repair it. Return a concise evidence report, not an unsupported declaration of production readiness.

Context: the integrated prototype and supported verification matrix are green. Produce a concrete release candidate with real artifacts, working installation, an actual recorded demonstration and an auditable launch checklist. Release-ready is earned by gates; the prompt does not authorize public publishing or invent signing credentials.

Files: .github/workflows/release.yml; dist configuration if used; docs/release-runbook.md; docs/launch-checklist.md; CHANGELOG; SECURITY/SUPPORT final placeholders inventory; release-toolchain.json; xtask release/sign/verify/demo-record modules; scripts/verify-artifacts.ps1 and .sh; release-candidate output manifest, platform archives, SHA256SUMS, SBOM, provenance, signatures and demo recording. Never commit private keys or embed signing tokens in logs.

Exact candidate tools: cargo-dist0.32.0, cargo-cyclonedx0.5.9, cargo-deny0.20.2, cargo-semver-checks0.51.0; Cosign3.1.3 from its official release. Node24.21.0/playwright1.64.0 can record the owned browser demo as WebM; alternatively produce a real terminal recording and its replayable local viewer without inventing screen frames. Resolve official tool binaries/checksums and record them; exact version is not sufficient without artifact integrity. Do not install tools irrelevant to the chosen release flow.

APIs/commands to implement in xtask:
```text
cargo run -p xtask -- release-plan
cargo run -p xtask -- release-build --target TARGET --output release-candidate
cargo run -p xtask -- release-verify --dir release-candidate --integrity-only
cargo run -p xtask -- release-verify --dir release-candidate --require-signatures
cargo run -p xtask -- demo-record --output release-candidate/demo
cargo run -p xtask -- release-sign --dir release-candidate --mode configured
```
Define a versioned ReleaseManifest containing source revision/dirty status, target triple, rustc verbose output, lock SHA256, runtime profile hash, file hashes, build checks, expected signing identity and supported-feature table. Artifact files must exist and hashes must match before inclusion. Freeze the manifest payload before signing; signature bundles are detached and excluded from its own file-hash list to avoid circular hashing. `release-verify --integrity-only` checks content/install evidence and clearly reports unsigned status; `--require-signatures` additionally requires authentic expected-identity signatures and is the only readiness gate. Missing signatures must fail the latter. Provision the pinned browser sidecar/browser before the all-features and recorded-demo gates; ordinary default offline checks remain independent of that optional installation.

Tests first: altered archive/checksum/signature rejected; verification with wrong signing identity fails; missing platform build cannot appear as supported; missing credentials cannot be reported as signed; archive path traversal denied; installed binary from archive performs offline demo/replay; source install matches documented CLI; demo recording is nonempty and corresponds to actual event log; source archive excludes secrets, build cache and local credentials.

Steps:
1. Freeze a release candidate version and inventory every incomplete guarantee. Confirm name/package availability before registry publication; keep working title if unresolved. Record no first-ever claim, users, certifications or performance without evidence. Do not silently downgrade a failed safety gate to a known issue.
2. Build each advertised target on a suitable native CI runner or validated cross toolchain. Record runners and compiler versions. Produce only artifacts actually built/tested. A passing local Windows build is not evidence of Linux/macOS support. Reproducibility checks compare independent builds where feasible; do not claim bitwise reproducibility from merely pinning dependencies.
3. Generate platform archives, checksums, source archive, SBOM from resolved dependencies, third-party notices and machine-readable provenance. Dist tooling is optional but configuration/output must be inspected rather than accepted blindly.
4. Install each candidate into a clean location, run --help, owned demo, offline replay and verification. Test shell and PowerShell install documentation. Do not advertise `cargo install chronosos` unless a real package has been published; source installation uses the local crates/chronos-cli path and --locked.
5. Record a real demonstration: objective -> shared branches across browser/SQL/WASM -> compare -> one selected intent -> simulated response loss -> Unknown -> provider reconciliation -> Confirmed -> offline replay. Label simulated providers and actual branch/worker counts onscreen. Include raw event log and video/terminal recording; no fabricated speedup captions.
6. Sign with a maintainer-authorized existing KMS/key or configured CI OIDC identity and pin expected verification identity/issuer. Official Cosign shape is `cosign sign-blob FILE --bundle FILE.sigstore.json`; verify with version-matched help and explicit trust identity. Do not silently use a generated throwaway key as maintainer identity or publish to transparency services without the release configuration's authorization. If credentials are absent, complete all other work, leave an explicit unsigned candidate and a pending signing gate. Do not mark the stage release-ready until configured real signing and verification pass.
7. Wire protected release CI with minimal permissions, pinned action commits, no privileged signing on untrusted pull requests, and immutable source-to-artifact traceability. Prepare draft release notes locally. Public publish/push remains a separate explicit action.
8. Complete launch checklist: supported platform results; install/recovery tests; security reporting; licence notices; name review; unresolved provider limitations; reproducible demo; authentic signatures; maintainer/recovery ownership; rollback and compatibility policy. Record actual failures and pending external configuration.

Licence/prior-art: include all dependencies' distribution obligations. MIT/Apache licensing does not establish freedom to operate or name availability. Candidate patent mechanisms require a claim-level search; no patent badges or legal-clearance language.

Non-goals: registry publication, repository push, customer outreach, fabricated signed artifacts, fake notarization/certification or promises of million-agent throughput without a real test.

Definition of done: signed, independently verifiable release artifacts for every advertised platform; working source/archive install; real recorded demo; complete launch checklist; all exact local gates plus real configured signing. Without credentials, report `candidate-complete; signing-pending`, not `release-ready`.
```sh
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo run -p xtask -- release-plan
cargo run -p xtask -- release-check --local
cargo run -p xtask -- demo-record --output release-candidate/demo
cargo run -p xtask -- release-verify --dir release-candidate --integrity-only
```
Run `release-build --target` for each actual release target and `release-sign --mode configured` only against the verified, frozen candidate. Run `cargo run -p xtask -- release-verify --dir release-candidate --require-signatures` after signing; a hash change invalidates the previous signature. Integrity-only success cannot complete the signing gate.

Common gates required in addition to stage-specific commands:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```
