# Troubleshooting

| Symptom | Meaning and next step |
| --- | --- |
| Exit 3 / Unknown | Dispatch may have succeeded. Resume the same run to reconcile; keep its operation ID and provider state. |
| Exit 2 / conflict | Current authority dependencies, rights or decision no longer admit that proposal. Inspect branch differences; do not bypass validation. |
| Exit 4 / input | Check command spelling, duplicate flags and 64-character lowercase hexadecimal IDs. Replay requires `--offline`; import requires `--data-only`. |
| Exit 5 / infrastructure | Required storage, worker or runtime failed. The attempt is not a successful deterministic result. Keep the saved run for investigation. |
| Writer fenced | Another process holds the owned lock, the external anchor is missing, or history differs. Stop duplicate processes; do not remint a fence from a copied authority. |
| Browser unavailable | Run `doctor`; check the browser feature, exact runtime versions and absolute environment paths in the getting-started guide. |
| Build linker failure on Windows | Install the C++ toolchain and Windows SDK; reopen a suitable developer shell. |
| Build uses too much memory | Set `CARGO_BUILD_JOBS=2` for this shell. Building Wasmtime and peers is materially larger than the portable codec/state crates. |
| Imported run cannot promote | Expected: imports are immutable data-only evidence. They do not authorize source-domain dispatch. |
| Offline replay reports missing object | The bundle is incomplete or altered. Obtain a complete export; replay never fills gaps by executing imported code or contacting providers. |

Do not paste cookies, tokens, provider databases or signing keys into issue reports.
Include the command, exit category, OS, `rustc --version --verbose`, package version
and a minimal original fixture. See [security reporting](../SECURITY.md).
