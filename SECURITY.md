# Security policy

Attow Network 0.1.0 is an experimental candidate. No stable release line, production support, response-time commitment or backport window is established. Implemented controls and their limits are documented in the [threat model](docs/threat-model.md) and [progress record](docs/progress.md).

Do not post vulnerabilities, credentials, provider sessions, authority databases, writer-fence state or private evidence in public issues. Use a verified private reporting channel once the repository owner has configured it. At this candidate checkpoint, that private channel has not been verified; ask the owner to establish a private route without including vulnerability details. No reporting email is invented by this policy.

A useful private report includes the affected revision/profile, a minimal owned-fixture reproduction, expected versus actual behavior, potential impact, and any mitigation. Avoid contacting real purchase/payment providers or accessing another person's data to demonstrate a report.

The owner must enable and test private reporting for the planned repository `sharb1235-hash/attow-network`, assign a responsible recipient, and publish coordinated-disclosure expectations before claiming a supported public release. Repository creation alone does not establish those controls.

Known boundaries include in-process WASM/browser execution, owner-scoped SQLite without encryption at rest, simulator-specific provider evidence, bounded local histories, and an owned-loopback fence that is not a distributed deployment fence. Imported bundles grant no writer activation; replay does not establish provider truth. See [adapter security](docs/adapter-security.md), [dependency policy](docs/dependency-policy.md) and the [launch checklist](docs/launch-checklist.md). The Linux-only unresolved advisory remains a failed qualification gate, not an ignored warning.
