# Owned workflow studio

Two CLI scenarios use the same worldline and scheduling contract:

- `data-change` evaluates five alternatives with actual admitted WASM components
  and an owned SQLite worker, transfers the selected immutable graph through two
  private authenticated peers and commits one managed change.
- `checkout` adds a proposed simulator purchase, one durable intent, a deliberately
  lost response, visible Unknown state, reconciliation and receipt-dependent WASM
  work before a confirmed response can promote.

The optional `--browser` mode adds an actual pinned Chromium worker against owned
loopback pages. Checkout preview evidence remains Model; actual browser activity
does not turn a simulated prediction into a confirmed provider receipt.

Provision the [documented browser runtime](../../docs/getting-started.md) and set
`ATTOW_NODE`, `ATTOW_BROWSER_SIDECAR` and `PLAYWRIGHT_BROWSERS_PATH` to absolute paths.
From the repository root:

```sh
cargo +1.95.0 run -p xtask --locked -- demo --record-json target/demo/events.json
```

The command builds the browser-enabled CLI and records actual command arguments,
exit codes and schema-1 envelopes. Checkout's paused step must return 3; resume
must return 0. Both scenarios then undergo offline replay and verification. An
unexpected failure stops recording with a failing exit code. The recorded output
declares `fixture_mode: "owned"`; it is not benchmark evidence.

The installed default CLI runs both scenarios without the optional browser:

```sh
attow init "demo workspace"
cd "demo workspace"
attow run checkout --workers 2 --pause-after-prepared --json
attow branch list --json
attow compare FIRST_BRANCH_ID SECOND_BRANCH_ID --json
attow run checkout --resume RUN_ID --workers 2 --pause-after-unknown --json
attow run checkout --resume RUN_ID --workers 2 --json
```

Use the actual returned run ID. Run records and the local inspector expose real
branch IDs, shared graph nodes, changed values, dependencies, proposed effects,
trace roots and evidence. Numeric timings and hashes are taken from execution;
there is no expected canned event history or speedup claim.

The prepared pause persists the immutable graph before any promotion or provider
dispatch. It supports inspection before choosing to resume. The two pause flags
are mutually exclusive. The Stage 10 terminal recorder follows this sequence with
the extracted release binary and actual optional browser, then performs offline
replay and a separate data-change scenario. See the
[release runbook](../../docs/release-runbook.md) for its artifact and trust gates.

Source installation is separately exercised from a path containing spaces:

```sh
cargo +1.95.0 run -p xtask --locked -- install-smoke --root target/install-smoke
```

This smoke command deliberately installs a debug binary to reuse local compilation
artifacts; the report labels that profile. Stage 10 candidate archive installation
checks the actual release binary separately.
