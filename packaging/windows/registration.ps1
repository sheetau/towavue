[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Inspect','Install','VerifyRemoval','Remove')][string]$Mode,
    [Parameter(Mandatory)][string]$InstallDirectory,
    [Parameter(Mandatory)][string]$OwnershipId,
    [Parameter(Mandatory)][int]$SizeKiB
)

$ErrorActionPreference = 'Stop'
try {
    . (Join-Path $PSScriptRoot 'registration-state.ps1')
    $programs = [Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)
    if (-not $programs) { throw 'The per-user Programs folder is unavailable.' }
    Invoke-TowavueRegistration -Mode $Mode -InstallDirectory $InstallDirectory -OwnershipId $OwnershipId -SizeKiB $SizeKiB `
        -RegistrySubKey 'Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue-evaluation' `
        -ShortcutPath (Join-Path $programs 'towavue (local evaluation).lnk')
    exit 0
}
catch { Write-Output "Application registration stopped: $($_.Exception.Message)"; exit 20 }
