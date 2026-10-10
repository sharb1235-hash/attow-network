# ChronosOS dependency probe

Independent API smoke test, verified on Windows x86_64 MSVC with Rust 1.95.0. It is not the ChronosOS implementation or a benchmark.

Run `./verify-existing.ps1` in PowerShell to require the already-installed Rust 1.95.0 and execute the locked build. The script never installs or updates a toolchain. A fresh machine needs the MSVC build tools/SDK and an explicitly selected Rust 1.95.0 installation.

The program validates Wasmtime 48.0.5 core/component calls and deterministic fuel trapping, a SQLite 3.53.2 WAL transaction and online backup, explicit CBOR/hash/signature calls, persistent map branch isolation, and an Iroh 1.3.0 direct QUIC echo between loopback endpoints. It uses no public discovery or relay services. A test-only fixed signing secret is not suitable for production identity creation.

A trapped Wasmtime Store is discarded before the independent component call. Logical state checkpointing and replay are application responsibilities. No live browser, live process or arbitrary WASM Store cloning is implemented or implied.

Commit Cargo.lock when copying this probe. Do not copy target/ build artifacts into source deliverables.
