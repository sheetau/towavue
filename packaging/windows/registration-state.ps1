function Invoke-TowavueRegistration {
    param(
        [Parameter(Mandatory)][ValidateSet('Inspect','Install','VerifyRemoval','Remove')][string]$Mode,
        [Parameter(Mandatory)][string]$InstallDirectory,
        [Parameter(Mandatory)][ValidatePattern('^towavue-local-[0-9a-f]{64}$')][string]$OwnershipId,
        [Parameter(Mandatory)][string]$RegistrySubKey,
        [Parameter(Mandatory)][string]$ShortcutPath,
        [Parameter(Mandatory)][ValidateRange(1,2147483647)][int]$SizeKiB
    )

    # Production uses one HKCU uninstall key; tests use a fresh non-ARP GUID key.
    if ($RegistrySubKey -ne 'Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue-evaluation' -and
        $RegistrySubKey -notmatch '^Software\\towavue\\InstallerTests\\[0-9a-f]{32}$') { throw 'Registration key is outside the owned namespace.' }
    foreach ($path in @($InstallDirectory,$ShortcutPath)) {
        if ($path -notmatch '^[A-Za-z]:[\\/]' -or [IO.Path]::GetFullPath($path) -ne $path) { throw 'Registration paths must be absolute and normalized.' }
        $current = $path
        while ($current) {
            if ((Test-Path -LiteralPath $current) -and ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Registration paths must not traverse reparse points.' }
            $parent = Split-Path -Parent $current
            if ($parent -eq $current) { break }
            $current = $parent
        }
    }
    if ($InstallDirectory.TrimEnd('\').Length -le 2 -or [IO.Path]::GetExtension($ShortcutPath) -ne '.lnk') { throw 'Invalid dedicated installation or shortcut path.' }
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
    $key = $null
    try {
        $key = $base.OpenSubKey($RegistrySubKey,$Mode -eq 'Remove')
        if ($Mode -in @('Inspect','Install')) {
            if ($key -or (Test-Path -LiteralPath $ShortcutPath)) { throw 'An existing registration or shortcut occupies this application identity. It was not changed.' }
            if (-not (Test-Path -LiteralPath (Split-Path -Parent $ShortcutPath) -PathType Container)) { throw 'The per-user shortcut directory is unavailable.' }
            if ($Mode -eq 'Inspect') { return 'Registration destination is available; no changes made.' }
            $executable = Join-Path $InstallDirectory 'towavue.exe'
            $uninstaller = Join-Path $InstallDirectory 'Uninstall.exe'
            foreach ($path in @($executable,$uninstaller)) {
                if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw 'Application files must exist before registration.' }
            }
            $key = $base.CreateSubKey($RegistrySubKey)
            $strings = [ordered]@{
                TowavueOwnershipId=$OwnershipId
                InstallLocation=$InstallDirectory
                DisplayName='towavue (local evaluation)'
                DisplayVersion='0.0.0 (local evaluation)'
                DisplayIcon=('"' + $executable + '",0')
                UninstallString=('"' + $uninstaller + '"')
            }
            foreach ($name in $strings.Keys) { $key.SetValue($name,$strings[$name],[Microsoft.Win32.RegistryValueKind]::String) }
            foreach ($pair in @(@('NoModify',1),@('NoRepair',1),@('EstimatedSize',$SizeKiB))) { $key.SetValue($pair[0],[int]$pair[1],[Microsoft.Win32.RegistryValueKind]::DWord) }
            if (-not ('TowavueInstallerShellLink' -as [type])) { Add-Type -Path (Join-Path $PSScriptRoot 'UnicodeShellLink.cs') }
            [TowavueInstallerShellLink]::Create($ShortcutPath,$executable,$InstallDirectory)
            $key.SetValue('TowavueShortcutSha256',(Get-FileHash -LiteralPath $ShortcutPath).Hash.ToLowerInvariant(),[Microsoft.Win32.RegistryValueKind]::String)
            return 'Registered for the current user and created the Start menu shortcut.'
        }
        if (-not $key) { return 'No registration remains. Any shortcut without its ownership record is preserved.' }
        if ($key.GetValue('TowavueOwnershipId') -cne $OwnershipId -or $key.GetValue('InstallLocation') -ne $InstallDirectory) { throw 'Registration belongs to another installation. No registration or shortcut was removed.' }
        if ($Mode -eq 'VerifyRemoval') { return 'Registration ownership verified; no changes made.' }
        $shortcutMessage = 'No shortcut remains.'
        if (Test-Path -LiteralPath $ShortcutPath) {
            $recordedHash = $key.GetValue('TowavueShortcutSha256')
            if ($recordedHash -is [string] -and $recordedHash -match '^[0-9a-f]{64}$' -and
                (Get-FileHash -LiteralPath $ShortcutPath).Hash -eq $recordedHash) {
                [IO.File]::Delete($ShortcutPath)
                $shortcutMessage = 'Removed the unchanged owned shortcut.'
            }
            else { $shortcutMessage = 'Preserved a changed or unverified shortcut.' }
        }
        # Only owned value names are removed. Extra values/subkeys are not ours.
        foreach ($name in @('DisplayName','DisplayVersion','DisplayIcon','UninstallString','NoModify','NoRepair','EstimatedSize','TowavueShortcutSha256','InstallLocation','TowavueOwnershipId')) { $key.DeleteValue($name,$false) }
        $empty = $key.ValueCount -eq 0 -and $key.SubKeyCount -eq 0
        $key.Dispose()
        $key = $null
        if ($empty) { $base.DeleteSubKey($RegistrySubKey,$false) }
        return "Removed owned registration values. $shortcutMessage"
    }
    finally { if ($key) { $key.Dispose() }; $base.Dispose() }
}
