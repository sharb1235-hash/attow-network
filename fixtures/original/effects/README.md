# Original durable provider fixture

`crates/chronos-adapters/src/provider_simulator.rs` implements the original
Attow Network loopback simulator under MIT OR Apache-2.0. It generates its own
SQLite inventory, holds, operation records and authenticated receipts in temporary
test directories. The test database and randomly generated simulator key are not
committed fixtures. No real provider response, schema, credential or provider
implementation was copied.

The executable cases live in the adapter's provider/gateway tests and
`tests/adapter_conformance.rs`. They create independent servers and exercise
duplicate operation bindings, commit-before-response loss, hidden lookups, hold
expiry, rejection, expired idempotency retention and partial provider outcomes.
All quantities are synthetic units and time is an explicit fixture tick counter.

The browser's predicted checkout is a separate `Model` observation. A simulated
provider receipt describes that simulator's durable result only. It is not a real
purchase or proof that an external service supports the same contract. The complete
behavior and bounds are in `docs/provider-contract.md`. Test execution evidence is
recorded in `docs/progress.md`, not inferred from the existence of these cases.
