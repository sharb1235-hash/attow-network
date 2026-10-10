# Contributing to Attow Network

The project is under active staged development. Start with the [README](README.md), [architecture](docs/architecture.md), [progress record](docs/progress.md) and [working rules](AGENTS.md). Discuss changes to authority, encoding, effect semantics or supported profiles through an ADR before treating them as a stable contract.

Use Rust 1.95.0 selected by the repository toolchain. Keep changes focused, preserve existing work and add meaningful failing tests before implementation. Include regression cases that exercise the invariant rather than mirror implementation details. New dependencies need an exact reviewed version, a reason, license/provenance review, minimal features and the corresponding lockfile change.

Run each required gate separately:

```sh
cargo run -p xtask -- verify-repository
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```

Run the stage-specific and relevant feature checks as well. Describe the problem, resulting behavior, actual validation and remaining limitations in your change. Update documentation and progress when implementation status changes. Do not claim an unavailable platform was tested. Tests use owned fixtures and simulators, never real payment or purchasing services.

Contributions must be compatible with the project's MIT OR Apache-2.0 licensing. Preserve upstream notices and record copied fixtures or code in [third-party notices](THIRD_PARTY_NOTICES.md). Submit only material you have the right to contribute. Project conduct is governed by the [code of conduct](CODE_OF_CONDUCT.md); sensitive vulnerabilities follow the [security policy](SECURITY.md).

Repository publication, maintainer identities and external collaboration channels are owner configuration decisions. No registry publication or public outreach is authorized merely by the implementation plan.
