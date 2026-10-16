# Dependency review

## October 16 security refresh

The refreshed native advisory gate rejected Wasmtime 48.0.5 and rustls 0.23.45.
The candidate selects the compatible patched releases Wasmtime 48.0.6 and
rustls 0.23.46. Wasmtime's matching internal/Cranelift packages move with its
exact release pin. No advisory ignore is used. The runtime profile identifies
48.0.6 explicitly, so execution and reuse cannot silently inherit the older
engine identity. Notice provenance follows each resolved package's source commit.

Sources: [Wasmtime 48.0.6 release](https://github.com/bytecodealliance/wasmtime/releases/tag/v48.0.6),
[rustls advisory and patched range](https://rustsec.org/advisories/RUSTSEC-2026-0346.html).
Stage 9 results describe the October 10 graph; final Stage 10 gates validate the
updated graph separately.

## Review policy

Direct versions are exact, inherited from the workspace and locked. The original
MIT OR Apache-2.0 license applies to Attow Network's own code. It does not replace
the licenses of dependencies, browser distributions or retained research.

`deny.toml` is checked with cargo-deny **0.20.2**, including development and build
dependencies and the all-target resolved graph for licenses. The Windows tool
archive was downloaded from the official release and its SHA256 checked against
the GitHub release asset digest. Local provisioning leaves the global tool intact.
See [tool evidence](research/stage9-tools.json).

| Allowed license | Reason in this dependency graph |
| --- | --- |
| MIT, Apache-2.0 | Existing project and dependency licensing |
| Apache-2.0 WITH LLVM-exception | Wasmtime/Cranelift toolchain components |
| BSD-2-Clause, BSD-3-Clause | Crypto, networking and utility dependencies |
| ISC | Crypto/network code, including conjunctive ring obligations |
| Unicode-3.0 | Unicode data and identifiers, including conjunctive obligations |
| Zlib | Compression dependencies |

Narrow exceptions apply only to the recorded versions: minicbor 2.3.0 uses
BlueOak-1.0.0; webpki-roots and webpki-root-certs 1.0.9 contain trust-anchor data
under CDLA-Permissive-2.0; async_io_stream 0.3.3, pharos 0.5.3 and ws_stream_wasm
0.7.5 use Unlicense in the target-specific Iroh dependency graph. Other expressions
with permitted MIT/Apache alternatives do not require blanket allowances. An
`AND` expression must satisfy each component; the checker does not erase notices.

Executed checks on the Stage 9 checkout:

```text
cargo-deny 0.20.2 --locked --offline check licenses
cargo-deny 0.20.2 --locked --offline check sources bans
```

Both passed. Duplicate transitive versions produce warnings under the explicit
`multiple-versions = "warn"` policy. No wildcard or unknown-source bypass was added.

The all-target advisory scan failed on **RUSTSEC-2024-0436**, the maintenance
advisory for paste 1.0.15, reached through Linux netlink-packet-core 0.8.2.
No patched 0.8.x release was available during review; 0.9.0 is not a compatible
drop-in lockfile update. No advisory ignore was added. The Windows x64 graph
does not contain paste and its native advisory check passed. Linux release
qualification remains blocked on resolving and testing that dependency chain.
The all-target failure is not reported as a passed safety or cross-platform gate.

References: [cargo-deny configuration](https://embarkstudios.github.io/cargo-deny/checks/licenses/cfg.html),
[official tool release](https://github.com/EmbarkStudios/cargo-deny/releases/tag/0.20.2),
[maintenance advisory](https://rustsec.org/advisories/RUSTSEC-2024-0436).
These checks evaluate published metadata and known advisories; release packaging
must also retain required license and notice files for the actual distribution.
