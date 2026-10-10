# Run Attow Network locally

The CLI runs owned demonstrations of alternative execution, selected promotion,
durable uncertainty and offline verification. All effects go to a loopback
simulator. Windows x64 is the locally validated platform; other CI lanes are
configured for verification and are not released platforms.

Install Rust through rustup and the native C/C++ build prerequisites. On Windows,
use Visual Studio Build Tools with the Desktop development with C++ workload and
a Windows SDK. Linux needs a C toolchain/linker; macOS needs Command Line Tools.
Those two platform instructions have not been validated locally.

From a checkout, install the pinned toolchain and build the CLI:

```sh
rustup toolchain install 1.95.0 --profile minimal --component rustfmt --component clippy
cargo +1.95.0 install --path crates/chronos-cli --locked --root target/install
```

The binary is `target/install/bin/attow` (`attow.exe` on Windows). Add that directory
to PATH for this shell or invoke its absolute path. There is no registry package
installation promise. All workspace packages remain `publish = false`.

```sh
attow --help
attow init "my workflow"
cd "my workflow"
attow run data-change --workers 2 --json
attow branch list --json
```

Save `data.run_id` from the run's JSON envelope. Use it for subsequent commands:

```sh
attow replay RUN_ID --offline --json
attow verify RUN_ID --json
attow export RUN_ID --output run.attow
```

`RUN_ID` is a placeholder for the actual 64-character hexadecimal identifier.
Branch listing gives the actual IDs for `attow compare BRANCH_A BRANCH_B --json`.
Every machine-readable response has schema version 1 and separates status from
data. Exit codes: 0 success, 2 conflict, 3 unknown outcome, 4 invalid input and
5 infrastructure failure. An Unknown exit is expected in the pause demonstration:

```sh
attow run checkout --workers 2 --pause-after-unknown --json
attow run checkout --workers 2 --resume RUN_ID --json
attow replay RUN_ID --offline --json
```

The second command resumes the exact persisted operation and reconciles the
simulator's receipt. It does not authorize a replacement purchase. Keep `.attow`
intact between these commands. A missing external demo fence stops resumption.

For data-only exchange, initialize another workspace and import the bundle:

```sh
attow init "inspection workspace"
cd "inspection workspace"
attow import /absolute/path/to/run.attow --data-only
attow replay RUN_ID --offline --json
attow verify RUN_ID --json
```

Imports contain immutable objects and run records. They do not copy writer
credentials, provider secrets or authority activation. Verification of an imported
bundle checks its portable object/trace evidence, not the source's full authority
database or the truth of a real provider's response.

## Optional owned browser

Default installation does not require Node or Chromium. For the complete browser
demonstration, install with `--features browser` and provision Node **24.21.0**,
Playwright **1.64.0** and the browser revision in its committed lockfile:

```sh
npm ci --ignore-scripts --prefix demos/workflow-studio/browser
node demos/workflow-studio/browser/node_modules/playwright/cli.js install chromium
```

Set `ATTOW_NODE` to the absolute Node executable, `ATTOW_BROWSER_SIDECAR` to the
absolute `demos/workflow-studio/browser/sidecar.mjs`, and `PLAYWRIGHT_BROWSERS_PATH`
to the browser installation directory used during provisioning. The installed
binary never searches an assumed source-checkout path for an optional sidecar.
Use `attow doctor --json`, then append `--browser` to the checkout run command.
See [browser fidelity](browser-fidelity.md) for runtime integrity and boundaries.

For PowerShell, environment variables use `$env:ATTOW_NODE = 'C:\absolute\node.exe'`;
POSIX shells use `export ATTOW_NODE='/absolute/node'`. Keep paths quoted when they
contain spaces. The Stage 9 source-install smoke deliberately uses such paths.
