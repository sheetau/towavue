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

    # Setup owns progress and error UI; only Windows owns the elevation prompt.
    # The verified original package runs quietly and never restarts Windows.
    $process = Start-Process -FilePath $PackagePath -ArgumentList @('/install','/quiet','/norestart') -Verb RunAs -WindowStyle Hidden -WorkingDirectory $PSScriptRoot -PassThru
    # Cache the process handle before it exits so Windows PowerShell retains ExitCode.
    [void]$process.Handle
    Write-Output "VC package PID $($process.Id), start UTC $($process.StartTime.ToUniversalTime().ToString('o')). Installing quietly; Setup is waiting for completion."
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
