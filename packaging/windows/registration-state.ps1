function Get-TowavueAssociationRecords([string]$InstallDirectory,[string]$RegistrySubKey) {
    # Test registrations are confined beneath their existing private namespace.
    $prefix = if ($RegistrySubKey -cmatch '^Software\\towavue\\InstallerTests\\[0-9a-f]{32}$') { $RegistrySubKey + '\ShellRegistration\' } elseif ($RegistrySubKey -ceq 'Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue') { '' } else { throw 'Shell registration is outside the owned namespace.' }
    $exe = Join-Path $InstallDirectory 'towavue.exe'
    $open = '"' + $exe + '" -- "%1"'
    $window = '"' + $exe + '" --new-window -- "%1"'
    $icon = '"' + $exe + '",0'
    $records = [Collections.Generic.List[object]]::new()
    function Add-AssociationValue([string]$Path,[string]$Name,[string]$Value) {
        $records.Add([pscustomobject]@{path=($prefix+$Path);name=$Name;value=$Value})
    }
    # Publish the path binding first and retire it last, including partial writes.
    Add-AssociationValue 'Software\towavue\FileAssociations' 'InstallLocation' $InstallDirectory
    Add-AssociationValue 'Software\towavue\Capabilities' 'ApplicationName' 'towavue'
    Add-AssociationValue 'Software\towavue\Capabilities' 'ApplicationDescription' 'Image, video and audio viewer'
    Add-AssociationValue 'Software\towavue\Capabilities' 'ApplicationIcon' $icon
    Add-AssociationValue 'Software\RegisteredApplications' 'towavue' ($prefix+'Software\towavue\Capabilities')
    $application = 'Software\Classes\Applications\towavue.exe'
    Add-AssociationValue $application 'FriendlyAppName' 'towavue'
    Add-AssociationValue ($application+'\shell\open\command') '' $open
    $groups = [ordered]@{
        Image=@('apng','avif','bmp','gif','jpeg','jpg','png','tif','tiff','webp')
        Video=@('3gp','avi','m2ts','m4v','mkv','mov','mp4','mpeg','mpg','mts','ogv','ts','webm','wmv')
        Audio=@('aac','aiff','alac','flac','m4a','mp3','oga','ogg','opus','wav','wma')
    }
    foreach ($kind in $groups.Keys) {
        foreach ($extension in $groups[$kind]) {
            $progid = 'towavue.' + $extension
            $class = 'Software\Classes\' + $progid
            Add-AssociationValue $class '' ($extension.ToUpperInvariant()+' '+$kind.ToLowerInvariant())
            Add-AssociationValue ($class+'\Application') 'ApplicationName' 'towavue'
            Add-AssociationValue ($class+'\Application') 'ApplicationIcon' $icon
            Add-AssociationValue ($class+'\shell') '' 'open'
            Add-AssociationValue ($class+'\shell\open\command') '' $open
            Add-AssociationValue ('Software\Classes\.'+$extension+'\OpenWithProgids') $progid ''
            Add-AssociationValue ($application+'\SupportedTypes') ('.'+$extension) ''
            Add-AssociationValue 'Software\towavue\Capabilities\FileAssociations' ('.'+$extension) $progid
            foreach ($verb in @(($class+'\shell\towavue.newwindow'),('Software\Classes\SystemFileAssociations\.'+$extension+'\shell\towavue.newwindow'))) {
                Add-AssociationValue $verb '' 'Open in new towavue window'
                Add-AssociationValue $verb 'Icon' $icon
                Add-AssociationValue $verb 'MultiSelectModel' 'Single'
                Add-AssociationValue ($verb+'\command') '' $window
            }
        }
    }
    # No extension defaults, UserChoice, DefaultIcon, thumbnail handlers or codecs.
    return $records.ToArray()
}

function Invoke-TowavueAssociations([string]$Mode,[string]$InstallDirectory,[string]$RegistrySubKey) {
    $records = @(Get-TowavueAssociationRecords $InstallDirectory $RegistrySubKey)
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
    try {
        $binding = $base.OpenSubKey($records[0].path)
        $owned = $false
        try {
            $owned = $binding -and $binding.GetValueNames() -contains 'InstallLocation' -and $binding.GetValueKind('InstallLocation') -eq 'String' -and $binding.GetValue('InstallLocation') -ceq $InstallDirectory
            if ($Mode -in @('Inspect','Install') -and $binding -and -not $owned) { throw 'Shell registration belongs to another installation.' }
        } finally { if ($binding) { $binding.Dispose() } }
        if ($Mode -in @('Inspect','Install')) {
            # Validate the entire namespace before any write; retries admit only
            # absent values or unchanged values behind the exact path binding.
            foreach ($record in $records) {
                $key = $base.OpenSubKey($record.path)
                try {
                    if ($key -and $key.GetValueNames() -contains $record.name -and
                        (-not $owned -or $key.GetValueKind($record.name) -ne 'String' -or $key.GetValue($record.name) -cne $record.value)) { throw "Shell registration value is occupied or changed: $($record.path)" }
                } finally { if ($key) { $key.Dispose() } }
            }
            if ($Mode -eq 'Inspect') { return }
            foreach ($record in $records) {
                $key = $base.CreateSubKey($record.path)
                try { $key.SetValue($record.name,$record.value,[Microsoft.Win32.RegistryValueKind]::String); $key.Flush() } finally { $key.Dispose() }
            }
        } elseif ($Mode -eq 'Remove') {
            if (-not $owned) { return }
            [array]::Reverse($records)
            foreach ($record in $records) {
                $key = $base.OpenSubKey($record.path,$true)
                try {
                    if ($key -and $key.GetValueNames() -contains $record.name -and $key.GetValueKind($record.name) -eq 'String' -and $key.GetValue($record.name) -ceq $record.value) { $key.DeleteValue($record.name,$false) }
                } finally { if ($key) { $key.Dispose() } }
                # Empty-only pruning, never recursive deletion of shared classes.
                $path = $record.path
                $stop = if ($RegistrySubKey -like 'Software\towavue\InstallerTests\*') { $RegistrySubKey } else { 'Software' }
                while ($path -and $path -ine $stop) {
                    $key = $base.OpenSubKey($path)
                    $empty = $key -and $key.ValueCount -eq 0 -and $key.SubKeyCount -eq 0
                    if ($key) { $key.Dispose() }
                    if (-not $empty) { break }
                    $base.DeleteSubKey($path,$false)
                    $path = $path.Substring(0,$path.LastIndexOf('\'))
                }
            }
        } else { throw 'Unsupported Shell registration operation.' }
        if ($RegistrySubKey -ceq 'Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue') {
            if (-not ('TowavueAssociationNotification' -as [type])) {
                Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class TowavueAssociationNotification { [DllImport("shell32.dll")] public static extern void SHChangeNotify(uint e, uint f, IntPtr a, IntPtr b); }'
            }
            # SHCNE_ASSOCCHANGED with SHCNF_FLUSHNOWAIT; Explorer must not block Setup.
            [TowavueAssociationNotification]::SHChangeNotify(0x08000000,0x3000,[IntPtr]::Zero,[IntPtr]::Zero)
        }
    } finally { $base.Dispose() }
}

function Assert-TowavueProductVersion([string]$Version) {
    if ($Version -cnotmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') { throw 'A canonical stable product version is required.' }
    foreach ($part in $Version.Split('.')) { if ([uint64]$part -gt 65535) { throw 'Product version exceeds Windows resource limits.' } }
}

function Assert-TowavueRegistrationLocation([string]$InstallDirectory,[string]$RegistrySubKey,[string]$ShortcutPath) {
    # Shared by registration and pending-update discovery before registry access.
    if ($RegistrySubKey -notin @('Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue-evaluation','Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue') -and
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
}

function Invoke-TowavueRegistration {
    param(
        [Parameter(Mandatory)][ValidateSet('Inspect','Install','VerifyRemoval','Remove','VerifyUpdate','Update')][string]$Mode,
        [Parameter(Mandatory)][string]$InstallDirectory,
        [Parameter(Mandatory)][ValidatePattern('^towavue-(local|release)-[0-9a-f]{64}$')][string]$OwnershipId,
        [Parameter(Mandatory)][string]$RegistrySubKey,
        [Parameter(Mandatory)][string]$ShortcutPath,
        [Parameter(Mandatory)][ValidateRange(1,2147483647)][int]$SizeKiB,
        [string]$PreviousOwnershipId,
        [int]$PreviousSizeKiB,
        [string]$ProductVersion,
        [string]$PreviousProductVersion
    )

    Assert-TowavueRegistrationLocation $InstallDirectory $RegistrySubKey $ShortcutPath
    $updating = $Mode -in @('VerifyUpdate','Update')
    $release = $OwnershipId.StartsWith('towavue-release-',[StringComparison]::Ordinal)
    if (($RegistrySubKey -eq 'Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue' -and -not $release) -or
        ($RegistrySubKey -eq 'Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue-evaluation' -and $release)) { throw 'Production and evaluation registrations cannot take over each other.' }
    if ($updating -and ($PreviousOwnershipId -cnotmatch '^towavue-(local|release)-[0-9a-f]{64}$' -or $PreviousSizeKiB -lt 1)) { throw 'An update requires the recorded previous identity and size.' }
    if ($updating -and $PreviousOwnershipId.StartsWith('towavue-release-',[StringComparison]::Ordinal) -ne $release) { throw 'Production and evaluation payloads cannot replace each other.' }
    if ($release -and ($updating -or $Mode -in @('Inspect','Install'))) { Assert-TowavueProductVersion $ProductVersion }
    if ($release -and $updating) { Assert-TowavueProductVersion $PreviousProductVersion }
    if (-not $release -and ($ProductVersion -or $PreviousProductVersion)) { throw 'Evaluation registration cannot claim a production version.' }
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
    $key = $null
    try {
        $key = $base.OpenSubKey($RegistrySubKey,$Mode -in @('Remove','Update'))
        if ($key -and $Mode -in @('VerifyRemoval','Remove') -and $key.GetValueNames() -contains 'TowavuePendingUpdate') { throw 'An update is pending. Recover it before uninstalling.' }
        if ($updating) {
            if (-not $key) { throw 'The existing update registration is missing.' }
            if ($key.GetValueKind('InstallLocation') -ne 'String' -or $key.GetValue('InstallLocation') -ne $InstallDirectory -or
                $key.GetValueKind('TowavueOwnershipId') -ne 'String' -or $key.GetValueKind('EstimatedSize') -ne 'DWord') { throw 'Update registration path or value type differs; preserve it.' }
            $currentId = $key.GetValue('TowavueOwnershipId')
            $currentSize = $key.GetValue('EstimatedSize')
            if (@($PreviousOwnershipId,$OwnershipId) -cnotcontains $currentId -or @($PreviousSizeKiB,$SizeKiB) -notcontains $currentSize) { throw 'Update registration has an unknown identity or size; preserve it.' }
            if ($release -and ($key.GetValueKind('DisplayVersion') -ne 'String' -or @($PreviousProductVersion,$ProductVersion) -cnotcontains $key.GetValue('DisplayVersion'))) { throw 'Update registration has an unknown product version or type; preserve it.' }
            $associationMode = if ($release -and [version]$ProductVersion -ge [version]'1.0.1') { 'Install' } else { 'Remove' }
            if ($release -and $associationMode -eq 'Install') { Invoke-TowavueAssociations 'Inspect' $InstallDirectory $RegistrySubKey }
            if ($Mode -eq 'VerifyUpdate') { return 'Registration transition verified; no changes made.' }
            if ($release) { Invoke-TowavueAssociations $associationMode $InstallDirectory $RegistrySubKey }
            # Separate writes are not atomic. The caller retains both typed states;
            # retry or reversed arguments can complete a known partial transition.
            if ($currentSize -ne $SizeKiB) { $key.SetValue('EstimatedSize',$SizeKiB,[Microsoft.Win32.RegistryValueKind]::DWord) }
            if ($release -and $key.GetValue('DisplayVersion') -cne $ProductVersion) { $key.SetValue('DisplayVersion',$ProductVersion,[Microsoft.Win32.RegistryValueKind]::String) }
            if ($currentId -cne $OwnershipId) { $key.SetValue('TowavueOwnershipId',$OwnershipId,[Microsoft.Win32.RegistryValueKind]::String) }
            $key.Flush()
            return 'Updated recorded ownership, size and production version; shortcut and unrelated values were preserved.'
        }
        if ($Mode -in @('Inspect','Install')) {
            if ($key -or (Test-Path -LiteralPath $ShortcutPath)) { throw 'An existing registration or shortcut occupies this application identity. It was not changed.' }
            if (-not (Test-Path -LiteralPath (Split-Path -Parent $ShortcutPath) -PathType Container)) { throw 'The per-user shortcut directory is unavailable.' }
            if ($release -and [version]$ProductVersion -ge [version]'1.0.1') { Invoke-TowavueAssociations 'Inspect' $InstallDirectory $RegistrySubKey }
            if ($Mode -eq 'Inspect') { return 'Registration destination is available; no changes made.' }
            $executable = Join-Path $InstallDirectory 'towavue.exe'
            $uninstaller = Join-Path $InstallDirectory 'Uninstall.exe'
            foreach ($path in @($executable,$uninstaller)) {
                if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw 'Application files must exist before registration.' }
            }
            if ($release -and [Diagnostics.FileVersionInfo]::GetVersionInfo($executable).ProductVersion -cne $ProductVersion) { throw 'Registered version must match the installed executable.' }
            $key = $base.CreateSubKey($RegistrySubKey)
            $strings = [ordered]@{
                TowavueOwnershipId=$OwnershipId
                InstallLocation=$InstallDirectory
                DisplayName=$(if ($release) { 'towavue' } else { 'towavue (local evaluation)' })
                DisplayVersion=$(if ($release) { $ProductVersion } else { '0.0.0 (local evaluation)' })
                DisplayIcon=('"' + $executable + '",0')
                UninstallString=('"' + $uninstaller + '"')
            }
            foreach ($name in $strings.Keys) { $key.SetValue($name,$strings[$name],[Microsoft.Win32.RegistryValueKind]::String) }
            foreach ($pair in @(@('NoModify',1),@('NoRepair',1),@('EstimatedSize',$SizeKiB))) { $key.SetValue($pair[0],[int]$pair[1],[Microsoft.Win32.RegistryValueKind]::DWord) }
            if (-not ('TowavueInstallerShellLink' -as [type])) { Add-Type -Path (Join-Path $PSScriptRoot 'UnicodeShellLink.cs') }
            [TowavueInstallerShellLink]::Create($ShortcutPath,$executable,$InstallDirectory)
            $key.SetValue('TowavueShortcutSha256',(Get-FileHash -LiteralPath $ShortcutPath -ErrorAction Stop).Hash.ToLowerInvariant(),[Microsoft.Win32.RegistryValueKind]::String)
            if ($release -and [version]$ProductVersion -ge [version]'1.0.1') { Invoke-TowavueAssociations 'Install' $InstallDirectory $RegistrySubKey }
            return 'Registered for the current user and created the Start menu shortcut.'
        }
        if (-not $key) { return 'No registration remains. Any shortcut without its ownership record is preserved.' }
        if ($key.GetValue('TowavueOwnershipId') -cne $OwnershipId -or $key.GetValue('InstallLocation') -ne $InstallDirectory) { throw 'Registration belongs to another installation. No registration or shortcut was removed.' }
        if ($Mode -eq 'VerifyRemoval') { return 'Registration ownership verified; no changes made.' }
        if ($release) { Invoke-TowavueAssociations 'Remove' $InstallDirectory $RegistrySubKey }
        $shortcutMessage = 'No shortcut remains.'
        if (Test-Path -LiteralPath $ShortcutPath) {
            $recordedHash = $key.GetValue('TowavueShortcutSha256')
            if ($recordedHash -is [string] -and $recordedHash -match '^[0-9a-f]{64}$' -and
                (Get-FileHash -LiteralPath $ShortcutPath -ErrorAction Stop).Hash -eq $recordedHash) {
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
