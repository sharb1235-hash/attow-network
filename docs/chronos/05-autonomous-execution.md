# ChronosOS staged autonomous execution

This package supplies a build contract and ten implementation prompts. The scripts initialize a new repository and run one Codex task per stage, in order. They do not turn the current reference implementation into a completed production system merely by copying files.

The supplied reference kernel passed **13 integration tests**, formatting and warning-free Clippy checks, plus its local checkout example. The separate runtime probe passed on Windows with Rust 1.95.0, Wasmtime 48.0.5, Iroh 1.3.0 and bundled SQLite 3.53.2. These are narrow functional checks. The full production workspace, all ten autonomous stages, browser integrations, distributed operation and scale claims have not been built or validated here. See [reference results](validation/reference-results.json), [dependency report](validation/dependencies.md) and [runtime probe](validation/runtime-probe/README.md).

## What is actually configurable

The installed CLI's `codex exec --help` was inspected. The runner uses these supported parameters:

```text
codex exec --cd TARGET --sandbox workspace-write --approve-for-me
  --json --color never --output-last-message FINAL_REPORT -
```

The final `-` reads the complete stage prompt from standard input. The script uses an argument array, not a constructed shell command. `--approve-for-me` routes approval requests through automatic review; it does not guarantee that every operation will be approved. The workspace sandbox and existing hook-trust rules remain in effect. The runner never supplies bypass flags, disables rules, ignores user configuration, or uses the absent `--full-auto` flag.

Without `-Model`, the scripts preserve the model selected by your existing Codex configuration. To explicitly select the model exposed on this host, add `-Model gpt-6-astra`. There are no undocumented “Astra” performance switches in this package. Concurrency and memory limits for ChronosOS are implementation settings specified by the build prompts, not model intelligence controls. These scripts do not set token budgets, buy credits or promise unlimited unattended execution.

## Prerequisites

Use PowerShell 7.2 or newer, Git, an authenticated Codex CLI, native MSVC build tools/Windows SDK, and the pinned Rust toolchain with rustfmt and Clippy. The current machine has PowerShell 7.6.5 and Rust 1.95.0 under its existing `stable` alias. No global toolchain was changed during preparation.

Check the CLI without starting an agent:

```powershell
codex exec --help
codex login status
rustc --version
cargo --version
git --version
```

On a fresh machine that needs the pinned toolchain, install it side by side; this does not change `rustup default`:

```powershell
rustup toolchain install 1.95.0-x86_64-pc-windows-msvc --profile minimal --component rustfmt --component clippy --no-self-update
if ($LASTEXITCODE -ne 0) { throw 'Toolchain installation failed' }
```

The target blueprint pins `rust-toolchain.toml` to 1.95.0. An existing toolchain under the mutable name `stable` does not necessarily satisfy that exact-name lookup; preinstall the named toolchain if necessary. Later toolchain changes belong in a reviewed upgrade. Wasmtime 48 is an LTS line and compiles with 1.95; latest Wasmtime 49.0.2 requires at least 1.96. The full rationale and exact package licenses are in the dependency report.

## Initialize a new repository

Start PowerShell in this package's original workspace:

```text
C:\Users\Sharb\Documents\Codex\2026-10-09\files-pasted-by-the-user-you
```

Choose a **new or empty child directory**. The initializer refuses a nonempty target, a target outside the workspace, or a path containing a junction or symbolic link. It copies only package inputs and runs `git init` in that target; it does not install dependencies, create commits, contact services or start Codex.

```powershell
$packagePath = (Resolve-Path '.\outputs\chronosos').Path
$targetPath = Join-Path (Get-Location).Path 'chronosos-build'

& (Join-Path $packagePath 'scripts\Verify-Package.ps1')
& (Join-Path $packagePath 'scripts\Initialize-Chronos.ps1') -TargetPath $targetPath -WhatIf
& (Join-Path $packagePath 'scripts\Initialize-Chronos.ps1') -TargetPath $targetPath
```

If the package is intentionally relocated, supply an explicit existing `-WorkspaceRoot` and choose a new child of that root. The package location is inferred from the initializer's directory; `-PackagePath` can select another complete package. There must be exactly ten Markdown prompts, one each beginning `01-` through `10-`, with the first named `01-foundation.md`.

The initialized target has:

```text
README.md                         # editable by the implementation stages
docs/chronos/                     # fixed specification, blueprint and execution guide
docs/chronos/validation/          # available dependency evidence and source-only runtime probe
.chronos/prompts/                 # the ten fixed stage prompts
.chronos/scripts/                 # execution scripts
.chronos/manifest.json            # input hashes and initialized target/workspace paths
.chronos/state/control.json       # request a stop at the next stage boundary
.chronos/state/stages.json        # created when execution starts
.chronos/runs/                    # per-run events, final messages and gate logs
```

Immutable package inputs are checked before each stage. The runner stops if those inputs change. This prevents silently continuing with a different plan; it is not a cryptographic protection against an actor who can rewrite the whole repository. Do not modify or fabricate runner completion records to bypass a failed stage.

## Preview, then run

Previewing has no paid agent calls and does not run Cargo:

