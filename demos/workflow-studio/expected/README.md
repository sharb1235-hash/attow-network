# Expected invariants, not canned results

The automated demo checks real schema-1 command responses. A successful run has
five branch descriptors and bounded worker counts; actual scheduler evaluation
counts, graph IDs and hashes come from its trace. Exact generated run/operation
IDs vary and must never be replaced with a prewritten recording.

Checkout's paused lost-response run returns exit 3 and Unknown. Resuming that run
keeps the same logical operation and intent, reconciles the simulator receipt and
returns Confirmed. Offline replay executes no backend/provider and retains the
recorded managed root. Data-change has no external purchase intent. Model browser
preview evidence remains distinguishable from the actual owned browser execution.

See the [verification matrix](../../../docs/verification-matrix.md) for test names
and the progress record for executed gate results. Generated JSON and terminal
recordings live in ignored build/release output directories.
