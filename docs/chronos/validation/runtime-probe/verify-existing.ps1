$ErrorActionPreference = 'Stop'
$version = & rustc --version
if ($LASTEXITCODE -ne 0 -or $version -notmatch '^rustc 1\.95\.0 ') {
    throw "Expected the already-installed Rust 1.95.0; found: $version. This script never installs or updates a toolchain."
}
& cargo run --locked --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'Chronos dependency probe failed' }
