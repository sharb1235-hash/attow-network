# Attow Network peer protocol v1

The public ALPN is `attow/1`. Protocol messages are data, not execution
instructions. A four-byte big-endian length prefixes a one-byte message tag and
a canonical, bounded payload. The length includes the tag. Unknown tags,
unsupported versions, trailing bytes and incomplete frames fail closed.

The protocol crate contains no sockets or ledger dependencies. The transport
crate authenticates Iroh endpoint identities, applies an explicit peer allowlist
and tenant/domain permissions, then invokes an application proposal handler.
Only that application can request a ledger promotion. A delivery acknowledgment
does not authorize a branch or confirm an external effect.

Control frames are limited to 1 MiB; object chunks to 64 KiB; a declared object
to 64 MiB. The receiver checks lengths before allocating body storage. Stricter
store and graph limits also apply: the protocol maximum is not a promise that
every application accepts an object of that size. Sessions bound concurrent
streams, buffered bytes and elapsed request time. Control and object transfers
use separate admission budgets so a bulk sender cannot consume every control
slot. Exhaustion is an explicit error, never unbounded queueing.

Object IDs bind kind, schema and canonical bytes. A receiver stages an entire
dependency closure, checks hashes, supported schemas and references, then commits
objects and root pins in one durable transaction. It sends a durable acknowledgment
after that commit. An interrupted transfer or a graph missing a dependency does
not make a partial branch available. Received code remains inert data.

A proposal binds request ID, tenant, domain, authority epoch, branch descriptor,
promotion payload, decision and generation. Retries use the same ID and complete
binding. The ledger returns the original durable outcome for an exact retry and
rejects reuse of an ID with changed content. A conflict is not a successful
promotion. `AcceptedPending` records an authorized intent; it is not a provider
confirmation. Lost provider replies remain `Unknown` until admissible evidence
resolves them.

Checkpoints sign a domain-separated canonical payload containing the tenant,
domain, designated writer, epoch, sequence, managed root and event head. Verify
each Ed25519 signature strictly against the configured writer and exact epoch.
Persist the accepted sequence floor together with its root and event head;
equal-sequence retries must match both. A larger epoch does not authorize itself.
Signatures provide attribution and integrity, not consensus or provider truth.

The initial transport uses Iroh 1.3.0's Minimal preset, explicit private/loopback
addresses and no public relay or discovery service. An initiator writes its Hello
before waiting for the authenticated server Hello. A fixed 32-byte proof derived
from the current TLS exporter follows the handshake, before the first operation.
The receiver checks that connection-bound proof before importing objects or
calling the proposal handler. A previous connection's proof cannot authorize
replayed early bytes. Iroh's TLS configuration supports early data internally;
the application does not accept an early-data mutation. See
[peer trust](peer-trust.md) and [handoff](authority-handoff.md).

The current service exchanges object graphs and proposal results. Signed
checkpoints have a portable codec and strict verification API; the service does
not automatically install a received checkpoint or advance trust from it.
