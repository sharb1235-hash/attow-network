# Persistence and recovery

The immutable-object store and local authority share a file-backed SQLite database.
Store, state and ledger changes compose through one transaction; event replay
reconstructs the authority's managed state and effect materializations. A storage
backup is not permission to run an effect dispatcher. Distributed activation and
automatic writer failover are outside the current support level.

## Database identity and durability

`Store::open` initializes schema version 1 with application ID `0x41545457`.
Version 0 is accepted only for an empty, unclaimed database. Newer versions,
another application's database, and missing expected columns fail closed.
Initialization and the version update share one immediate transaction; repeated
opens do not rerun the migration.

Writable store connections enable foreign keys, WAL, `synchronous=FULL`, and a two-second
busy timeout. SQLite's FULL setting in WAL mode syncs the WAL on transaction
commit, subject to the operating system and storage device honoring sync requests.
See the [SQLite synchronous documentation](https://www.sqlite.org/pragma.html#pragma_synchronous).
This project does not claim tested survival of every hardware or filesystem failure.

Objects use `(id BLOB PRIMARY KEY, kind, schema, bytes)` with fixed-width IDs and
bounded integer metadata. Blobs stay inside the database so object insertion,
pins, and authority records can share one transaction. Root pins are
identified by `(owner, id)`; owner names contain 1–256 UTF-8 bytes without NUL.
Multiple owners can retain the same graph independently.

## Integrity and transaction boundary

Every read recomputes the kind/schema/length-separated object hash. Import also
requires an independently expected ID. Reusing an existing ID checks its stored
hash, metadata, and exact bytes. Known schema bytes must pass strict canonical
decoding. Direct child references must already exist and verify when their parent
is inserted; build graphs children first. Unknown schemas may be retained as
opaque bounded bytes, but cannot be traversed by collection.

Object limits default to 1 MiB, 64 levels of CBOR nesting, and 100,000 decoded
items. Explicit limits must satisfy the codec's hard caps before a database is
opened. A single SQL statement substitutes NULL for an oversized blob before
allocating its Rust byte vector, avoiding a separate size-read race.

Use `Store::transaction` for object insertion plus pinning. A standalone `put`
commits an unpinned object; GC may collect it before a subsequent `pin`. Callback
errors and unwinding roll back changes. The callback receives a
`StoreTransaction`; `transaction_with` also preserves the caller's typed business
errors through `E: From<StoreError>`. Its `sql()` accessor exposes the same `rusqlite::Transaction` for trusted
local modules to write their own tables. Callers must not change store tables or
pragmas, issue transaction-control SQL, or dispatch effects. This interface does
not safely execute arbitrary SQL from external callers.

## Collection

GC holds `BEGIN IMMEDIATE` from the root read through the mark and sweep. SQLite
allows only one writer; a competing pin or collection waits for that transaction
or returns a busy error. This follows the documented
[immediate transaction behavior](https://www.sqlite.org/lang_transaction.html).
A pin committed first protects its graph. A pin following collection must find
its object or fail, so it cannot create a dangling root.

The collector validates hashes and reference schemas while traversing a bounded
queue. Unknown reachable schemas, missing/corrupt children, and exhausted budgets
abort before any deletion. Default traversal budgets are 100,000 distinct objects,
1,024 reference edges from a root, and 256 MiB of visited object payloads. Explicit
`GcLimits` can lower or change these budgets. The codec separately bounds each
object and its nested payload. A temporary SQLite mark table supports the final
sweep without allocating all unreachable object IDs in Rust.

Collection removes logically unreachable rows. It is **not secure deletion**:
free pages, WAL records, filesystem snapshots, and backups may retain bytes.

## Backups and recovery checks

`backup_to` uses the
[SQLite Online Backup API](https://www.sqlite.org/backup.html), via the pinned
[rusqlite backup interface](https://docs.rs/rusqlite/0.40.2/rusqlite/backup/struct.Backup.html).
It exclusively creates a new destination, never overwrites an existing path,
copies in 128-page steps, and stops if its 30-second budget expires. Individual
SQLite busy waits can extend the elapsed time slightly beyond that budget.
Errors remove the incomplete destination; a cleanup failure is reported explicitly.
Only a successful return identifies a completed snapshot. Never copy just a live
main SQLite file as a backup.

Tests cover committed WAL data in a backup, exclusion of another connection's
uncommitted writes, reopen and migration idempotence, whole-transaction rollback
before and after object insertion, corruption detection, missing references,
both pin/GC orderings, and collection resource failures. These are local Windows
test results when the progress record records them; other platforms require their
own executed evidence.

## Authority data and startup

The ledger keeps separate materializations for domain metadata, cells, policy,
rights, decision generations/winners, promotion deduplication, intents, outbox,
semantic slots and verified evidence. Canonical `ledger_events` are the replay
history; `ledger_meta` records the current sequence, head and state root. Store
objects and pins participate in the same transaction as accepted promotion.

Opening an existing local authority treats durable `Dispatching` intents as
potentially sent and records recovery to `Unknown`. This is conservative even
when a process failed before it actually sent anything. No recovery path treats
absence of a receipt as proof of rejection, releases an unknown slot or sends a
provider request automatically. A second connection opening the same authority
can trigger this conservative recovery too; clients must tolerate the change
and use matching evidence to reconcile a valid outstanding ticket.

Writable startup verifies the full history and materializations before recovering
effects. Ordinary authority operations also compare the canonical snapshot with
its materialized tables, so a later write cannot silently repair a tampered mirror.

Authority activation requires a trusted local deployment. The local writable
open path is bound to a stored canonical database path; opening a copied or moved
authority at another path fails with `ActivationRequired`. `Authority::inspect`
opens an existing database read-only for verification without creating files,
migrating schemas or recovering effects. Use inspection for a backup rather than
writable open. Relocation/activation is deliberately not automatic.

Path binding is a local guard, **not cross-host writer fencing**. A copy on another
host with the same path is not made safe by this check. Production activation
needs an independently enforced deployment epoch, fencing of the old dispatcher
and separately issued credentials. That distributed boundary remains future
work; a content hash, backup or static writer ID cannot substitute for it.

## Replay and materialized integrity

`Authority::rebuild_and_verify` reads events and the materialized source through
one consistent read transaction. Its pure reducer reconstructs state in a fresh
database and checks the event chain, sequence, state root and materialized
authority tables. This catches a modified cell, right, saved response, outbox or
intent even if the original event bytes and chain remain untouched. Verification
does not receive a provider interface, claim an intent or alter source rows.

The consistent snapshot matters when another database connection commits during
verification. Comparing events from one snapshot to materializations from a
later one could report false corruption or conceal an inconsistent comparison.
The store's read-transaction port also rolls back any accidental SQL writes made
inside its callback rather than committing them.

Replay streams the objects needed by admitted canonical events from the same SQL
snapshot. Each object read verifies its hash and is limited to 1 MiB. Direct
references are limited to 4,096 children and 16 MiB including the parent;
reopening a frozen branch checks its complete graph within 64 levels, 4,096
objects and 16 MiB. Replay does not scan or retain a cumulative cache of unrelated
staged objects. Unpromoted payloads therefore do not consume a historical-object
byte budget that could prevent reopening an otherwise valid authority.

Event verification allows at most 4,096 events and 64 MiB of event bytes.
Materialized comparisons allow at most 4,096 rows per table, 16 MiB per cell and
64 MiB per table copy. The canonical authority snapshot is capped at 16 MiB; the
codec and managed-state limits also apply. Exceeding a bound fails verification
rather than skipping admitted history.

The event limit includes capacity reserved for outstanding effects: three future
events for each `Authorized` intent, two for `Dispatching`, one for `Unknown` and
zero for a terminal intent. Every transition checks that the current sequence
plus these reservations fits within 4,096. New administrative work or promotions
are rejected before they can consume the event slots needed to claim, recover
and finish already authorized effects. Snapshot encoding also reserves 256 bytes
and 16 codec items per remaining lifecycle event for evidence metadata and counter
growth. Claiming, recovering, or completing an effect releases the corresponding
reservation. Receipt bodies remain separately bounded CAS objects. Actual storage
failures still return errors and preserve the prior durable state. The initial implementation
has a bounded history, with no automatic log rotation or archival migration.

A self-consistent malicious replacement of the whole database and its local head
is outside this check. Authenticate externally supplied histories against an
independently trusted checkpoint and an explicit rollback/equivocation policy;
the local hash chain alone is insufficient.

## Crash and upgrade procedure

Before reopening after a failure, stop or fence the earlier dispatcher. Preserve
the database and associated WAL files for diagnosis. Open the original authority
through the supported local path, inspect its recovery events and run
`rebuild_and_verify` before scheduling new work. Reconcile unknown operations
using authoritative provider status or the identical operation identity only
when the adapter contract permits it. Never create a fresh key merely because a
timeout or idempotency-retention expiry is inconvenient.

Use the online backup API before a schema upgrade and retain the exact previous
binary/profile and source revision. Migration must be transactional and reject
an unsupported newer schema. If an upgrade fails, do not point an old writer at a
partially migrated database or activate a backup while another dispatcher can
still send. No automatic authority migration/activation or remote fencing is
claimed by this stage.

The local gates include the registered `replay` and `storage_crash` integration
targets and the bounded `ledger_model` target. Child-process failpoints test abrupt
termination at specific transaction boundaries; they are not proof of every
storage-device failure mode. Run:

```sh
cargo test --test replay --locked
cargo test --test storage_crash --locked
cargo test --test ledger_model --locked
```

Content addressing and transactional storage are established techniques. Original
store code is dual-licensed MIT OR Apache-2.0; dependency obligations are recorded
in the repository's notices and dependency evidence.
