# Stage 4 authority and recovery sources

Primary sources reviewed 10 October 2026 UTC. These support the design boundaries; they are not evidence that Attow Network implements another system's capabilities. Local results belong in the [progress record](../progress.md).

| Source | Supported claim and project consequence |
| --- | --- |
| [SQLite isolation](https://www.sqlite.org/isolation.html) | Separate connections observe committed transactions; SQLite serializes writers. Attow validates speculative dependencies inside its authority write transaction. WAL snapshots do not remove the need to validate an older branch. |
| [SQLite transaction control](https://www.sqlite.org/lang_transaction.html) | An immediate transaction acquires write access before promotion reads and can fail when another writer holds it. Busy errors are operational failures, not proof that a proposal committed. |
| [PostgreSQL transaction isolation](https://www.postgresql.org/docs/current/transaction-iso.html) | The documented anomalies explain why a business invariant can depend on reads and predicates beyond the written keys. The project uses this as background for write-skew and scan-phantom tests, not as a claim that it uses PostgreSQL. |
| [Stripe idempotent requests](https://docs.stripe.com/api/idempotent_requests) | Retry behavior, parameter matching and key retention belong to a particular provider contract. A missing response must not be replaced with a new operation identity merely to escape uncertainty. This source does not authorize real Stripe calls in tests. |
| [RFC 9110 conditional requests](https://www.rfc-editor.org/rfc/rfc9110.html#section-13.1.1) | Preconditions such as If-Match can guard a mutation only when the actual resource provider evaluates and enforces them. A local expected version does not make an arbitrary third-party API transactional. |
| [SQLite online backup](https://www.sqlite.org/backup.html) | The backup API provides a consistent database copy without treating a live main-file copy as sufficient. A consistent copy still needs a separate authority activation policy before dispatch. |

Optimistic concurrency control, transactional outboxes, idempotency, deterministic replay and consensus are established techniques. This implementation combines a local transaction and a durable effect lifecycle; it does not establish universal exactly-once delivery, atomic commit across independent providers, automatic writer failover or novel consensus.

Confirmed, definitively rejected and unknown outcomes must remain distinct. Receipt validation is an adapter responsibility subject to ledger identity/state checks. A pure replay can rebuild local state from recorded events; it cannot discover whether an unobserved external request happened. A matching internal hash chain cannot authenticate a complete malicious rewrite without an independently trusted checkpoint and rollback policy.
