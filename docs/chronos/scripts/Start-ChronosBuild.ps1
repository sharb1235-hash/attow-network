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
. (Join-Path $PSScriptRoot 'Chronos.Common.ps1')
$verified = Read-ChronosManifest $TargetPath
$target = $verified.Target
$runner = Join-Path $target '.chronos/scripts/Run-ChronosStages.ps1'
$shell = (Get-Command pwsh -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$arguments = @('-NoProfile', '-NonInteractive', '-File', $runner, '-TargetPath', $target, '-ThroughStage', [string]$ThroughStage)
if ($Model) { $arguments += @('-Model', $Model) }
if ($Resume) { $arguments += '-Resume' }
$argumentLine = ($arguments | ForEach-Object { ConvertTo-ChronosProcessArgument $_ }) -join ' '
Write-Host "Background command: $shell $argumentLine"
Write-Host 'WindowStyle=Hidden; standard output, errors and the process ID will be recorded under .chronos/runs/.'
if ($DryRun -or -not $PSCmdlet.ShouldProcess($target, 'Start a hidden background stage runner that can consume Codex account usage')) { return }
$launchId = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ') + '-' + [Guid]::NewGuid().ToString('N').Substring(0, 8)
$launchDir = Join-Path $target ".chronos/runs/launch-$launchId"
$null = New-Item -ItemType Directory -Path $launchDir
$stdoutPath = Join-Path $launchDir 'runner.stdout.log'
$stderrPath = Join-Path $launchDir 'runner.stderr.log'
$process = Start-Process -FilePath $shell -ArgumentList $argumentLine -WorkingDirectory $target -WindowStyle Hidden -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -PassThru
$receipt = @{ schema = 'chronos.background-launch.v1'; process_id = $process.Id; launched_utc = [DateTime]::UtcNow.ToString('o'); target_path = $target; stdout_path = $stdoutPath; stderr_path = $stderrPath; through_stage = $ThroughStage; resume = [bool]$Resume; model_override = $Model }
$receiptPath = Join-Path $launchDir 'launch.json'
Write-ChronosJson -Path $receiptPath -Value $receipt
Write-Host "Started process $($process.Id). Launch receipt: $receiptPath"
Write-Host "Progress: Get-Content -LiteralPath '$stdoutPath' -Wait"
