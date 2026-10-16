# 0.1.0 candidate launch conditions

The source implements the candidate flow. Actual completion is established by
the candidate's manifest/evidence and [progress](progress.md), not this checklist
alone. `candidate-complete; signing-pending` requires every non-signing artifact
gate to pass. `release-ready` additionally requires authentic configured signing.

| Condition | Required evidence or remaining owner action |
| --- | --- |
| Source identity | Clean committed revision and matching lock hash before/after every release check |
| Native platform | Actual Windows x64 release archive and extracted-binary install/replay/verify evidence |
| Other platforms | Linux remains blocked by the unignored paste advisory; macOS qualification has not run |
| Dependencies | Exact registry archive checksums, target SBOM, conservative source inventory and complete retained notice mapping |
| Browser prerequisites | Verified Node 24.21.0, Playwright 1.64.0 and Chromium revision 1248; no bundled runtime or silent Model fallback |
| Real demonstration | Captured release-binary output/events, five branches, owned browser/SQL/WASM/two peers, Unknown reconciliation and offline replay |
| Independent integrity | Manifest/artifact/checksum consistency and data-only evidence validation; no execution of untrusted candidate payloads |
| Signing identity | Pending: existing maintainer-authorized key/KMS or protected CI OIDC identity, pinned external verifier trust |
| Authentic signature | Pending: frozen candidate signature and successful require-signatures gate; temporary test keys do not qualify |
| Public repository | Created and API-verified public: [sharb1235-hash/attow-network](https://github.com/sharb1235-hash/attow-network); source push follows final gates |
| Security reporting | GitHub API confirms private vulnerability reporting enabled; delivery test, recipient assignment and disclosure expectations remain pending |
| Conduct reporting | Pending: private moderation route and responsible owner |
| Release CI protection | Pending: configure required reviewers/default-branch protection; workflow YAML alone does not prove protection |
| Maintenance ownership | Repository owner is the current steward; no invented team, SLA, backport window or stable release line |
| Name/package review | Public name chosen by owner; registry ownership and trademark/patent clearance are not established |
| Registry publication | Disabled (`publish = false`); no registry artifacts or installation claims |
| Upgrade/rollback | No migration or rolling-upgrade promise; preserve external fence/head and unresolved operation IDs; inspect backups without activation |
| Reproducibility | Pinning and recorded provenance only; independently equal binary builds have not been demonstrated |

The owned-loopback fence is a same-host fixture and assumes the external anchor
is not copied. Provider guarantees apply to the durable owned simulator, not an
arbitrary real purchasing service. WASM executes in the Rust host process;
Node/Chromium use separate child processes with fixture network mediation, not
an independent OS security sandbox. No hostile-OS isolation, universal exactly-once execution, encrypted
storage, automatic partition failover, consensus, novelty or throughput claim is
made. These boundaries remain visible in release notes and the actual demo.
