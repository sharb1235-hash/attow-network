#Requires -Version 7.2
Set-StrictMode -Version Latest

function Get-ChronosFullPath {
    param([Parameter(Mandatory)][string] $Path)
    [System.IO.Path]::GetFullPath($Path)
}

function Assert-ChronosChildPath {
    param([Parameter(Mandatory)][string] $Path, [Parameter(Mandatory)][string] $Root)
    $full = Get-ChronosFullPath $Path
    $rootFull = (Get-ChronosFullPath $Root).TrimEnd([char[]]@('\', '/'))
    $prefix = $rootFull + [System.IO.Path]::DirectorySeparatorChar
    if (-not $full.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Target must be a child of the workspace: $rootFull"
    }
    $cursor = $full
    while (-not [string]::IsNullOrEmpty($cursor)) {
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Refusing a path containing a junction or symbolic link: $cursor"
            }
        }
        $parent = [System.IO.Directory]::GetParent($cursor)
        if ($null -eq $parent) { break }
        $cursor = $parent.FullName
    }
    return $full
}

function Get-ChronosHash {
    param([Parameter(Mandatory)][string] $Path)
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Write-ChronosJson {
    param([Parameter(Mandatory)][string] $Path, [Parameter(Mandatory)] $Value)
    $json = $Value | ConvertTo-Json -Depth 40
    [System.IO.File]::WriteAllText($Path, $json + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
}

function Read-ChronosPackage {
    param([Parameter(Mandatory)][string] $PackagePath)
    $package = (Resolve-Path -LiteralPath $PackagePath).Path
    $documents = @('01-technical-specification.md', '02-repository-blueprint.md', '03-README.md', '05-autonomous-execution.md')
    foreach ($name in $documents) {
        if (-not (Test-Path -LiteralPath (Join-Path $package $name) -PathType Leaf)) { throw "Missing package document: $name" }
    }
    $promptDir = Join-Path $package 'prompts'
    $allPrompts = @(Get-ChildItem -LiteralPath $promptDir -File -Filter '*.md')
    if ($allPrompts.Count -ne 10) { throw 'Expected exactly ten Markdown files in prompts/.' }
    $prompts = @()
    foreach ($number in 1..10) {
        $prefix = '{0:D2}-' -f $number
        $matches = @($allPrompts | Where-Object { $_.Name.StartsWith($prefix, [StringComparison]::Ordinal) })
        if ($matches.Count -ne 1) { throw "Expected exactly one prompt with prefix $prefix" }
        if ([string]::IsNullOrWhiteSpace([IO.File]::ReadAllText($matches[0].FullName))) { throw "Empty prompt: $($matches[0].Name)" }
        $prompts += $matches[0]
    }
    if ($prompts[0].Name -ne '01-foundation.md') { throw 'Stage 1 must be named 01-foundation.md.' }
    [pscustomobject]@{ Root = $package; Documents = $documents; Prompts = $prompts }
}

function Read-ChronosManifest {
    param([Parameter(Mandatory)][string] $TargetPath)
    $target = (Resolve-Path -LiteralPath $TargetPath).Path
    $manifestPath = Join-Path $target '.chronos/manifest.json'
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw 'Target was not initialized by Initialize-Chronos.ps1.' }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json -AsHashtable
    if ($manifest.schema -ne 'chronos.execution-package.v1') { throw 'Unsupported package manifest.' }
    if ((Get-ChronosFullPath $manifest.target_path) -ne (Get-ChronosFullPath $target)) { throw 'Package target differs from its initialized path.' }
    $null = Assert-ChronosChildPath -Path $target -Root $manifest.workspace_root
    if (@($manifest.prompts).Count -ne 10) { throw 'Manifest must contain ten prompts.' }
    foreach ($entry in $manifest.files) {
        $path = Assert-ChronosChildPath -Path (Join-Path $target $entry.relative_path) -Root $target
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing immutable package input: $($entry.relative_path)" }
        if ((Get-ChronosHash $path) -ne $entry.sha256) { throw "Package input changed: $($entry.relative_path). Reconcile the execution plan before continuing." }
    }
    [pscustomobject]@{ Target = $target; Manifest = $manifest; ManifestPath = $manifestPath; ManifestHash = (Get-ChronosHash $manifestPath) }
}

function Get-ChronosGateDefinitions {
    @(
        @{ name = 'fmt'; args = @('fmt', '--all', '--check') },
        @{ name = 'test'; args = @('test', '--workspace', '--all-targets', '--locked') },
        @{ name = 'clippy'; args = @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings') }
    )
}

function Invoke-ChronosGates {
    param([Parameter(Mandatory)][string] $TargetPath, [Parameter(Mandatory)][string] $LogPrefix)
    $results = @()
    $cargo = (Get-Command cargo -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
    Push-Location -LiteralPath $TargetPath
    try {
        foreach ($gate in (Get-ChronosGateDefinitions)) {
            $logPath = "$LogPrefix-$($gate.name).log"
            Write-Host "Gate: cargo $($gate.args -join ' ')"
            & $cargo @($gate.args) *> $logPath
            $code = $LASTEXITCODE
            $results += @{ name = $gate.name; exit_code = $code; log_path = $logPath; command = 'cargo ' + ($gate.args -join ' ') }
            if ($code -ne 0) { break }
        }
    } finally { Pop-Location }
    return ,$results
}

function Test-ChronosGatesPassed {
    param($Gates)
    $expected = @('fmt', 'test', 'clippy')
    if (@($Gates).Count -ne $expected.Count) { return $false }
    for ($i = 0; $i -lt $expected.Count; $i++) {
        if ($Gates[$i].name -ne $expected[$i] -or $Gates[$i].exit_code -ne 0) { return $false }
    }
    return $true
}

function ConvertTo-ChronosProcessArgument {
    param([Parameter(Mandatory)][AllowEmptyString()][string] $Value)
    # Windows CommandLineToArgvW quoting. No shell expression evaluation.
    '"' + [regex]::Replace([regex]::Replace($Value, '(\\*)"', '$1$1\"'), '(\\+)$', '$1$1') + '"'
}
