# Attow Network store

SQLite content-addressed storage with verified reads, atomic object/root transactions,
bounded mark/sweep collection, and online backups. Public package: `attow-store`.
Internal workspace dependency alias: `chronos-store`.

Use `Store::transaction` when inserting and pinning a new root together. Separate
`put` and `pin` calls allow collection between those calls. Known generic objects
must name already-present children; insert children before parents in the same
transaction. Unknown schemas can be stored as opaque objects, but a reachable
unknown schema makes collection fail without sweeping.

The `StoreTransaction::sql` accessor is reserved for trusted local modules that
need to commit their own tables in the same transaction. It is not an untrusted
query API. Store APIs do not grant dispatch authority.

See [persistence and recovery](../../docs/persistence-recovery.md) for transaction,
resource, backup, and corruption guarantees and their limits.
