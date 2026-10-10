# Local authority model and crash evidence

`model/ledger.rs` is a small independent state machine. It explores a protocol
design; it does not execute the production ledger or prove arbitrary Rust,
SQLite, filesystem, hardware, or provider behavior.

## Explicit bounds and assumptions

- Two distinct requests propose branches competing for one semantic slot and one
  initially available right. Each request can be accepted at most once.
- One local authority serializes transactions. A pending transaction either
  commits all modeled durable fields or disappears in a crash.
- At most two crashes, two sends per operation, and one replay observation occur.
  A 100,000-state cap fails the test instead of silently truncating exploration.
- A dispatch claim must commit before its ticket can be used. Crash recovery
  changes durable `Dispatching` effects to `Unknown` and destroys volatile
  tickets and replies. `Unknown` cannot automatically resend or release rights.
- The simulated provider treats the same operation ID idempotently. It can
  perform an effect and lose its acknowledgement, definitively reject, or leave
  delivery unobserved. A terminal provider result cannot change on retry.
- Definitive evidence truthfully describes this simulated provider. Real
  provider authentication and dishonest verifier implementations are outside
  this model. Inconclusive evidence cannot release a reservation.
- Confirmation spends the right and retains the semantic slot. Definitive
  rejection and cancellation before dispatch release both. Duplicate requests
  and duplicate terminal replies do not alter the committed result.
- Replay is modeled as a pure observation with zero provider calls. Actual
  event reconstruction and index comparison require the implementation tests.

The search checks conservation of rights; at most one unresolved or confirmed
operation per slot; atomic request, managed-state, and outbox publication;
authorization before send; a durable dispatch record for every live ticket;
reservation retention while ambiguity remains; and no dispatch from replay.
Intentionally broken rule variants demonstrate that the checker rejects send
before commit, releasing an unknown reservation, replay dispatch, and duplicate
requests creating extra rights.

## Reproducible model run

Run `cargo +1.95.0 test --test ledger_model --locked -- --nocapture` from the
repository root. The first stub run failed all five tests; the implemented model
passed all five. Its completed breadth-first traversal reported:

```json
{"model":"two_branches_one_slot","states":3184,"transitions":10380,"duplicate_edges":4128,"lost_ack_edges":192,"unknown_states":976,"crash_states":608,"replay_states":1592}
```

States are unique complete model states, including bounded counters and volatile
state. Transitions count every labeled outgoing edge, including self-loops and
edges to already visited states. Coverage categories overlap. These are measured
exploration counts, not estimates or production throughput measurements.

## Real process-kill tests

`tests/storage_crash.rs` starts the integration-test executable as a separate
child process. The child supplies an explicit boundary observer to the ledger,
writes a marker when that boundary is reached, and parks. The parent waits for
the marker with a bounded watchdog, then calls `Child::kill()` and waits for the
process to exit unsuccessfully. A destructor sentinel must remain absent. The
ledger itself does not read failpoint environment variables. All databases and
markers live in temporary directories; no provider or network operation occurs.

The tests interrupt both sides of four commit boundaries:

| Boundary | Before commit | After commit |
| --- | --- | --- |
| Promotion | CAS, pins, root, request, event, outbox, and reservation roll back together | One durable result is returned on duplicate retry |
| Dispatch claim | Effect remains `Authorized` | Raw database is `Dispatching`; startup conservatively records `Unknown` even when no send occurred |
| Outcome | Claim remains durable; startup records `Unknown` | Confirmation evidence and spent right survive |
| Recovery | Durable `Dispatching` record remains available for another recovery attempt | One `Unknown` event survives and the next startup adds no duplicate recovery event |

Raw SQLite read transactions inspect the committed rows before normal authority
startup can recover them. Subsequent authority replay compares the event-derived
state and materialized indexes. These tests cover abrupt process death with the
host OS and SQLite still running. They do not simulate sudden machine power loss,
damaged storage hardware, or all possible instruction-level crash positions.

Run `cargo +1.95.0 test --test storage_crash --locked` to execute the boundary
suite. Test results must be taken from an actual run; the presence of a test case
or a passing independent model is not evidence that a boundary test passed.

The native Windows run on 2026-10-10 UTC (2026-10-09 local time) used
`cargo +1.95.0 test --test storage_crash --locked -- --nocapture` and passed all
10 tests in 11.84 seconds: eight ledger boundary cases, the process-kill
infrastructure smoke test, and the child entry helper (which does no work in the
parent test run). Every ledger boundary case actually reached its marker and was
killed. This is local Windows evidence; Linux and macOS results are not claimed.
`cargo +1.95.0 clippy -p attow-network --test ledger_model --test storage_crash --locked -- -D warnings`
also passed.
