[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Directory,
    [Parameter(Mandatory = $true, ParameterSetName = 'Integrity')]
    [switch]$IntegrityOnly,
    [Parameter(Mandatory = $true, ParameterSetName = 'Authenticated')]
    [switch]$RequireSignatures
)
$ErrorActionPreference = 'Stop'
$candidateDirectory = [System.IO.Path]::GetFullPath($Directory)
$repositoryDirectory = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$verificationMode = if ($RequireSignatures) { '--require-signatures' } else { '--integrity-only' }
Push-Location -LiteralPath $repositoryDirectory
try {
    & cargo +1.95.0 run --locked -p xtask -- release-verify --dir $candidateDirectory $verificationMode
    $verificationExit = $LASTEXITCODE
} finally {
    Pop-Location
}
exit $verificationExit
