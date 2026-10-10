# Release runbook

This is a prospective release checklist. The foundation stage creates no public release, deployment or registry publication. Every package stays `publish = false`. Local development validation is recorded in [progress](progress.md); it is not evidence of an executed remote CI matrix.

## Before preparing a release

1. Obtain explicit authorization identifying repository, version, registries and distribution channels. Confirm repository/package ownership and use **Attow Network** in public names; the executable is `attow`. Naming selection alone is not a trademark clearance claim.
2. Review the implemented feature table, unsupported behaviors and changelog against tests. Do not publish placeholder targets as completed runtime features. Record supported OS/architecture combinations and unavailable lanes.
3. Confirm the exact Rust toolchain, direct dependency pins, committed lockfiles, action commit provenance, license obligations and third-party notices for the enabled release profiles. Inventory bundled native tools and fixtures as well as Rust dependencies.
4. Review authority activation, recovery, upgrade/rollback, provider credentials, peer defaults, browser confinement and export policy for each included capability. Secrets and real provider credentials do not belong in artifacts or tests.

## Local validation

Start from the intended revision and preserve all existing changes. Inspect the working tree rather than discarding it. Run each gate separately and stop on a failure:

```sh
cargo run -p xtask -- verify-repository
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```

Run stage-specific tests, implemented feature lanes, model checks, crash recovery tests and fixture integrations. Run dependency/license checks and bounded fuzzing using reviewed pinned tooling when those release lanes exist. Preserve actual output, tool versions and revision IDs. An unavailable check is an open release condition, not a pass.

## Artifact preparation

Build each claimed platform using an available native runner. Record compiler and linker versions, target, source revision, lockfile hash, enabled features and reproducible build procedure. Do not claim bit-for-bit reproducibility until independently compared builds establish it.

Include applicable license texts, third-party notices, documentation, checksums and a dependency inventory/SBOM. Verify archive contents and clean-machine help/version behavior. Exclude credentials, developer paths and private fixtures. Document schema compatibility and migration/rollback boundaries; an old binary must not silently mutate a newer authority store.

Prepare a concrete draft release with artifact hashes, evidence, limitations and recovery instructions for review. Configure the real private security reporting channel and maintenance policy before public distribution. Stage 1 does not provide a signing identity, artifact automation, SBOM generator or configured registry ownership; those require later implementation and owner setup.

## Publication and follow-up

Only after explicit authorization, enable publication for the reviewed packages, create the intended tag/release and distribute reviewed artifacts. Verify retrieved artifact checksums and installation behavior, then record exact locations and outcomes. A successful local build does not authorize a push or publication.

If validation or distribution fails, preserve the evidence and stop the affected release. Do not rewrite an already published artifact under the same version or erase an actual external effect from release/recovery history. Publish corrective versions through the authorized channel.
