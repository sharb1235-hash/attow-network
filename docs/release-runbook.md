# Release runbook

The 0.1.0 candidate flow produces a native Windows x64 CLI archive, a committed
source archive, dependency notices/SBOM, installation evidence, an actual terminal
demo and a frozen manifest. Every package remains `publish = false`. The
[launch checklist](launch-checklist.md) distinguishes executed evidence from
external setup. An unsigned candidate is never described as release-ready.

## Prepare the exact source and tools

The source must be clean and committed. Do not discard working files to obtain a
clean tree. Source distribution reads Git blobs from that commit; it rejects
unsafe paths and excludes private workspaces, credentials, caches and local tools.
The owner authorized the public repository destination
`sharb1235-hash/attow-network`; this does not imply registry publication, configured
security reporting, an authorized signing identity or trademark clearance.

Use Rust 1.95.0 and the reviewed tools in
[`release-toolchain.json`](../release-toolchain.json). The ordinary offline CLI
does not require Node. The all-features and recorded browser-demo gates require
the verified Node 24.21.0 archive, locked Playwright 1.64.0 dependencies and Chromium
revision 1248. Set absolute `ATTOW_NODE`, `ATTOW_BROWSER_SIDECAR` and
`PLAYWRIGHT_BROWSERS_PATH` as described in [getting started](getting-started.md).
Set `ATTOW_CARGO_DENY` to the verified cargo-deny 0.20.2 executable; fetch its
advisory database before the offline policy check. Cargo's registry cache must
contain the complete locked graph, including target-specific packages, for notice
inventory. The native `tar` command reads selected notice members from verified
registry archives and its version is recorded in the generated inventory.

## Gates and candidate production

Run the plan and complete local gates on the final source revision:

```sh
cargo +1.95.0 run -p xtask --locked -- release-plan
cargo +1.95.0 run -p xtask --locked -- release-check --local
cargo +1.95.0 run -p xtask --locked -- release-build --target x86_64-pc-windows-msvc --output release-candidate
cargo +1.95.0 run -p xtask --locked -- release-verify --dir release-candidate --integrity-only
```

`release-check` records separate repository, formatting, default/all-features test
and Clippy, documentation, license, native advisory, actual demo and source-install
results. A failing check stops the run. The build requires that successful report
to match the same clean source revision and lock hash. Use a new output directory;
do not merge new files into an old candidate or rewrite a signed manifest.

Only `x86_64-pc-windows-msvc` is qualified by this flow. The Linux-only paste
maintenance advisory remains an unignored failure; macOS has no executed release
qualification. Remote workflow configuration does not establish remote results.
Pinned tools and deterministic archive metadata do not prove bitwise reproducible
binaries; no independent-build equality claim is made.

## Inspect and install

Inspect `manifest.json`, `SHA256SUMS`, `provenance.json`, `runtime-profile.json`,
`sbom.cdx.json`, `dependency-inventory.json`, `notices/`,
`evidence/archive-install.json` and `demo/`. The target SBOM includes development
and build dependencies; the all-source inventory also includes inactive target
packages. Exact notice provenance and archive checksums are retained. The optional
browser runtime binaries are not bundled; sidecar/app sources and locked package
metadata are shipped and runtime prerequisites remain explicit.

Candidate production extracts its own verified archive into a fresh directory
containing spaces and runs the actual release binary's help, doctor, initialization,
offline data-change, replay and verification. These results are distinct from the
debug-profile source-install smoke. The archive contains `bin/attow.exe`.
After integrity verification, manual PowerShell installation into a new directory
uses the same standard archive format:

```powershell
New-Item -ItemType Directory 'C:\Attow Candidate'
tar -xf release-candidate/attow-network-0.1.0-x86_64-pc-windows-msvc.tar -C 'C:\Attow Candidate'
& 'C:\Attow Candidate\bin\attow.exe' --help
```