```powershell
$runnerPath = Join-Path $targetPath '.chronos\scripts\Run-ChronosStages.ps1'
& $runnerPath -TargetPath $targetPath -ThroughStage 1 -DryRun
```

Run one stage in the foreground first:

```powershell
& $runnerPath -TargetPath $targetPath -ThroughStage 1
```

After inspecting its results, continue the remaining stages:

```powershell
& $runnerPath -TargetPath $targetPath -ThroughStage 10 -Resume
```

To explicitly override the existing model selection, append `-Model gpt-6-astra` to either invocation. The script does not add a model override otherwise. All live invocations consume your Codex account's usage and are subject to its normal limits.

Each stage is a **separate `codex exec` invocation**. State carries forward through repository files and the previous validation record, not a hidden shared conversation. The stage runner passes the relevant prompt, shared document paths and retry rules to each invocation. It will not start stage N+1 unless stage N returns successfully, produces a final report, preserves its fixed inputs and passes all three independently run gates:

```powershell
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

The stage prompts also define feature-specific acceptance criteria. Stages 9 and 10 include additional release gates in their prompts; the generic runner does not replace those requirements. Read their final reports and recorded release evidence before accepting a release. A runner record marked `completed` means the agent returned a report and passed the three Cargo gates; it does not certify semantic completion of every stage-specific condition. In particular, a stage 10 candidate can still await signing, platform checks or other explicit release prerequisites. Never equate that record with an approved or published release. Successful generic Cargo gates alone are not proof of every product claim.

## Run in the background

The launcher starts PowerShell with `-NoProfile -NonInteractive -File`, uses `WindowStyle Hidden`, and captures stdout, stderr and a process-ID receipt. It does not create a recurring automation or use an untrusted hook override.

```powershell
$launcherPath = Join-Path $targetPath '.chronos\scripts\Start-ChronosBuild.ps1'

# Preview only. Nothing starts.
& $launcherPath -TargetPath $targetPath -ThroughStage 10 -Resume -DryRun

# Actual background continuation; account usage begins when a stage starts.
& $launcherPath -TargetPath $targetPath -ThroughStage 10 -Resume
```

For a newly initialized repository with no earlier stage execution, omit `-Resume`. Add `-Model gpt-6-astra` only if you want that explicit model override.

The launcher prints the exact progress log path. Open that path, or inspect the newest launch directory:

```powershell
$launchPath = Get-ChildItem -LiteralPath (Join-Path $targetPath '.chronos\runs') -Directory -Filter 'launch-*' |
    Sort-Object Name -Descending | Select-Object -First 1
Get-Content -LiteralPath (Join-Path $launchPath.FullName 'runner.stdout.log') -Wait
```

Stop viewing the log with Ctrl+C; that does not stop the background build. Request a graceful stop **before the next stage** without interrupting the current agent:

```powershell
$controlPath = Join-Path $targetPath '.chronos\state\control.json'
'{"stop_before_next_stage":true}' | Set-Content -LiteralPath $controlPath -Encoding utf8
```

Before resuming, set the flag to `false` and launch with `-Resume`. The flag does not impose a time limit on the currently running stage. In a foreground run, Ctrl+C interrupts the runner; inspect any still-running child agent before starting a replacement. No claim of reliable process-tree termination is made by these scripts.

## Failure and resume behavior

The runner holds an exclusive file lock so two stage runners cannot execute the same target concurrently. It saves per-stage attempts, agent exit codes, three gate results, prompt hashes and log locations. The lock file can remain on disk after a process ends; its existence alone is not a running lock.

`-Resume` checks that completed stages form a contiguous prefix and that their successful validation records still match this package. It reruns the three Cargo gates on the current workspace before skipping that prefix. If those gates fail—such as after an interrupted later attempt—it replays the last completed stage to repair the checkpoint, then continues. Failed or interrupted stages are never marked complete. If all requested stages were completed and the fresh checks pass, no new agent is launched.

An agent failure, missing final report, modified package input or failed gate stops the run. Logs identify the last attempted stage. Automatic approval review can reject an operation; the scripts do not suppress that decision or retry with elevated privileges. Resolve the concrete cause, retain the logs, and resume. A malformed state record or missing validation log fails closed rather than guessing completion.

Do not rerun initialization on a partially initialized or completed target: it intentionally refuses to overwrite it. Inspect an initialization failure and either finish the known setup manually or choose another empty target. No script deletes an existing directory.

## What was validated for these scripts

Validation completed during packaging: all five PowerShell files parsed; initializer `-WhatIf` created no target; a real new smoke repository initialized and its copied-input hashes verified; foreground/background `-DryRun` created no execution state; nonempty and outside-workspace targets were refused. Local mocked executables then verified that agent exit 7 stops a stage, agent exit 0 cannot hide Cargo test exit 23, all three passing gates record completion, and resume revalidates an already completed requested stage without launching another agent. The mock also checked stdin prompt delivery and that no default model override was inserted. [Script validation results](validation/scripts-results.json).

No real Codex agent or production Cargo build was launched by these pipeline tests. The live paid-agent path and a complete ten-stage production build remain untested. Before unattended use on another machine, run the one-stage foreground trial above to validate authentication, approval routing, local tool availability and that machine's native build environment.
