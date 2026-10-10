#Requires -Version 7.2
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [Parameter(Mandatory)][string] $TargetPath,
    [ValidateRange(1, 10)][int] $ThroughStage = 10,
    [ValidatePattern('^[A-Za-z0-9._:/-]*$')][string] $Model,
    [switch] $Resume,
    [switch] $DryRun
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$OutputEncoding = [Text.UTF8Encoding]::new($false)
. (Join-Path $PSScriptRoot 'Chronos.Common.ps1')
$verified = Read-ChronosManifest $TargetPath
$target = $verified.Target
$manifest = $verified.Manifest
$statePath = Join-Path $target '.chronos/state/stages.json'
$state = $null
$completed = 0
if (Test-Path -LiteralPath $statePath) {
    if (-not $Resume) { throw 'Execution state already exists. Use -Resume to validate it and continue.' }
    $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json -AsHashtable
    if ($state.schema -ne 'chronos.stage-run.v1' -or $state.manifest_sha256 -ne $verified.ManifestHash) { throw 'Execution state does not match this package manifest.' }
    if (@($state.stages).Count -ne 10) { throw 'Execution state must contain ten stages.' }
    $seenIncomplete = $false
    for ($i = 0; $i -lt 10; $i++) {
        $record = $state.stages[$i]
        if ($record.stage -ne ($i + 1) -or $record.prompt_sha256 -ne $manifest.prompts[$i].sha256) { throw "Stage $($i + 1) identity mismatch." }
        if ($record.status -eq 'completed') {
            if ($seenIncomplete) { throw 'Completed stages must form a contiguous prefix.' }
            if ($record.codex_exit_code -ne 0 -or -not (Test-ChronosGatesPassed $record.gates)) { throw "Stage $($i + 1) has no successful agent and gate record." }
            foreach ($gate in $record.gates) {
                if (-not (Test-Path -LiteralPath $gate.log_path -PathType Leaf)) { throw "Missing validation log: $($gate.log_path)" }
            }
            $completed++
        } else { $seenIncomplete = $true }
    }
} else {
    $records = @()
    foreach ($prompt in $manifest.prompts) {
        $records += @{ stage = $prompt.stage; prompt_sha256 = $prompt.sha256; status = 'pending'; attempts = @(); gates = @(); codex_exit_code = $null }
    }
    $state = @{ schema = 'chronos.stage-run.v1'; manifest_sha256 = $verified.ManifestHash; target_path = $target; stages = $records }
}
Write-Host "Target: $target"
Write-Host "Validated completion records: $completed of 10. Requested through stage: $ThroughStage."
Write-Host 'Agent flags: exec --cd TARGET --sandbox workspace-write --approve-for-me --json --color never --output-last-message FILE -'
if ($Model) { Write-Host "Explicit model override: $Model" } else { Write-Host 'Model: existing Codex configuration; no override.' }
foreach ($gate in (Get-ChronosGateDefinitions)) { Write-Host "Gate: cargo $($gate.args -join ' ')" }
if ($DryRun -or -not $PSCmdlet.ShouldProcess($target, 'Run sequential Codex stages and independent Cargo gates; these agent runs consume account usage')) { return }
$codex = (Get-Command codex -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$null = Get-Command cargo -CommandType Application -ErrorAction Stop
$git = (Get-Command git -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$gitRoot = & $git -C $target rev-parse --show-toplevel
if ($LASTEXITCODE -ne 0 -or (Get-ChronosFullPath ($gitRoot | Select-Object -Last 1)) -ne (Get-ChronosFullPath $target)) { throw 'Target must be its own Git repository.' }
$lockPath = Join-Path $target '.chronos/state/run.lock'
try { $runLock = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
catch { throw 'Another stage runner may be active. Its exclusive run lock could not be acquired.' }
try {
    # Re-read after obtaining the lock, so two concurrent invocations cannot use stale state.
    if (Test-Path -LiteralPath $statePath) {
        $currentState = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json -AsHashtable
        if (($currentState | ConvertTo-Json -Depth 40 -Compress) -ne ($state | ConvertTo-Json -Depth 40 -Compress)) {
            throw 'Execution state changed while the runner was starting. Re-run with -Resume.'
        }
    }
    $runId = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ') + '-' + [Guid]::NewGuid().ToString('N').Substring(0, 8)
    $runDir = Join-Path $target ".chronos/runs/$runId"
    $null = New-Item -ItemType Directory -Path $runDir
    Write-ChronosJson -Path $statePath -Value $state
    $start = $completed
    if ($Resume -and $completed -gt 0) {
        Write-Host 'Revalidating the current workspace before skipping completed stages.'
        $preflight = Invoke-ChronosGates -TargetPath $target -LogPrefix (Join-Path $runDir 'resume-check')
        Write-ChronosJson -Path (Join-Path $runDir 'resume-check.json') -Value $preflight
        if (-not (Test-ChronosGatesPassed $preflight)) {
            $start = $completed - 1
            if (($start + 1) -gt $ThroughStage) { throw 'Resume validation failed beyond the requested stage limit. Resume through the last completed stage to repair it.' }
            Write-Host "Current workspace gates failed; replaying stage $($start + 1) to repair the checkpoint before proceeding."
            $state.stages[$start].status = 'needs-repair'
            Write-ChronosJson -Path $statePath -Value $state
        }
    }
    for ($index = $start; $index -lt $ThroughStage; $index++) {
        $control = Get-Content -LiteralPath (Join-Path $target '.chronos/state/control.json') -Raw | ConvertFrom-Json -AsHashtable
        if ($control.stop_before_next_stage -eq $true) {
            Write-Host 'Stopped at a stage boundary as requested in control.json.'
            break
        }
        $null = Read-ChronosManifest $target
        $number = $index + 1
        $definition = $manifest.prompts[$index]
        $promptText = [IO.File]::ReadAllText((Join-Path $target $definition.relative_path))
        $prefix = Join-Path $runDir ('stage-{0:D2}' -f $number)
        $eventsPath = "$prefix.events.jsonl"
        $errorsPath = "$prefix.stderr.log"
        $lastMessagePath = "$prefix.last-message.md"
        $fullPrompt = @"
Execute ChronosOS stage $number of 10 in this initialized repository.
Read docs/chronos/01-technical-specification.md and docs/chronos/02-repository-blueprint.md first.
Respect the stage scope and complete its explicit acceptance requirements. If this is a retry,
inspect existing work and repair it; do not recreate or overwrite unrelated completed work.
Restore failing workspace gates before claiming success. Never disable or weaken a gate to pass it.
Preserve .chronos/prompts, .chronos/scripts, .chronos/manifest.json, docs/chronos, and runner state/logs.
Do not modify these orchestration inputs or mark your own runner stage completed.
Do not bypass approval review, sandbox restrictions, or hook trust. Explain any blocking decision.
Do not publish, deploy, purchase services, or send messages as part of this implementation stage.
Record actual checks, remaining limitations, and stage-specific acceptance evidence in your final response.

$promptText
"@
        $record = $state.stages[$index]
        $attempt = @{ started_utc = [DateTime]::UtcNow.ToString('o'); run_id = $runId; events_path = $eventsPath; stderr_path = $errorsPath; last_message_path = $lastMessagePath; model_override = $Model; status = 'running' }
        $record.attempts += $attempt
        $record.status = 'running'
        $record.codex_exit_code = $null
        $record.gates = @()
        Write-ChronosJson -Path $statePath -Value $state
        $arguments = @('exec', '--cd', $target, '--sandbox', 'workspace-write', '--approve-for-me', '--json', '--color', 'never', '--output-last-message', $lastMessagePath)
        if ($Model) { $arguments += @('--model', $Model) }
        $arguments += '-'
        Write-Host "Starting stage $number. Events: $eventsPath"
        $fullPrompt | & $codex @arguments 1> $eventsPath 2> $errorsPath
        $record.codex_exit_code = $LASTEXITCODE
        $attempt.codex_exit_code = $LASTEXITCODE
        if ($record.codex_exit_code -ne 0) {
            $record.status = 'agent-failed'; $attempt.status = 'agent-failed'
            $attempt.finished_utc = [DateTime]::UtcNow.ToString('o')
            Write-ChronosJson -Path $statePath -Value $state
            throw "Stage $number agent exited $($record.codex_exit_code). Inspect $errorsPath and $eventsPath; resolve the reported issue, then use -Resume."
        }
        if (-not (Test-Path -LiteralPath $lastMessagePath -PathType Leaf) -or [string]::IsNullOrWhiteSpace([IO.File]::ReadAllText($lastMessagePath))) {
            $record.status = 'missing-final-report'; $attempt.status = 'missing-final-report'
            Write-ChronosJson -Path $statePath -Value $state
            throw "Stage $number returned no final report. It is not marked complete."
        }
        $null = Read-ChronosManifest $target
        $record.gates = Invoke-ChronosGates -TargetPath $target -LogPrefix $prefix
        $attempt.gates = $record.gates
        $attempt.finished_utc = [DateTime]::UtcNow.ToString('o')
        if (-not (Test-ChronosGatesPassed $record.gates)) {
            $record.status = 'gate-failed'; $attempt.status = 'gate-failed'
            Write-ChronosJson -Path $statePath -Value $state
            throw "Stage $number failed an independent Cargo gate. Inspect $prefix-*.log, then use -Resume."
        }
        $record.status = 'completed'; $attempt.status = 'completed'
        $record.completed_utc = $attempt.finished_utc
        Write-ChronosJson -Path $statePath -Value $state
        Write-Host "Stage $number completed and passed fmt, test, and clippy."
    }
    Write-Host "Runner finished. Completion records: $statePath"
} finally { $runLock.Dispose() }
