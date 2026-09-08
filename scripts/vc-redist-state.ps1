function Get-VcRedistState {
    param([AllowNull()]$Snapshot, [Parameter(Mandatory = $true)][version]$MinimumVersion)

    if ($MinimumVersion.Major -ne 14 -or $MinimumVersion.Revision -lt 0) { throw 'Use a complete v14 prerequisite version.' }
    if ($null -eq $Snapshot) {
        return [pscustomobject]@{state='required';reason='not_registered';installed_version=$null}
    }
    if ($Snapshot.Installed -isnot [int] -or $Snapshot.Installed -notin @(0,1)) {
        return [pscustomobject]@{state='unknown';reason='invalid_installed_flag';installed_version=$null}
    }
    if ($Snapshot.Installed -eq 0) {
        return [pscustomobject]@{state='required';reason='not_installed';installed_version=$null}
    }
    foreach ($name in @('Major','Minor','Bld','Rbld')) {
        if ($Snapshot[$name] -isnot [int] -or $Snapshot[$name] -lt 0 -or $Snapshot[$name] -gt 65535) {
            return [pscustomobject]@{state='unknown';reason='invalid_version_fields';installed_version=$null}
        }
    }
    $numeric = [version]::new($Snapshot.Major,$Snapshot.Minor,$Snapshot.Bld,$Snapshot.Rbld)
    $textVersion = $null
    if ($Snapshot.Version -isnot [string] -or $Snapshot.Version -notmatch '^[vV]?[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$' -or
        -not [version]::TryParse($Snapshot.Version.TrimStart('v','V'),[ref]$textVersion) -or $numeric -ne $textVersion) {
        return [pscustomobject]@{state='unknown';reason='inconsistent_version';installed_version=$null}
    }
    if ($numeric.Major -ne 14) {
        return [pscustomobject]@{state='unknown';reason='unverified_abi_major';installed_version=$numeric.ToString()}
    }
    if ($numeric -lt $MinimumVersion) {
        return [pscustomobject]@{state='required';reason='older_version';installed_version=$numeric.ToString()}
    }
    return [pscustomobject]@{state='satisfied';reason='compatible_version_registered';installed_version=$numeric.ToString()}
}
