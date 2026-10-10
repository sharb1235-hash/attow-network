# State and worldline representation

Stage 3 implements immutable state roots, isolated speculative branches, explicit
dependencies, portable descriptors and durable admission. It does not authorize
effects, clone native processes, restore arbitrary browser sessions, or establish
a distributed writer. The [architecture](architecture.md) describes those later
boundaries; the [progress record](progress.md) identifies implemented stages.

## Roots and versions

`StateRoot` contains a domain, sequence, whole-namespace epoch and immutable cells
in an `rpds::HashTrieMapSync`. A cell stores a positive `Version` and an optional
byte value. A missing cell has version zero; a deleted cell remains a tombstone
with its positive deletion version. Reading absence, then observing an insertion
and deletion, therefore detects a changed version even when the value is absent
again.

`StateRoot::apply` mechanically creates a new root. Every nonempty batch increments
the root sequence and namespace epoch once and gives all touched cells that fresh
version. Empty batches preserve the existing root. Inputs and aggregate resultant
size are checked before new values are cloned into the persistent trie. Sequence
overflow fails. This function does not check rights or authorize a transaction;
the later authority layer must validate its own invariants before applying a batch.

The canonical state object uses the generic codec kind/schema `1/1` envelope, has
no object references, and carries this payload:

```text
["attow-state-root", 1, domain_bytes32, sequence, namespace_epoch,
 [[key_bytes, positive_version, value_bytes_or_null], ...]]
```

Cells are sorted by key bytes. Duplicate or unsorted keys, zero versions, versions
newer than the root, inconsistent epochs, unsupported schemas and oversized data
are rejected. Tombstones are serialized. Trie iteration order is never durable
encoding. Root IDs include domain, versions and tombstones; equal visible values
alone do not imply equal root IDs.

## Branches and semantic reads

`fork(domain, root, nonce)` shares an immutable root and creates private empty
read/overlay maps. A mismatched domain is rejected. Branch identity hashes the
domain, base object ID and explicit 32-byte nonce under a separate object kind.
Distinct nonces distinguish branches over identical state. Callers must supply a
fresh nonce when they intend a distinct branch; repeating all inputs repeats the
identity. Changing branch contents produces a distinct frozen descriptor ID.

`get` returns the branch's own put/delete when present and otherwise reads the
base. Either path records the base key version, including zero for never-seen
absence. Blind put/delete also records that expected version. A prefix scan merges
the base and overlay, removes deleted entries, returns sorted keys and records the
whole namespace epoch, including an empty scan. This conservative epoch detects
phantoms and any other write in the namespace. Failed writes restore the previous
overlay.

`freeze` moves the branch into an immutable `FrozenBranch`. It is an in-memory
proposal, not a durable checkpoint. Its dependencies and overlay remain private
and are exposed through immutable accessors. No branch method spends rights,
activates a writer or dispatches an external effect.

## Portable worldline metadata

A `Worldline` contains schema version one and optional code, profile, continuation,
observation, policy, backend-binding and security-scope object roots, followed by
an ordered ancestry list. The default metadata explicitly describes a data-only
branch. Executing code later requires the relevant roots and backend admission;
default metadata supplies no execution permission. Ancestry entries are unique
object IDs. Native handles, host stacks and credentials are not portable roots.

The frozen object's payload has exactly ten fields:

```text
["attow-frozen-branch", 1, domain_bytes32, branch_id_bytes32, nonce_bytes32,
 base_root_id_bytes32, base_sequence, dependencies, overlay, worldline]

dependency = [0, key_bytes, expected_version] | [1, namespace_epoch]
overlay_entry = [key_bytes, 0, value_bytes] | [key_bytes, 1, null]
worldline = [1, code, profile, continuation, observation_root, policy,
             bindings, security_scope, ancestry]
```

