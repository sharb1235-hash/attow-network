#!/usr/bin/env pwsh
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Invoke-CargoCheck {
    param([Parameter(Mandatory = $true)][string[]]$CargoArguments)

    Write-Host ('> cargo +1.95.0 ' + ($CargoArguments -join ' '))
    & cargo '+1.95.0' @CargoArguments
    if ($LASTEXITCODE -ne 0) {
        throw ('cargo ' + ($CargoArguments -join ' ') + ' failed with exit code ' + $LASTEXITCODE)
    }
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $repositoryRoot
try {
    Invoke-CargoCheck -CargoArguments @('run', '--package', 'xtask', '--locked', '--', 'verify-repository')
    Invoke-CargoCheck -CargoArguments @('fmt', '--all', '--check')
    Invoke-CargoCheck -CargoArguments @('test', '--workspace', '--all-targets', '--locked')
    Invoke-CargoCheck -CargoArguments @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings')
    Invoke-CargoCheck -CargoArguments @('doc', '--workspace', '--no-deps', '--locked')
}
finally {
    Pop-Location
}
