# Prior art, patent leads and naming

Focused research screen, 9 October 2026. This appendix distinguishes established mechanisms, recent research proposals and the product hypothesis. It is not a freedom-to-operate opinion, exhaustive patent search or trademark clearance. The current implementation is a small reference kernel; the proposed operating layer remains substantially broader.

The useful hypothesis is a coherent execution contract spanning agent state, browser/application adapters, speculative alternatives and mediated real-world effects. That ambition does not require claiming invention of snapshots, deterministic logs or distributed consensus. A stronger research question is whether a common branch/promotion interface can expose each backend's actual guarantees and retain useful alternatives without confusing simulated success with external completion.

## Closest research challenges

**State forks and deterministic reconciliation are established.** Microsoft's Concurrent Revisions project dates to 2010 and describes mutable snapshots with deterministic conflict resolution. Its associated semantics paper distinguishes determinacy from serializability. Consequently, a repeatable merge result does not by itself justify safe promotion of scarce-resource operations. Chronos must separately validate dependencies and authority. [Project](https://www.microsoft.com/en-us/research/project/concurrent-revisions/), [formal treatment](https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/semantics-revisions-2010.pdf)

**Checkpoint/fork correctness is already an explicit research topic.** Sajjad Khan's August 2026 preprint, *Resume Means Resume*, proposes a machine-checkable persistence contract covering continuation, effect behavior, fork determinism and recovery. It reports formal verification and framework experiments. Those are the author's reported results; they were not reproduced here. The relevant challenge is that a new runtime needs precise fork/resume semantics and adversarial tests, not merely a saved-state API. [Primary preprint](https://arxiv.org/abs/2608.03836)

**Peer-owned durable journals also have close contemporary work.** Luca Maraschi and Matteo Collina's September 2026 *State Without a Landlord* proposes authenticated workflow journals, writer epochs, quorum-finalized succession and explicit certificates. It distinguishes authentication from truth of recorded effects and identifies validation still required. It is an architecture proposal, not evidence that a production equivalent has shipped. It nevertheless prevents treating peer-to-peer durable workflow state as unexplored territory. [Primary preprint](https://arxiv.org/abs/2609.17645)

**Consensus and commit have different responsibilities.** Gray and Lamport's transaction-commit work provides an established foundation for reasoning about participating resource managers and durable decisions. Ordering a local log does not turn an unrelated booking endpoint into a transaction participant. Chronos must make that boundary visible in its API and product claims. [Consensus on Transaction Commit](https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/tr-2003-96.pdf)

## Concrete patent records

The following are patent publications and claim leads, not a chart of every independent claim. Assignees, dates and status labels are those reported by Google Patents when inspected; that service itself warns that its legal-status and ownership fields are not legal conclusions. Official prosecution, assignment and maintenance records require verification before reliance.

| Record and reported ownership | Priority and reported status | Specific claim mechanism inspected |
|---|---|---|
| [US7047441B1 — Recovery guarantees for general multi-tier applications](https://patents.google.com/patent/US7047441B1/en), Microsoft Technology Licensing | 2001-09-04; expired for fees, adjusted expiry 2023-06-21 | Claim 1 identifies active/deactivated components after failure, reincarnates active components and replays nondeterministic events. Claim 2 addresses recovery contracts and installation points. |
| [US8732670B1 — Ensuring determinism during programmatic replay in a virtual machine](https://patents.google.com/patent/US8732670B1/en), CA Inc | 2010-06-29; reported active, adjusted expiry 2033-01-16 | Claim 1 detects an impending native call, creates function/argument metadata, generates a native thunk and changes the VM binding to invoke it. |
| [US9122601B2 — Advancing and rewinding a replayed program execution](https://patents.google.com/patent/US9122601B2/en), CA Inc | 2006-06-07; expired for fees, anticipated expiry 2027-11-21 | Claim 1 combines recorded memory state points, extra markers for OS-resource API calls, replay mismatch detection and restrictions on skipping marker points. |

These mechanisms affect different implementation choices. A managed application-state ledger is not automatically native-function rebinding. A logical branch is not automatically a memory-level debugger that jumps through recorded state points. Conversely, expanding the OS layer into transparent runtime interception or process rewind would make those details much more relevant. The engineering team should preserve this distinction rather than infer safety from different product terminology.

For the Microsoft recovery record, the practical implication is historical: persistent components, nondeterministic-event replay and recovery contracts substantially predate this proposal. For the CA records, the next step is a claim comparison against any proposed interception/debugging adapter. Related records identified on the first CA page include US9606820B2 and US10585796B2; their claims were not independently charted in this screen. A status label for one member cannot resolve that unfinished family review.

No record above establishes that somebody owns every deterministic ledger, nor that Chronos is clear to implement every such ledger. The relevant comparison is between a specific implemented mechanism and every required element of an applicable claim. Publication age, an expired member, independent implementation and open-source licensing are not substitutes for that comparison.

## Exact name collision

The existing [spawn08/chronos](https://github.com/spawn08/chronos) project uses **ChronosOS** for its agent control plane. Its own architecture describes authentication, RBAC, approvals, tracing and runtime services. This is adjacent functionality, so the collision matters more than a similarly named unrelated project. [Project architecture](https://spawn08.github.io/chronos/reference/architecture/)

Its Go package is named `chronosos` at `github.com/spawn08/chronos/os`; the inspected package page currently displays version 0.15.0 published 25 September 2026. Package availability is evidence of use, not proof of trademark ownership or registration. Keep ChronosOS as an internal working title and resolve public naming separately. [Package record](https://pkg.go.dev/github.com/spawn08/chronos/os)

This screen did not complete official trademark-register searches, phonetic variants, domains, national business names or every software package registry. A replacement suggestion would therefore be an unverified candidate. The architecture can advance while those naming checks proceed.

## Claim discipline for the project

| Proposed claim | Assessment |
|---|---|
| Invented state forks or deterministic replay | Unsupported; direct historical research and patent publications exist. |
| First peer-owned durable workflow journal | Unsupported; close September 2026 research was found. |
| One local winner from competing branches | Demonstrated narrowly by the included reference tests under their stated assumptions. |
| Arbitrary external actions execute exactly once | Unsupported; requires specific receiver contracts and reconciliation semantics. |
| A common execution layer across heterogeneous adapters | Valid product/design hypothesis; full implementation and comparative evidence remain outstanding. |
| Meaningfully new patentable combination | Undetermined; no novelty, nonobviousness or freedom-to-operate opinion was performed. |

The practical contribution should be stated through observable behavior: what can be forked, what is shared, what a promotion validates, which authority enforces an effect, and what survives a failure. Demonstrating those boundaries across several real adapters would provide stronger evidence than renaming established primitives.

## Search limits and next investigation

This review covered selected primary research, three concrete US patent records, related-family leads and an exact product-name collision. It did not exhaust time-travel debugging, checkpoint migration, speculative execution, transactional memory, browser replay, capability security, durable workflow engines or authenticated-ledger patents. It did not establish current enforceability, geographic coverage or all continuation relationships. Unpublished applications and unindexed product work can also remain unseen.

Before selecting a transparent VM/browser interception design, create a mechanism-level description and have relevant claims compared against it. Before claiming a new software category, run a reproducible comparison against close workflow and state-fork systems using the same failure cases. Before publication, resolve naming and preserve dependency notices and implementation provenance. The included MIT/Apache license files govern this reference package; they do not supply third-party patent or trademark clearance.

The result is a credible engineering direction with identifiable predecessors, concrete unresolved questions and a small tested foundation. It is not evidence that the broader category, protocol or implementation is unprecedented.
