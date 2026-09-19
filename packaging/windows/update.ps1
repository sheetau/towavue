[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Inspect','Apply','Rollback')][string]$Mode,
    [Parameter(Mandatory)][string]$InstallDirectory,
    [string]$IncomingPayloadDirectory,
    [string]$IncomingOwnershipId,
    [string]$NewUninstaller,
    [string]$ProductVersion
)

$ErrorActionPreference = 'Stop'
try {
    . (Join-Path $PSScriptRoot 'registration-state.ps1')
    $programs = [Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)
    if (-not $programs) { throw 'The per-user Programs folder is unavailable.' }
    if ($IncomingOwnershipId -cnotmatch '^towavue-(local|release)-[0-9a-f]{64}$') { throw 'An explicit incoming payload identity is required.' }
    $release = $IncomingOwnershipId.StartsWith('towavue-release-',[StringComparison]::Ordinal)
    if ($release) { Assert-TowavueProductVersion $ProductVersion }
    $registration = if ($release) {
        @{RegistrySubKey='Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue';ShortcutPath=(Join-Path $programs 'towavue.lnk')}
    } else {
        @{RegistrySubKey='Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue-evaluation';ShortcutPath=(Join-Path $programs 'towavue (local evaluation).lnk')}
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
            $identityPattern = if ($release) { '^towavue-release-[0-9a-f]{64}$' } else { '^towavue-local-[0-9a-f]{64}$' }
            if ($key.GetValueKind('TowavueOwnershipId') -ne 'String' -or $key.GetValue('TowavueOwnershipId') -cnotmatch $identityPattern) { throw 'The installation ownership record is invalid.' }
            if ($release) {
                if ($key.GetValueKind('DisplayVersion') -ne 'String') { throw 'The registered product version has an invalid type.' }
                Assert-TowavueProductVersion $key.GetValue('DisplayVersion')
                if ([version]$ProductVersion -le [version]$key.GetValue('DisplayVersion')) { throw 'This Setup is not newer than the installed production version.' }
            }
            Write-Output 'Existing registration found. All installed files will be verified before updating.'
            exit 10
        } finally { if ($key) { $key.Dispose() }; $base.Dispose() }
    }
    if ($release -and $Mode -eq 'Apply') {
        $inventory = Get-Content -LiteralPath (Join-Path $IncomingPayloadDirectory 'licenses/INSTALLED-FILES.json') -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($inventory.schema_version -ne 2 -or $inventory.product_version -cne $ProductVersion) { throw 'Setup and incoming payload versions differ.' }
    }
    . (Join-Path $PSScriptRoot '../../scripts/setup-registered-update.ps1')
    $result = Invoke-TowavueRegisteredUpdate -Mode $Mode -InstallDirectory $InstallDirectory -Registration $registration -IncomingPayloadDirectory $IncomingPayloadDirectory -IncomingOwnershipId $IncomingOwnershipId -NewUninstaller $NewUninstaller -Verbose
    if ($result.state -eq 'no_pending_update') { throw 'The pending operation already changed or completed. Inspect the installation again.' }
    Write-Output "Update operation completed: $($result.state). Recovery files are retained; no application was launched."
    exit 0
}
catch { Write-Output "Update operation stopped: $($_.Exception.Message) Retain the installation and recovery files, close users of these files and retry Setup."; exit 20 }
