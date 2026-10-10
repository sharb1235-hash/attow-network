# Worldline scheduler

The scheduler manages logical dependency graphs through the shared `WorkerBackend`
port. A graph descriptor is not a resident worker. The application installs a
backend registry keyed by resource class; the scheduler has no dependency on a
concrete browser, SQL adapter, network client or model API. It returns data and
proposals to the compositor. The ledger remains the sole effect authority.

## Graph admission

`WorldlineGraph` contains `GraphNode` values with a unique node ID, branch ID,
bounded `StepRequest` and dependency IDs. Dependencies identify producers that
must finish before a consumer becomes ready. Descriptor order is not execution
order. Validation rejects duplicate node IDs, missing or duplicate dependencies,
self edges, cycles, invalid request budgets and excessive graph structure.
Requests already bind their immutable inputs; a dependency edge orders work and
does not silently substitute its producer's output into another request.

The initial hard caps are 4,096 nodes, 16,384 total edges, 64 dependencies per
node, 64 direct consumers per node and depth 64 (a root has depth one). Size
preflight happens before building scheduler-owned indexes or cloning descriptors.
The conservative descriptor estimate charges 1,024 bytes per allocated node slot
plus 32 bytes per allocated dependency slot, including spare vector capacity,
separately from backend resident memory and retained results. Backing capacities
must also fit the corresponding graph limits, so oversized unused allocations
cannot bypass retention accounting when ownership transfers to the scheduler.
Backend validation supplies the actual code/profile/fidelity/security admission;
structural graph validation cannot grant it.

## Work, waiting and cancellation

Submission applies queue and retained-byte backpressure. Ready work is selected
fairly among tenants and runs under global and resource-class worker limits.
Blocking backend steps execute away from the async reactor. A suspended node
waiting for a receipt holds its logical state, not a busy execution slot.

Each scheduler instance admits at most 256 retained runs and 16,384 total
retained nodes.
The configured queue cap charges pending and sharing consumers as well as ready
work; dispatch releases a worker's queue slot. The global resident-worker cap
is at most 256, with separate resource-class caps. The retained-byte setting is
at most 4 GiB and charges both original and runtime graph descriptors, reserved
trace storage and repository-reported reachable object bytes. It is conservative
logical accounting, not a measurement of the process's resident memory.

Each trace reserves two events per admitted node and another two per affected
node plus one for each observation replacement, up to 16,384 events. This keeps
terminal outcomes representable after admission. The completed-result cache
holds at most 128 entries, evicts entries to make space and supports explicit
`clear_cache`. Run and cache pins have distinct owners in a randomly generated
scheduler-instance namespace. Applications must release them through `forget`
and `clear_cache`; dropping an in-memory handle is not durable pin cleanup.

Each run records node outcomes without merging branch histories. Eligible pure
nodes can share completed results or join one in-flight evaluation, according to
the [full reuse contract](reuse-contract.md). Unsupported or inadmissible requests
fail before a cache lookup. Traps, cancellation and infrastructure failures
never supply reusable completed results. A suspended outcome is a checkpoint,
not a reusable completed result.

Cancellation stops further scheduling for the consumer and releases its queue
ownership. It cannot recall an external effect, revoke a ledger
dispatch ticket, interrupt an arbitrary synchronous backend safely or erase
another consumer's interest in a shared evaluation. Resource permits remain
held while a real worker is running. The composing application must use the
ledger's [effect lifecycle](effect-lifecycle.md) for already authorized effects.
Cancelling releases queue ownership; retained report and pin metadata require
the run's explicit `forget` operation.

The coordinator and blocking workers continue independently of a particular
`drive` future. Dropping that future is not cancellation. A joined evaluation
that suspends supplies no reusable completed result: other consumers return to
the ready queue and execute their own attempts. Failed and cancelled producer
nodes cause their dependent pending nodes to be cancelled.

## Replacing observations

The invalidation helper validates the graph, rejects an unknown root ID and
returns that root plus its transitive consumers in deterministic dependency
order. It does not mutate the supplied graph. The initial engine permits
`replace_observation` only when the run is quiescent: completed nodes and nodes
parked behind a suspended dependency qualify; active, ready or otherwise runnable
work returns `Busy`. The caller must wait for that boundary before retrying.

Each run also has a cumulative replacement-work budget of 1,000,000, using the
same bound as replay. Each accepted replacement charges the whole graph's node
count plus edge count. Exhaustion returns `Limit("replacement work")` before
cloning the graph, evicting cache entries or changing the run or its trace.
Rejected replacements do not consume this budget. Trace, queue and retained-byte
limits still apply independently.

At that boundary the engine applies the replacement to every affected request,
advances its attempt identity and clears its current outcome. The derivation
binds the previous attempt, new observation root and a monotonically increasing
per-run revision starting at one. New work uses the new observation's cache key.
This conservative boundary prevents an old active reply from overwriting the
replacement; the initial API does not invalidate a running attempt in place.
Unrelated nodes retain their inputs and independent cache entries remain valid
for their original complete keys.

Earlier requests and outcomes remain part of the trace. Their reachable object
graphs keep distinct owners for each revision until `forget`, and their bytes
continue to count against the retained-byte limit. Replacement invalidates
current results without releasing the history needed for replay after garbage
collection. Cache eviction cannot release these independent run-history pins.

If repository cleanup fails during `forget`, the run becomes unavailable to
`drive` and further replacement. Calling `forget` again retries the remaining
owners without losing their retained-byte accounting. Failed cache eviction
likewise retains its owner and charge until cleanup succeeds.

## Semantic trace and replay

Accepted scheduling decisions, observation replacements and result identities
belong to the execution trace. Timings, worker occupancy and cache counters are
telemetry; they must not change semantic results. Replay consumes recorded inputs
and the trace without obtaining a backend or provider interface. It validates
recorded identities and transitions rather than contacting the outside world to
rediscover a missing observation.

The replay function is an in-memory trace reducer. It checks graph validity,
event order, attempt/profile bindings, object hashes and references, complete-key
result consistency and receipt invalidation. It consumes recorded outcomes; it
does not independently rerun guest code, authenticate a provider receipt or
attest that recorded external events really occurred. Pending nodes may remain
parked behind a suspended dependency, but a failed or cancelled dependency
requires cancellation of its pending descendants. Another suspended ancestor
does not excuse a missing cancellation event in the recorded trace.

`RecordedInputs` is an explicit corpus limited to 4,096 known objects, 16 MiB
and reference depth 64; traces are limited to 16,384 semantic events. Replay
enforces the same cumulative replacement-work limit as scheduler admission,
charging the complete graph's node and edge count for each replacement. This
bounds repeated invalidation traversals even when the recorded event count fits
its limit.

Scheduler traces remain in memory until the application exports them. The SQLite
facade makes retained request/outcome objects and their reachable graph pins
durable, but those pins do not themselves persist the full scheduling trace.
Keep both the trace and its required immutable objects for an offline replay.

The initial scheduler is bounded and process-local. It does not provide durable
distributed work stealing, global optimization, multi-writer effect authority or
automatic provider reconciliation. The [progress record](progress.md) states
which test, replay and benchmark commands actually ran. Performance samples
separate descriptor counts, executed evaluations, resident-worker peaks and
retained storage; a large logical graph is never reported as equally many
resident agents.