Optional object roots are null or 32-byte IDs. Key dependencies and overlay entries
are sorted by key; a namespace dependency appears at most once, after key entries.
Every overlay key must have its expected base version dependency. Decoding checks
the base domain, sequence, nonce-derived branch identity and every expected version.
The generic object's explicit reference set must equal the union of its base root
and all worldline roots/ancestry. A payload cannot omit a declared metadata root
from the reachable object graph.

## Persistence and admission

Production state depends on the codec and persistent collection, not SQLite.
`StateReader`, `StateTransaction` and `StateStorage` are backend-neutral ports.
The facade's local adapter composes them with the existing store transaction.
A storage adapter is a trusted boundary: its callback must serialize object/pin/GC
changes, roll back on errors or unwinding, and return success only after commit.

`StateRoot::persist(storage, owner)` writes and verifies the canonical root and
pins it in one callback. `FrozenBranch::admit(storage, owner)` writes its base and
descriptor, traverses and independently hash-checks the complete referenced graph,
then pins the descriptor in that same transaction. Unknown schemas, missing or
corrupt references, excessive graph depth and pin failures abort the transaction.
The descriptor itself counts toward graph limits. The pin protects the base and
metadata transitively from GC while its owner retains it.

Reopening independently checks content hashes even when the reader claims to have
verified its data. It enforces the expected domain and schema. Reopen is a read and
does not create an additional retention owner; the caller must keep its existing
pin for continued durable retention. `data_only_import(new_domain)` explicitly
copies only visible values into a fresh domain/root sequence. It discards source
tombstones and history and carries no source authority or dispatcher activation.

## Conflicts and joining

`conflicts(current_root)` compares all frozen key versions and namespace epochs
with the current root, including absent keys and tombstones. `diff` reports cells
whose value or version changed, retaining deletions as versioned cells.

`join` is deliberately conservative. It rejects domain mismatch, reports different
bases, and reports shared writes, read/write overlap or a scan against another
proposal's writes. Disjoint proposals can produce a combined overlay/dependency
set. The result remains a proposal: compatibility is not authorization or a proof
that arbitrary external actions can be serialized.

## Bounds and cost model

Each root permits 4,096 cells including tombstones, 1,024 bytes per key, 64 KiB per
value and 512 KiB total key/value bytes. An overlay uses the same key/value/count
limits. Branches permit 4,096 dependencies and 64 ancestry entries. Canonical
objects also obey the codec's default 1 MiB byte, 64-level and 100,000-item limits.
Admission permits 64 object-graph levels, 4,096 bounded object reads/pending IDs
and 16 MiB total verified bytes. Invalid input fails with a typed error. Readers
must bound their own object allocation before returning data to the state layer.

Logical fork is constant-sized work after root identity is cached: roots share
persistent trie nodes and cached canonical bytes. Root construction/application
and reopen still inspect all cells; sorted durable encoding costs O(n log n),
and first loading costs at least O(n). Durable storage currently contains a flat
canonical snapshot, not a disk-backed persistent tree. Applying a root caches its
encoding/ID so subsequent forks do not traverse the state again.

Overlay lookups use ordered maps. Writes currently recompute overlay byte usage,
so each write can inspect the existing overlay. Scans inspect the root/overlay and
sort returned entries. Freeze encodes dependencies and the overlay. Admission adds
serialization, graph verification and a durable transaction. The benchmark reports
logical fork, admission and reopen separately; it is not a materialization or
browser-cloning benchmark and has no wall-clock pass threshold.

## Research and attribution

Structural sharing is established work, implemented here using
[rpds](https://github.com/orium/rpds). Microsoft's
[Concurrent Revisions project](https://www.microsoft.com/en-us/research/project/concurrent-revisions/)
and its [semantics paper](https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/semantics-revisions-2010.pdf)
study snapshots and deterministic reconciliation. These are prior art, not source
code copied into this project. Deterministic merge behavior does not establish
serializable promotion. The [branch semantics ADR](adr/0006-branch-semantics.md)
records the choices and rejected guarantees.
