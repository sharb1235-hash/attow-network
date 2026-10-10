# Retained adapter runtime notices

These files are unmodified copies of notices from the locally installed exact
Stage 7 runtime packages. They retain the upstream authors' terms and attribution;
the project's MIT OR Apache-2.0 choice does not replace those terms.

| Files | Original source |
|---|---|
| `node-24.21.0-LICENSE` | `LICENSE` in the official Node 24.21.0 Windows x64 archive, including bundled dependency terms |
| `playwright/**` | Playwright 1.64.0 npm archive: root license, notice, third-party index and every installed `*.LICENSE` bundle notice |
| `playwright-core/**` | Playwright-core 1.64.0 npm archive: corresponding root and bundle notices |
| `chromium-1248-LICENSE.headless_shell` | Complete `LICENSE.headless_shell` from the installed Windows Chromium headless-shell revision 1248, version 156.0.8078.4 |
| `sqlite-NOTICE` | Exact complete copyright-disclaimer comment at lines 32–45 of `libsqlite3-sys-0.38.2/sqlite3/sqlite3.c`, bundled SQLite 3.53.2 |

[The browser provenance inventory](../../research/stage7-browser-provenance.json) records
the original archive/registry URLs, versions, source commit, extraction paths and
SHA-256 of each retained notice. Playwright's license is Apache-2.0; its third-party
bundle notices contain additional upstream terms. Node and Chromium's aggregate
notices likewise contain multiple component licenses. No notice was shortened or
relicensed. The Chromium notice is intentionally retained in full.

[The SQLite provenance record](../../research/stage7-sqlite-provenance.json)
separately identifies the installed Rust FFI package, registry archive checksum,
source commit/URL, SQLite source ID, whole source-file hash and retained notice
hash. `sqlite-NOTICE` contains only the original complete comment from the embedded
`sqliteInt.h` header, with its exact bytes and newline preserved; no implementation
code is copied. The installed source matched the archive member byte for byte,
and the archive hash matched the lockfile and official registry metadata. The
Rust FFI package's MIT metadata and SQLite's own disclaimer are distinct.

The binaries and npm implementation packages are locally installed, ignored build
inputs, not vendored here. This inventory is specific to that installation and is
not a universal release SBOM. A future distribution must include the applicable
notices for its actual resolved artifacts. SQLite attribution and its public-domain
statement remain linked from the repository's
[third-party notices](../../../THIRD_PARTY_NOTICES.md).
