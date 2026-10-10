#Requires -Version 7.2
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [Parameter(Mandatory)][string] $TargetPath,
    [string] $PackagePath = (Split-Path -Parent $PSScriptRoot),
    [string] $WorkspaceRoot = ([IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))),
    [switch] $DryRun
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Chronos.Common.ps1')
$package = Read-ChronosPackage $PackagePath
$workspace = (Resolve-Path -LiteralPath $WorkspaceRoot).Path
$target = Assert-ChronosChildPath -Path $TargetPath -Root $workspace
if (Test-Path -LiteralPath $target) {
    if (-not (Test-Path -LiteralPath $target -PathType Container)) { throw 'Target exists and is not a directory.' }
    if (@(Get-ChildItem -LiteralPath $target -Force).Count -ne 0) { throw 'Target must be new or empty; no existing files will be overwritten.' }
}
$requiredScripts = @('Chronos.Common.ps1', 'Initialize-Chronos.ps1', 'Run-ChronosStages.ps1', 'Start-ChronosBuild.ps1', 'Verify-Package.ps1')
foreach ($name in $requiredScripts) {
    if (-not (Test-Path -LiteralPath (Join-Path $package.Root "scripts/$name") -PathType Leaf)) { throw "Missing script: $name" }
}
Write-Host "Initialize a new repository at: $target"
Write-Host "Copy ten prompts, execution scripts, and design documents from: $($package.Root)"
if ($DryRun -or -not $PSCmdlet.ShouldProcess($target, 'Create package inputs and run git init only in this empty target')) { return }
$git = (Get-Command git -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$null = New-Item -ItemType Directory -Path $target -Force
foreach ($relative in @('.chronos/prompts', '.chronos/scripts', '.chronos/state', '.chronos/runs', 'docs/chronos')) {
    $null = New-Item -ItemType Directory -Path (Join-Path $target $relative) -Force
}
$files = @()
$prompts = @()
foreach ($item in $package.Prompts) {
    $relative = '.chronos/prompts/' + $item.Name
    Copy-Item -LiteralPath $item.FullName -Destination (Join-Path $target $relative)
    $hash = Get-ChronosHash (Join-Path $target $relative)
    $files += @{ relative_path = $relative; sha256 = $hash }
    $prompts += @{ stage = $prompts.Count + 1; relative_path = $relative; sha256 = $hash }
}
foreach ($name in $package.Documents) {
    $relative = 'docs/chronos/' + $name
    Copy-Item -LiteralPath (Join-Path $package.Root $name) -Destination (Join-Path $target $relative)
    $files += @{ relative_path = $relative; sha256 = (Get-ChronosHash (Join-Path $target $relative)) }
}
$validationFiles = @(
    'validation/dependencies.md', 'validation/dependencies.json', 'validation/reference-results.json', 'validation/scripts-results.json', 'validation/auxiliary-tools.json',
    'validation/runtime-probe/Cargo.toml', 'validation/runtime-probe/Cargo.lock',
    'validation/runtime-probe/README.md', 'validation/runtime-probe/verify-existing.ps1',
    'validation/runtime-probe/src/main.rs'
)
foreach ($name in $validationFiles) {
    $source = Join-Path $package.Root $name
    if (Test-Path -LiteralPath $source -PathType Leaf) {
        $relative = 'docs/chronos/' + $name
        $destination = Join-Path $target $relative
        $null = New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force
        Copy-Item -LiteralPath $source -Destination $destination
        $files += @{ relative_path = $relative; sha256 = (Get-ChronosHash $destination) }
    }
}
Copy-Item -LiteralPath (Join-Path $package.Root '03-README.md') -Destination (Join-Path $target 'README.md')
foreach ($name in $requiredScripts) {
    $relative = '.chronos/scripts/' + $name
    Copy-Item -LiteralPath (Join-Path $package.Root "scripts/$name") -Destination (Join-Path $target $relative)
    $files += @{ relative_path = $relative; sha256 = (Get-ChronosHash (Join-Path $target $relative)) }
}
$manifest = @{
    schema = 'chronos.execution-package.v1'; initialized_utc = [DateTime]::UtcNow.ToString('o')
    target_path = $target; workspace_root = $workspace; source_package_path = $package.Root
    prompts = $prompts; files = $files
}
Write-ChronosJson -Path (Join-Path $target '.chronos/manifest.json') -Value $manifest
Write-ChronosJson -Path (Join-Path $target '.chronos/state/control.json') -Value @{ stop_before_next_stage = $false }
[IO.File]::WriteAllText((Join-Path $target '.gitignore'), "/target/`n/.chronos/runs/`n/.chronos/state/`n", [Text.UTF8Encoding]::new($false))
& $git -C $target init
if ($LASTEXITCODE -ne 0) { throw 'git init failed. The copied files remain for inspection; nothing was deleted.' }
Write-Host 'Initialized. No Codex run, dependency installation, build, commit, or paid action was started.'
