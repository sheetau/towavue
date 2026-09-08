# Internal registered-update entry point. Actual Setup/uninstaller lifecycle
# coordination and recovery UI must be connected before enabling installed updates.
. (Join-Path $PSScriptRoot 'setup-update-transaction.ps1')
. (Join-Path $PSScriptRoot '../packaging/windows/registration-state.ps1')
. (Join-Path $PSScriptRoot '../packaging/windows/operation-lock.ps1')

function Invoke-TowavueRegisteredUpdate {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][ValidateSet('Apply','Rollback')][string]$Mode,
        [Parameter(Mandatory)][string]$InstallDirectory,
        [Parameter(Mandatory)][hashtable]$Registration,
        [string]$IncomingPayloadDirectory,
        [string]$IncomingOwnershipId,
        [string]$NewUninstaller
    )
    if ($Registration.Count -ne 2 -or -not $Registration.ContainsKey('RegistrySubKey') -or -not $Registration.ContainsKey('ShortcutPath')) { throw 'Registration requires only RegistrySubKey and ShortcutPath.' }
    Assert-TowavueRegistrationLocation $InstallDirectory $Registration.RegistrySubKey $Registration.ShortcutPath
    Assert-LocalPath $InstallDirectory
    if ([TowavueUpdatePaths]::Expand($InstallDirectory) -ine $InstallDirectory) { throw 'Use the canonical installation directory.' }
    $binding = @{InstallDirectory=$InstallDirectory;RegistrySubKey=$Registration.RegistrySubKey;ShortcutPath=$Registration.ShortcutPath}
    # Match Setup's user/registration lease, including different destinations
    # competing for the one product registration. No wait or thread ownership.
    $lease = New-TowavueOperationLease $Registration.RegistrySubKey
    $base = $null
    $key = $null
    try {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
        $key = $base.OpenSubKey($Registration.RegistrySubKey,$true)
        if (-not $key -or $key.GetValueKind('InstallLocation') -ne 'String' -or $key.GetValue('InstallLocation') -ine $InstallDirectory) { throw 'The expected update registration is missing or belongs to another directory.' }
        $pendingName = 'TowavuePendingUpdate'
        $pending = $key.GetValueNames() -contains $pendingName
        if ($Mode -eq 'Apply') {
            if ($pending) { throw 'An update is pending. Roll back before preparing another update.' }
            $token = New-TowavueUpdateTransaction -InstallDirectory $InstallDirectory -IncomingPayloadDirectory $IncomingPayloadDirectory -IncomingOwnershipId $IncomingOwnershipId -NewUninstaller $NewUninstaller -Registration $Registration
            $saved = [ordered]@{schema_version=1;TransactionDirectory=$token.TransactionDirectory;JournalSha256=$token.JournalSha256} | ConvertTo-Json -Compress
            # Publish one complete value after the flushed immutable journal and
            # before any installed-file mutation. Failure retains recovery data.
            $key.SetValue($pendingName,$saved,[Microsoft.Win32.RegistryValueKind]::String)
            $key.Flush()
        } else {
            if (-not $pending) { return [pscustomobject]@{state='no_pending_update'} }
            if ($key.GetValueKind($pendingName) -ne 'String') { throw 'Pending update record has an invalid type; preserve it.' }
            $saved = $key.GetValue($pendingName)
            $token = $saved | ConvertFrom-Json
            if ($null -eq $token -or @($token.PSObject.Properties).Count -ne 3 -or $token.schema_version -ne 1 -or
                $token.TransactionDirectory -isnot [string] -or $token.JournalSha256 -isnot [string] -or
                $token.JournalSha256 -cnotmatch '^[0-9a-f]{64}$') { throw 'Pending update record is invalid; preserve it.' }
        }
        $result = Invoke-TowavueUpdateTransaction -Mode $Mode -TransactionDirectory $token.TransactionDirectory -JournalSha256 $token.JournalSha256 -ExpectedRegistration $binding
        if ($key.GetValueKind($pendingName) -ne 'String' -or $key.GetValue($pendingName) -cne $saved) { throw 'Pending update identity changed; preserve it.' }
        # A crash before this deletion remains recoverable, including a fully
        # applied payload. Never clear the pointer merely because recovery began.
        $key.DeleteValue($pendingName)
        $key.Flush()
        return $result
    } finally {
        if ($key) { $key.Dispose() }
        if ($base) { $base.Dispose() }
        $lease.Dispose()
    }
}
