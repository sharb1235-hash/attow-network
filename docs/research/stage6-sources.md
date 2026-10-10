# Stage 6 scheduling and reuse sources

Primary research sources reviewed 10 October 2026 UTC. The references establish
prior art and inform the design boundaries; no source implementation was copied.

| Source | Relevant claim and Attow consequence |
| --- | --- |
| [Hammer et al., Incremental Computation with Names / Nominal Adapton](https://arxiv.org/abs/1503.07792), [author-hosted paper](https://www.cs.tufts.edu/~jfoster/papers/nominal-adapton.pdf) | Names and dependency structure can support reuse across executions. The paper's from-scratch consistency result applies to its formal system; Attow instead tests its own bounded exact-key implementation against unshared execution. |
| [Mokhov, Mitchell and Peyton Jones, Build Systems à la Carte](https://www.microsoft.com/en-us/research/publication/build-systems-la-carte/) | Scheduling dependency order and deciding whether work must be rebuilt are separable concerns. Attow separates graph readiness and worker limits from complete-key cache eligibility. |
| [Willsey et al., egg: Fast and Extensible Equality Saturation](https://arxiv.org/abs/2004.03082), [author-hosted POPL paper](https://homes.cs.washington.edu/~cnandi/docs/popl21-cr.pdf) | Equality saturation uses equivalence representations and justified rewrites. Attow's initial scheduler has no e-graph engine or proof that arbitrary external actions commute or can be rewritten. |

Exact memoization, dependency DAGs, fair queues and single-flight evaluation are
established techniques. The proposed optimization of an effect-indexed worldline
graph remains experimental until a concrete transformation has independent
semantic evidence. Sharing immutable computation results never grants new
authority, merges independent samples or removes an unknown external outcome.

These sources do not support throughput, resident-agent counts or production
readiness claims for this project. Actual commands and raw benchmark evidence
belong in [progress](../progress.md) and
[performance methodology](../performance-method.md).