Do not run a downloaded artifact merely because its self-contained hashes agree.
Hashes establish internal consistency; authentic release trust additionally
requires the independently configured signature check below. Source installation
is documented as `cargo +1.95.0 install --path crates/chronos-cli --locked`; there
is no `cargo install attow` registry claim.

## Actual recorded demo

The producer records its extracted release binary. The standalone recorder is:

```sh
cargo +1.95.0 run -p xtask --locked -- demo-record --output target/recorded-demo
```

`demo/events.json` contains actual commands, exits and CLI envelopes;
`terminal.cast` contains captured subprocess output and measured timing;
`index.html` is a local viewer; `recording.json` binds the binary and recording
hashes. Browser/WASM/SQL and two private peers are real local components; the
purchase provider is explicitly the owned durable simulator. The sequence pauses
with `--pause-after-prepared`, lists/compares branches before promotion, resumes
the same run into Unknown, reconciles the same intent into Confirmed, and performs
offline replay/verification. Worker counts are measured; no speedup is asserted.
The two pause flags are mutually exclusive.

## Freeze and authenticate

The manifest lists artifact paths, sizes and SHA-256 hashes and binds source,
target, compiler, lock, runtime profile, gates and expected signing identity.
`SHA256SUMS` includes the manifest and its listed artifacts. Neither manifest nor
checksums list the detached `manifest.sigstore.json` signature, preventing a hash
cycle. Once frozen, changing any listed byte invalidates the candidate.

Configure `ATTOW_RELEASE_SIGN_CONFIG` as an absolute file outside the candidate.
It selects the verified Cosign 3.1.3 executable/hash and independent trust:
an existing public key path/hash, or an exact OIDC identity/issuer. Its schema-1
fields are `cosign`, `cosign_sha256`, `trust`, `require_transparency` and optional
`signing`. Key signing names an environment variable holding an already-authorized
key/KMS reference; no secret values go into the candidate. OIDC requires explicit
transparency authorization. Verification-only configuration omits `signing`.
See `xtask/src/release/sign.rs` for the strict tagged configuration schema.

The expected identity must be configured before candidate construction. A
candidate frozen with no identity must be rebuilt once real trust is established;
do not retrofit an unreviewed identity into its manifest. No signing credentials
are currently configured. A temporary test key only tests cryptographic behavior
and never qualifies as maintainer identity.

```sh
cargo +1.95.0 run -p xtask --locked -- release-sign --dir release-candidate --mode configured
cargo +1.95.0 run -p xtask --locked -- release-verify --dir release-candidate --require-signatures
```

Missing credentials, signatures or matching external identity must fail this
gate. Until genuine signing passes, use `candidate-complete; signing-pending`
only after all other candidate gates pass. `release-ready` requires authentic
expected-identity verification. The PowerShell and shell wrappers in `scripts/`
require an explicit integrity-only or require-signatures mode. Verification never
executes a candidate binary or its claimed checks.

## Publication, recovery and compatibility

The manual release workflow builds an unsigned candidate from the default branch
with read-only repository permissions and pinned actions. Its environment's
required reviewers/protection must be configured by the owner; naming an
environment in YAML does not establish that protection. It grants no OIDC token
and performs no tagging, registry publication, GitHub release or signing. Review
actual workflow results before advertising any remote qualification.

Preserve the original authority database and external fence before upgrades.
Data-only exported bundles cannot reactivate a writer, and restoring an older
authority copy is not a safe rollback: its head must still match the external
fence. Inspect/replay offline first. Cross-version migrations and rolling upgrades
are not supported; do not point older binaries at newer stores. Preserve unresolved
Unknown intents and their original operation identities across recovery. Stop
dispatch if activation, fencing or evidence verification fails.

If validation or distribution fails, preserve the evidence and stop the affected release. Do not rewrite an already published artifact under the same version or erase an actual external effect from release/recovery history. Publish corrective versions through the authorized channel.
