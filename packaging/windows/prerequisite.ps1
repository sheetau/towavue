[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Inspect','Install')][string]$Mode,
    [Parameter(Mandatory)][string]$PackagePath
)

$ErrorActionPreference = 'Stop'
try {
    $reader = Join-Path $PSScriptRoot 'get-vc-redist-status.ps1'
    $state = & $reader -PackagePath $PackagePath | ConvertFrom-Json
    Write-Output ($state | ConvertTo-Json -Compress)
    if ($state.state -eq 'satisfied') { exit 0 }
    if ($state.state -ne 'required') { exit 20 }
    if ($Mode -eq 'Inspect') { exit 10 }

    # Only the installer's explicit confirmation reaches this branch. The original
    # interactive Microsoft UI owns consent; no passive/quiet mode or forced restart.
    $process = Start-Process -FilePath $PackagePath -ArgumentList @('/install','/norestart') -Verb RunAs -WindowStyle Normal -WorkingDirectory $PSScriptRoot -PassThru
    # Cache the process handle before it exits so Windows PowerShell retains ExitCode.
    [void]$process.Handle
    Write-Output "VC package PID $($process.Id), start UTC $($process.StartTime.ToUniversalTime().ToString('o')). Waiting for its original UI."
    $process.WaitForExit()
    $code = $process.ExitCode
    Write-Output "VC package exit code: $code"
    $state = & $reader -PackagePath $PackagePath | ConvertFrom-Json
    Write-Output ($state | ConvertTo-Json -Compress)
    if ($state.state -ne 'satisfied' -or $code -notin @(0,3010)) { exit 20 }
    exit $code
}
catch {
    Write-Output "Prerequisite check or installation stopped: $($_.Exception.Message)"
    exit 20
}
