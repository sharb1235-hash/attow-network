# Changelog

## 0.1.0 candidate — signing pending

- Establish Attow Network's public package names and original MIT OR Apache-2.0 implementation, with the supplied ChronosOS design inputs preserved.
- Add canonical content-addressed storage, immutable branches, durable promotion, rights and effect lifecycle, verified ledger replay and explicit writer fencing.
- Add bounded Wasmtime component execution, durable continuations, conservative scheduler reuse and recorded-input replay without worker or dispatch ports.
- Add owned SQLite/browser adapters, a durable simulated provider with Unknown reconciliation, authenticated private peer transfer and data-only imports.
- Add the installed `attow` CLI, durable five-branch checkout and data-change scenarios, inspection, bounded portable bundles and recovery across interruption.
- Add native Windows candidate archives, resolved dependency inventory and notices, recorded demo tooling, installation evidence and detached-signature verification.

The native Windows candidate completed archive installation, recorded execution and independent integrity checks. Its status is `candidate-complete; signing-pending`; the exact source and artifact hashes are in [candidate evidence](docs/research/stage10-candidate.json). No maintainer signing credentials or registry publication are configured. Linux release qualification remains blocked by the unignored paste maintenance advisory; macOS source CI passed, while release-artifact qualification has not run.

This is an experimental 0.1.0 candidate, with no stable protocol, migration or production support promise. Provider guarantees apply to the owned simulator; browser network mediation is a fixture boundary, not an independent OS security sandbox. Exact implementation and test evidence are recorded in [progress](docs/progress.md), and pending launch conditions in the [launch checklist](docs/launch-checklist.md).
