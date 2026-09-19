[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Inspect','Install','VerifyRemoval','Remove')][string]$Mode,
    [Parameter(Mandatory)][string]$InstallDirectory,
    [Parameter(Mandatory)][string]$OwnershipId,
    [Parameter(Mandatory)][int]$SizeKiB,
    [string]$ProductVersion
)

$ErrorActionPreference = 'Stop'
try {
    . (Join-Path $PSScriptRoot 'registration-state.ps1')
    $programs = [Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)
    if (-not $programs) { throw 'The per-user Programs folder is unavailable.' }
    $release = $OwnershipId -cmatch '^towavue-release-[0-9a-f]{64}$'
    $registry = if ($release) { 'Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue' } else { 'Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue-evaluation' }
    $shortcut = if ($release) { 'towavue.lnk' } else { 'towavue (local evaluation).lnk' }
    Invoke-TowavueRegistration -Mode $Mode -InstallDirectory $InstallDirectory -OwnershipId $OwnershipId -SizeKiB $SizeKiB -ProductVersion $ProductVersion `
        -RegistrySubKey $registry `
        -ShortcutPath (Join-Path $programs $shortcut)
    exit 0
}
catch { Write-Output "Application registration stopped: $($_.Exception.Message)"; exit 20 }
