#Requires -Version 7.2
[CmdletBinding()]
param([string] $PackagePath = (Split-Path -Parent $PSScriptRoot), [string] $TargetPath)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Chronos.Common.ps1')
if ($TargetPath) {
    $verified = Read-ChronosManifest $TargetPath
    Write-Host "Initialized target inputs verified: $($verified.Target)"
} else {
    $package = Read-ChronosPackage $PackagePath
    foreach ($file in (Get-ChildItem -LiteralPath (Join-Path $package.Root 'scripts') -File -Filter '*.ps1')) {
        $tokens = $null; $parseErrors = $null
        $null = [System.Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$parseErrors)
        if ($parseErrors.Count -gt 0) { throw "PowerShell parse errors in $($file.Name): $($parseErrors -join '; ')" }
    }
    Write-Host "Package structure and PowerShell syntax verified: $($package.Root)"
}
