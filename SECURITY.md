# Security policy

Attow Network 0.1.0 is an experimental candidate. No stable release line, production support, response-time commitment or backport window is established. Implemented controls and their limits are documented in the [threat model](docs/threat-model.md) and [progress record](docs/progress.md).

Do not post vulnerabilities, credentials, provider sessions, authority databases, writer-fence state or private evidence in public issues. Use [GitHub private vulnerability reporting](https://github.com/sharb1235-hash/attow-network/security/advisories/new). GitHub's API confirmed this feature is enabled on October 16, 2026. Recipient delivery and response procedures have not yet been exercised; no reporting email or response-time promise is invented by this policy.

A useful private report includes the affected revision/profile, a minimal owned-fixture reproduction, expected versus actual behavior, potential impact, and any mitigation. Avoid contacting real purchase/payment providers or accessing another person's data to demonstrate a report.

The repository owner must test report delivery, assign a responsible recipient, and publish coordinated-disclosure expectations before claiming a supported public release. Enabling the reporting feature alone does not establish those procedures.

Known boundaries include in-process WASM execution, Node/Chromium child processes with fixture network mediation rather than an independent OS security sandbox, owner-scoped SQLite without encryption at rest, simulator-specific provider evidence, bounded local histories, and an owned-loopback fence that is not a distributed deployment fence. Imported bundles grant no writer activation; replay does not establish provider truth. See [adapter security](docs/adapter-security.md), [dependency policy](docs/dependency-policy.md) and the [launch checklist](docs/launch-checklist.md). The Linux-only unresolved advisory remains a failed qualification gate, not an ignored warning.
