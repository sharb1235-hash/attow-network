# Exact computation reuse

Attow Network shares completed pure evaluations only after the selected backend
validates the request and explicitly permits reuse. A caller-provided `Pure`
label is insufficient. A fresh browser observation, SQL operation, independent
model sample or external-effect proposal does not become pure because its
arguments resemble an earlier request.

## Complete identity

The initial `EvaluationKey` binds these exact fields:

| Fields | Why they matter |
| --- | --- |
| Tenant and security scope | Results may depend on data or capabilities unavailable to another caller. |
| Code and execution profile | Program bytes and admitted runtime/configuration affect meaning. |
| Whole input root and continuation | Hidden dependency projection is not assumed. |
| Recorded-observation root | A recorded reply differs from a new live observation. |
| Policy and backend fidelity | Authorization context and declared restoration semantics must match. |
| Independent sample identity | Equal prompts do not merge distinct stochastic samples. |
| Resource class and effect class | Backend admission controls the actual capability contract. |
| Fuel, memory, output and host-call budgets | Different limits can change the outcome or completion status. |

Only the attempt ID is omitted from `StepRequest` when forming the key. Branch
and graph-node identities are separate from the request and remain distinct
even when outputs are shared. A cached result never combines decision IDs,
provider operation IDs, rights, authority history or semantic effect slots.

`EvaluationKey::from_request` rejects non-pure classes and invalid budgets. The
scheduler additionally validates the real backend admission before consulting
the cache. The key itself contains no permission to execute or dispatch.

## Sharing a result

Concurrent eligible requests with the same full key can join one evaluation.
Each consumer retains its own node, branch and attempt binding. A successful
backend result must match the producing attempt/profile and pass repository
validation before its objects are retained or exposed to other consumers.
Completed results are eligible; suspension, traps, cancellation and infrastructure
failure are not cached as deterministic success.

A pure result must preserve the request's recorded-observation root, whether
reuse is enabled or disabled. Turning caching off does not relax that semantic
contract or authorize a fresh observation.

Cancelling one consumer must not corrupt work still needed by another consumer.
Likewise, an invalidated old result must not replace a newer attempt merely
because it finished later. Worker residency remains charged until the actual
blocking backend returns; cancelling a logical consumer is not evidence that a
guest, browser operation or provider request stopped.

The cache stores immutable output identities with validated reachable objects.
Sharing does not make the output's bytes independent of tenant/security context.
The compositor still owns application interpretation and the ledger alone owns
promotion and external-effect authorization.

## Receipt-driven invalidation

Replacing an assumed observation with a live receipt invalidates the affected
node and every transitive consumer. The initial algorithm deliberately follows
the whole dependency subtree. Replacement requires a quiescent run; active or
runnable work returns `Busy`. Each affected request then receives the new
observation root and a new attempt identity, and its old outcome is cleared.
Unrelated siblings retain their requests. Existing cache entries still refer to
their original complete keys. This can recompute more than necessary, but it
does not infer undeclared independence or replace a running attempt in place.

Both execution and replay bound cumulative replacement work to 1,000,000 per
run, charging the full graph's node and edge count for each replacement. The
scheduler rejects an exhausted replacement before changing the current requests
or accepted trace. The event, queue and retention limits remain separate bounds.

Clearing an outcome changes the current run view, not its recorded history.
Earlier request/outcome graphs stay pinned under distinct revision owners until
`forget`, remain charged against the retained-byte limit and survive cache
eviction. This preserves the immutable inputs needed to replay the old trace
events after garbage collection.

Projection to a smaller dependency set, symbolic equivalence, e-graph rewriting
and effect-indexed graph optimization remain experiments. This stage implements
exact memoization and conservative invalidation, not arbitrary website or SQL
program optimization. [Nominal Adapton](https://arxiv.org/abs/1503.07792),
[build-system research](https://www.microsoft.com/en-us/research/publication/build-systems-la-carte/)
and [equality saturation](https://arxiv.org/abs/2004.03082) provide established
prior art; they do not prove this implementation correct.

Generated differential tests compare shared and unshared execution on the tested
graphs. That evidence is bounded by its fixtures and generators, not a proof for
every backend. Offline trace replay consumes the recorded outcomes and verifies
their consistency; it is distinct from independently rerunning guest computation
or validating real provider truth. See [scheduler behavior](scheduler.md),
[performance methodology](performance-method.md) and the
[source record](research/stage6-sources.md).
