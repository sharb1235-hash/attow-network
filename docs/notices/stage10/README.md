# Release notice recovery

`fallbacks.json` maps exact Cargo package versions to retained files, their original
URLs and SHA-256 hashes. These files supplement upstream archives that omit a
workspace license, and Iroh's archive, which includes its BSD contribution notice
but omits its MIT/Apache texts. File bytes are unchanged. The generator verifies
these hashes offline before including them in a candidate.

Most source commits come from the published archive's `.cargo_vcs_info.json`.
Wasmtime, Cranelift and Pulley share their recorded workspace commit. Repository
URLs are upstream metadata; null repository fields were resolved through the
containing workspace. The records preserve each exact commit rather than assuming
all similarly versioned subcrates were published from one commit.

Two exceptions are explicit:

- `enum-assoc 1.4.1` and `pharos 0.5.3` provide license declarations but no standalone
  text in their archives or the inspected upstream root. Their published Cargo
  manifests and READMEs retain attribution. Complete standard Apache-2.0 and
  Unlicense texts were retrieved from their official publishers; those texts are
  labeled as standard texts, not fabricated upstream package files.
- The two `winapi-*-pc-windows-gnu 0.4.0` archives omit VCS metadata and license
  files. Their original declarations/author attribution are retained alongside
  workspace licenses from the immutable commit recorded by resolved parent
  `winapi 0.3.9`. This does not establish the import libraries' original commit.

The release generator checks cached `.crate` archive hashes against `Cargo.lock`
and checks every copied registry notice against its archive member. It retains
nested notices, including ring's Fiat and once_cell notices, and the conjunctive
WHATWG/Unicode terms. Local `tar` reads archive members without extracting code;
the candidate archive writer and verifier are separate bounded Rust code.

The generated CycloneDX 1.6 SBOM covers the target-filtered, all-features Cargo
workspace graph, including build/development dependencies. The accompanying
inventory covers all resolved source packages and their notices. Neither scope
is a binary reachability analysis or a claim that inactive target packages were
built. Optional Node/Playwright/Chromium distributions and bundled SQLite have
their separate [runtime notice inventory](../stage7/README.md). Browser runtime
binaries are provisioned separately, not shipped in the CLI archive.

No source implementation is vendored here beyond the unmodified snippets already
present inside upstream READMEs. Retention and a passing metadata policy check do
not establish patent clearance or independent legal review.
