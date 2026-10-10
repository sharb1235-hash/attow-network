# Original scenario definitions

The authoritative executable definitions are in
[`attow-cli`'s demo module](../../../crates/chronos-cli/src/demo.rs). WASM and SQL
fixture bytes are embedded in the installed binary. They use existing execution
ports; scenario code does not implement a second scheduler or authority reducer.

`data-change` selects a managed update from five alternatives. `checkout` selects
one simulator intent and demonstrates uncertain dispatch and reconciliation.
Optional browser execution uses the original owned application in `../app` and
the pinned sidecar in `../browser`; no arbitrary website or live account is used.

These scenario definitions are original project code under MIT OR Apache-2.0.
Any separately retained upstream runtime/notice material keeps its own attribution.
