# Static authority and explicit handoff

A local authority protects one host-managed database. A copied database is not
a second authority. A federated authority additionally requires an explicit
designated writer identity, epoch and a trusted external writer fence. The fence
owns credentials and its exclusive session outside the database and all exports.
A filesystem path, timeout, signed checkpoint or restored SQLite file is not
cross-host fencing evidence.

The ledger exposes `WriterFence` and `WriterLease` as trusted integration ports.
The deployment must implement exclusive admission and retain the latest durable
authority head across sessions and hosts. A lease checks the current binding
before every authority mutation or dispatch and advances the external head after
durable mutation. Failure or disagreement fails closed. Recovering a database /
fence mismatch requires an explicit operator procedure; it never silently opens
an older snapshot for writing. Test fencers are owned in-memory fixtures and do
not provide a distributed service for deployments.

For a planned handoff:

1. Stop new application work and provider dispatch. Serialize the handoff with
   existing application operations.
2. Close the current epoch, naming the exact next writer. Preserve unresolved
   in-flight effects as `Unknown`; retain their original provider operation and
   attempt bindings. Do not release their rights because the network is quiet.
3. Transfer and verify the closed database checkpoint. A plain reopen remains
   read-only or fails writer admission.
4. Explicitly authorize the new writer through the external fence for the closed
   head and next epoch, then activate it. The old writer remains closed.
5. Reconcile inherited uncertainty using the original provider evidence binding.
   A new authority epoch does not create a new logical provider operation.

The operator must supply a fence whose authority and credentials are independent
of restorable snapshots. This prototype intentionally has no built-in production
fencing service, quorum protocol or crash-available automatic failover. If that
external service cannot prove admission, mutation and dispatch remain blocked.
