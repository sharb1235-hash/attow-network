# Adapter security boundary

Stage 7 supports owned local fixtures and trusted host code. It does not provide
an OS sandbox for a hostile browser, malicious native adapter, database parser
exploit or user with access to the host's files. Live browser mode and credentials
are rejected. No real purchase/payment provider is contacted by the tests.

## Separate workers from authority

SQL and browser adapters implement the state crate's `WorkerBackend` port. A
worker receives immutable object identities and a validated binding, not a gateway
permit. Unknown capability flags default to false; unsupported inputs fail with
typed errors. Tenant, security scope, policy, fidelity, code, profile, resource
class, effect class and the exact budget must match. Stochastic sample identities
are unsupported by these adapters; relabelling an irreversible effect as work is
rejected.

Only the gateway constructs `DispatchAuthorization`, after checking the provider
contract/preflight and durably claiming and revalidating the exact ledger ticket
and body. Its private fields and authority borrow enforce this interface in safe
Rust. This is not a security boundary against arbitrary native code that can open
sockets or modify the authority database. Provider implementations and their
evidence verifiers are trusted contract code. The simulator's private MAC key is
a fixture credential shared with its client, not a public-key trust anchor.
See the [provider contract](provider-contract.md) for ambiguity and retention rules.

## Owned browser containment

The sidecar accepts only versioned bounded commands against included original
assets. It starts a fresh context for restore, blocks service workers, closes
WebSockets and routes requests to a fixed owned-origin allowlist. CSP adds another
page-level restriction. The Rust launcher clears inherited environment variables
and passes only runtime path variables; Node injection options and provider
credentials are not forwarded. Source-byte and runtime-version checks fail closed.

These are application controls, **not OS-level egress enforcement**. There is no
implemented Windows firewall policy, Linux network namespace, macOS network filter
or equivalent hostile-browser containment in this stage. Browser background
traffic is not covered by a universal no-egress claim. The negative probes use a
second owned loopback server; passing them establishes those probes' behavior only.
The supported mode therefore remains fixture-only, without live credentials or
arbitrary navigation. Native browser memory and CPU are not governed by the logical
worker buffer budget. [Playwright routing guidance](https://playwright.dev/docs/network)

Browser snapshot objects validate tenant, scope and profile before restoration.
SQL snapshots validate their owning scope. These are SDK access checks, not
filesystem encryption. Anyone who can read the CAS database or its backups may
read stored fixture data. Protect those files at the host boundary; do not import
live authentication state. No sensitive snapshot or simulator database is a
committed fixture.

## Owned SQLite subset

`SqlAdapter` opens an explicitly supplied owned database. Snapshot and restore use
SQLite's online backup API, including committed WAL state, rather than copying a
live main file. A snapshot is limited to 512 KiB and backup work has a bounded
deadline and step count. Materialization uses a temporary database with conservative
SQLite limits, disabled trusted schema and integrity checking. The SQLite backup
API is an existing database primitive. [SQLite online backup](https://sqlite.org/backup.html)

The admitted query grammar is `SELECT a,b FROM table ORDER BY a,b`: uppercase
keywords, simple ASCII identifiers, at most sixteen distinct columns, and ordering
by every selected column. The implementation additionally checks ordinary table
metadata. Functions, joins, expressions, arbitrary writes, views, virtual or
generated columns, floating-point results, PRAGMAs and attached databases are
outside the supported query interface. Results cap at 1,024 rows and 256 KiB with
bounded execution work. Dependencies cover the whole immutable snapshot; the
adapter does not invent a precise read set for arbitrary SQL. A live owned capture
has `OwnedEnvironment` fidelity, while an admitted immutable-snapshot SQL worker
uses `RecordedReplay` and may be pure under its exact profile.

The query fuel meter covers the supported query execution. It excludes online
backup, temporary database materialization and `quick_check`, which have their own
size or operation bounds. The logical memory budget does not enforce native SQLite
RSS through an OS quota. Scope validation protects calls through this host SDK;
it does not isolate a hostile caller with direct access to the filesystem.

## Retained evidence and deployment limits

Provider receipts, observation roots, snapshots and runtime profiles are immutable
objects. A modelled checkout is labelled `Model`; replacing it with an actual
receipt requires scheduler invalidation and rerunning dependent work before a
dependent authoritative transition. Offline replay does not resend effects.

The current conformance code covers local ownership, framing, provider uncertainty,
storage restoration and declared negative network probes. Read the dated
[progress record](progress.md) for commands that actually completed. A configured
CI lane is not a remote pass, and a local fixture test is not approval for a
hostile-site or production-provider deployment. Upstream runtime and license
provenance is in [the Stage 7 inventory](research/stage7-browser-provenance.json).
