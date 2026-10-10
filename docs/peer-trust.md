# Peer trust and deployment boundary

Each endpoint has a locally supplied Iroh identity. Configuration explicitly
allows endpoint IDs and the operations they may perform for one tenant/domain.
Successful TLS authentication identifies a peer; it does not grant object read,
import, proposal or effect-dispatch permission. Root export grants restrict what
an otherwise authenticated reader can obtain. Keep endpoint secrets outside CAS,
branch exports, logs and source control.

The default endpoint binds a supplied loopback/private socket address, disables
relays and address lookup, and clears implicit IP listeners. Tests use real
QUIC endpoints on loopback. A separately labelled fault model exercises message
loss, duplicates and stale proposals; it is not a claim to simulate Iroh internals.

Imported graphs are read-only data. Inspecting a branch, replaying recorded work,
or verifying a checkpoint does not acquire a writer lease. The application must
separately admit a designated writer through the trusted external fence described
in [authority handoff](authority-handoff.md). During a partition peers may continue
speculation; they cannot create a replacement writer after a timeout.

There is no default public bootstrap server, anonymous object hosting, remote
guest execution, provider credential delegation, automatic takeover or consensus
service. An operator adding a relay or routed network must independently provision
endpoint keys, allowlists, tenant isolation, firewall policy and availability.
That deployment is outside the verified private-loopback configuration.
