function New-TowavueUpdateRegistrationRecord($Plan,[hashtable]$Registration) {
    if ($Registration.Count -ne 2 -or -not $Registration.ContainsKey('RegistrySubKey') -or -not $Registration.ContainsKey('ShortcutPath')) { throw 'Registration requires only RegistrySubKey and ShortcutPath.' }
    . (Join-Path $PSScriptRoot '../packaging/windows/registration-state.ps1')
    $arguments = @{
        InstallDirectory=$Plan.install_directory
        OwnershipId=$Plan.installed_ownership_id
        SizeKiB=1
        RegistrySubKey=$Registration.RegistrySubKey
        ShortcutPath=$Registration.ShortcutPath
    }
    # Reuse namespace/path/ownership guards before opening the existing test or
    # production key. Missing registration is not permission to recreate it.
    Invoke-TowavueRegistration @arguments -Mode VerifyRemoval | Out-Null
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
    $key = $null
    try {
        $key = $base.OpenSubKey($arguments.RegistrySubKey)
        if (-not $key) { throw 'The existing update registration is missing.' }
        if ($key.GetValue('TowavueOwnershipId') -cne $Plan.installed_ownership_id -or $key.GetValueKind('EstimatedSize') -ne 'DWord') { throw 'Previous update registration changed or has an invalid size type.' }
        $arguments.PreviousSizeKiB = $key.GetValue('EstimatedSize')
    } finally { if ($key) { $key.Dispose() }; $base.Dispose() }
    $arguments.PreviousOwnershipId = $Plan.installed_ownership_id
    $arguments.OwnershipId = $Plan.incoming_ownership_id
    $arguments.SizeKiB = [int][Math]::Ceiling($Plan.incoming_bytes / 1024)
    Invoke-TowavueRegistration @arguments -Mode VerifyUpdate | Out-Null
    return [pscustomobject]$arguments
}

function Invoke-TowavueUpdateRegistration($Record,[string]$Direction,[switch]$VerifyOnly) {
    . (Join-Path $PSScriptRoot '../packaging/windows/registration-state.ps1')
    $arguments = @{}
    foreach ($name in @('InstallDirectory','RegistrySubKey','ShortcutPath','OwnershipId','SizeKiB','PreviousOwnershipId','PreviousSizeKiB')) { $arguments[$name] = $Record.$name }
    if ($Direction -eq 'Rollback') {
        $arguments.OwnershipId = $Record.PreviousOwnershipId
        $arguments.SizeKiB = $Record.PreviousSizeKiB
        $arguments.PreviousOwnershipId = $Record.OwnershipId
        $arguments.PreviousSizeKiB = $Record.SizeKiB
    }
    $mode = if ($VerifyOnly) { 'VerifyUpdate' } else { 'Update' }
    Invoke-TowavueRegistration @arguments -Mode $mode | Out-Null
}
