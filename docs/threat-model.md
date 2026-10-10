# Threat model

This document defines requirements for the planned Attow Network runtime. Stage 1 supplies architecture and repository safeguards; it does not provide a sandbox, credential store, network service or hardened deployment. Consult the [progress record](progress.md) for implemented controls.

## Assets and assumptions

Protect authority credentials, tenant data, policy and rights, canonical object integrity, accepted ledger history, and the evidence needed to distinguish an actual effect from an unresolved attempt. The initial authority assumes a trusted local deployment and reliable SQLite transaction semantics on supported local storage. Administrative host compromise, Byzantine authority consensus, and universal provider correctness are outside the initial guarantee.

Untrusted inputs include guest programs, peer frames, object graphs, imported archives, browser pages, model responses and provider replies. None of these grant additional authority merely by containing an instruction or a signature. Signed content establishes origin only under the key's trust policy.

## Boundaries and planned controls

| Threat | Required control | Evidence required before claiming support |
| --- | --- | --- |
| Guest consumes unbounded resources | Admission limits, fuel, bounded hostcalls, memory/output budgets, fresh workers after traps | Exhaustion and interruption cases; incomplete attempts never cached as success |
| Peer uses malformed sizes or deep graphs | Check lengths before allocation; quotas, deadlines and canonical encoding | Negative parser tests and fuzzing with bounded memory |
| Imported object executes code implicitly | Separate validation, admission and execution; reject untrusted native artifacts | Import tests proving no execution or dispatch |
| Speculation duplicates purchase authority | Domain rights and semantic conflict slots outside rewindable branch state | Promotion model and competing-branch tests |
| Crash duplicates an external operation | Atomic intent/outbox transaction; durable dispatch state; stable provider identity | Separate-process crash tests at dispatch and persistence boundaries |
| Timeout permits a competing operation | `Unknown` retains conflict slot and rights until evidence resolves outcome | Ambiguous-send and reconciliation fixtures |
| Backup creates a second live writer | Read-only restore; explicit activation and fencing outside snapshot | Backup/import activation rejection tests |
| Partition produces two authorizers | Static writer policy; no timeout-triggered takeover | Peer partition tests |
| Replay sends a real mutation | Dispatch disabled by construction; recorded observations only | Offline replay tests and a rejecting dispatch implementation |
| Cache leaks data or alters sample semantics | Bind tenant, policy, profile and inputs; independent sample identities | Scope separation and hidden-dependency cases |
| Browser bypasses policy | Owned fixtures initially; declared fidelity; hardened egress and OS isolation later | Escape/egress tests for each claimed deployment profile |
| Logs or exports disclose credentials | Keep secrets outside graph payloads; redact capture and diagnostics; scoped export | Secret-canary tests and export review |
| Ledger rewrite or rollback appears authentic | Trusted signed checkpoints and explicit rollback/equivocation policy | Replay root verification plus independent checkpoint tests |
| Dependency or CI supply-chain compromise | Exact direct versions, committed lockfile, SHA-pinned actions, least privilege | Recorded pin provenance and review of each graph change |

Content hashes can reveal equality and enable guessing of low-entropy data. Future exports must consider tenant-scoped identifiers, encryption and retention policy; a hash is not anonymization. Relay operation and execution hosting have different trust requirements because an executing worker may see plaintext.

## Provider limits

Document each provider's idempotency scope, retention, payload matching, concurrency and status consistency. A transport timeout is ambiguous. An effect may have happened even if the process crashed before persisting its receipt. Compensation is a separate operation and may fail or remain unknown itself. Real purchases and payments are excluded from automated tests.

Security reports follow the [security policy](../SECURITY.md). Do not publish sensitive reproductions, credentials or personal data in public issue trackers.
