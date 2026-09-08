[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Inspect','Apply','Rollback')][string]$Mode,
    [Parameter(Mandatory)][string]$InstallDirectory,
    [string]$IncomingPayloadDirectory,
    [string]$IncomingOwnershipId,
    [string]$NewUninstaller
)

$ErrorActionPreference = 'Stop'
try {
    . (Join-Path $PSScriptRoot 'registration-state.ps1')
    $programs = [Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)
    if (-not $programs) { throw 'The per-user Programs folder is unavailable.' }
    $registration = @{
        RegistrySubKey='Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue-evaluation'
        ShortcutPath=(Join-Path $programs 'towavue (local evaluation).lnk')
    }
    Assert-TowavueRegistrationLocation $InstallDirectory $registration.RegistrySubKey $registration.ShortcutPath
    if ($Mode -eq 'Inspect') {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
        $key = $null
        try {
            $key = $base.OpenSubKey($registration.RegistrySubKey)
            if (-not $key -or $key.GetValueKind('InstallLocation') -ne 'String' -or $key.GetValue('InstallLocation') -ine $InstallDirectory) { throw 'No matching installation registration was found.' }
            if ($key.GetValueNames() -contains 'TowavuePendingUpdate') {
                Write-Output 'An interrupted update requires recovery before another update.'
                exit 11
            }
            if ($key.GetValueKind('TowavueOwnershipId') -ne 'String' -or $key.GetValue('TowavueOwnershipId') -cnotmatch '^towavue-local-[0-9a-f]{64}$') { throw 'The installation ownership record is invalid.' }
            Write-Output 'Existing registration found. All installed files will be verified before updating.'
            exit 10
        } finally { if ($key) { $key.Dispose() }; $base.Dispose() }
    }
    . (Join-Path $PSScriptRoot '../../scripts/setup-registered-update.ps1')
    $result = Invoke-TowavueRegisteredUpdate -Mode $Mode -InstallDirectory $InstallDirectory -Registration $registration -IncomingPayloadDirectory $IncomingPayloadDirectory -IncomingOwnershipId $IncomingOwnershipId -NewUninstaller $NewUninstaller
    if ($result.state -eq 'no_pending_update') { throw 'The pending operation already changed or completed. Inspect the installation again.' }
    Write-Output "Update operation completed: $($result.state). Recovery files are retained; no application was launched."
    exit 0
}
catch { Write-Output "Update operation stopped: $($_.Exception.Message) Retain the installation and recovery files, close users of these files and retry Setup."; exit 20 }
