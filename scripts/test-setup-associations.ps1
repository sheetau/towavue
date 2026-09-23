$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../packaging/windows/registration-state.ps1')
$registry = 'Software\towavue\InstallerTests\' + [guid]::NewGuid().ToString('N')
$install = Join-Path ([IO.Path]::GetTempPath()) 'towavue association space & unicode 日本語'
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
$records = @(Get-TowavueAssociationRecords $install $registry)
function Assert-True($Condition,$Message) { if (-not $Condition) { throw $Message } }
function Read-Value($Record) {
    $key = $base.OpenSubKey($Record.path)
    try { if ($key) { return $key.GetValue($Record.name,$null) } } finally { if ($key) { $key.Dispose() } }
}
function Refuse-Install {
    try { Invoke-TowavueAssociations Install $install $registry } catch { return }
    throw 'Changed Shell registration was overwritten.'
}
try {
    Assert-True (@($records | Where-Object { -not $_.path.StartsWith($registry+'\ShellRegistration\',[StringComparison]::Ordinal) }).Count -eq 0) 'Fixture escaped its private namespace.'
    Assert-True (@($records | Where-Object { $_.path -match 'UserChoice|shellex' }).Count -eq 0) 'Default or thumbnail ownership was requested.'
    $media = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../crates/towavue-core/src/media.rs') -Raw -Encoding UTF8
    $extensions = @([regex]::Matches($media.Substring(0,$media.IndexOf('#[cfg(test)]')),'"([a-z0-9]+)"') | ForEach-Object { '.'+$_.Groups[1].Value } | Sort-Object -Unique)
    $registered = @($records | Where-Object { $_.path.EndsWith('\Capabilities\FileAssociations') } | ForEach-Object name | Sort-Object)
    Assert-True (($extensions -join ',') -ceq ($registered -join ',')) 'Registered types differ from supported media.'
    $icons = @($records | Where-Object { $_.path.EndsWith('\DefaultIcon') })
    Assert-True ($icons.Count -eq $extensions.Count) 'Not every media type has an icon.'
    foreach ($entry in $icons) {
        Assert-True ($entry.path -match '\\Classes\\towavue\.[a-z0-9]+\\DefaultIcon$') 'Icon escaped an owned ProgID.'
    }
    foreach ($pair in @(@('png',2),@('mp4',3),@('mp3',4))) {
        $entry = @($icons | Where-Object { $_.path.EndsWith('\towavue.'+$pair[0]+'\DefaultIcon') })[0]
        Assert-True ($entry.value -ceq ('"'+$install+'\towavue.exe",-'+$pair[1])) 'Media icon resource or quoting differs.'
    }
    Invoke-TowavueAssociations Inspect $install $registry
    Assert-True ($null -eq (Read-Value $records[0])) 'Inspection wrote registry state.'
    Invoke-TowavueAssociations Install $install $registry
    Invoke-TowavueAssociations Install $install $registry
    foreach ($record in $records) { Assert-True ((Read-Value $record) -ceq $record.value) 'An installed association differs.' }
    $open = @($records | Where-Object { $_.path.EndsWith('\towavue.png\shell\open\command') })[0]
    $window = @($records | Where-Object { $_.path.EndsWith('\SystemFileAssociations\.png\shell\towavue.newwindow\command') })[0]
    Assert-True ($open.value -ceq ('"'+$install+'\towavue.exe" -- "%1"')) 'Open command quoting differs.'
    Assert-True ($window.value -ceq ('"'+$install+'\towavue.exe" --new-window -- "%1"')) 'Window command quoting differs.'
    $key = $base.OpenSubKey($open.path,$true)
    try { $key.SetValue('','Owner replacement'); $key.SetValue('Sentinel','keep') } finally { $key.Dispose() }
    Refuse-Install
    Invoke-TowavueAssociations Remove $install $registry
    Assert-True ((Read-Value $open) -ceq 'Owner replacement') 'Removal changed a replacement command.'
    Assert-True ($null -eq (Read-Value $records[0])) 'Removal retained its own binding.'
    Refuse-Install
    $key = $base.OpenSubKey($open.path,$true)
    try { $key.DeleteValue('') } finally { $key.Dispose() }
    Invoke-TowavueAssociations Install $install $registry
    Invoke-TowavueAssociations Install $install $registry '1.0.1'
    Invoke-TowavueAssociations Install $install $registry '1.0.1'
    foreach ($entry in $icons) { Assert-True ($null -eq (Read-Value $entry)) 'Rollback kept icons absent from the older executable.' }
    foreach ($entry in @(Get-TowavueAssociationRecords $install $registry '1.0.1')) {
        Assert-True ((Read-Value $entry) -ceq $entry.value) 'Rollback changed legacy associations.'
    }
    Invoke-TowavueAssociations Install $install $registry '1.0.3'
    foreach ($entry in $icons) { Assert-True ((Read-Value $entry) -ceq $entry.value) 'Upgrade did not restore media icons.' }
    $key = $base.OpenSubKey($icons[0].path,$true)
    try { $key.SetValue('','Owner icon replacement') } finally { $key.Dispose() }
    Invoke-TowavueAssociations Install $install $registry '1.0.1'
    Assert-True ((Read-Value $icons[0]) -ceq 'Owner icon replacement') 'Legacy rollback removed a foreign icon.'
    Refuse-Install
    $key = $base.OpenSubKey($icons[0].path,$true)
    try { $key.DeleteValue('') } finally { $key.Dispose() }
    Invoke-TowavueAssociations Install $install $registry '1.0.3'
    # Simulate interruption after the path binding and one command were written.
    foreach ($record in $records | Select-Object -Skip 2) {
        $key = $base.OpenSubKey($record.path,$true)
        try { if ($key) { $key.DeleteValue($record.name,$false) } } finally { if ($key) { $key.Dispose() } }
    }
    Invoke-TowavueAssociations Install $install $registry
    foreach ($record in $records) { Assert-True ((Read-Value $record) -ceq $record.value) 'Partial registration retry failed.' }
    Invoke-TowavueAssociations Remove $install $registry
    Invoke-TowavueAssociations Remove $install $registry
    foreach ($record in $records) { Assert-True ($null -eq (Read-Value $record)) 'Owned value remains after removal.' }
    $key = $base.OpenSubKey($open.path)
    try { Assert-True ($key.GetValue('Sentinel') -ceq 'keep') 'Unrelated registry value was removed.' } finally { $key.Dispose() }
    Write-Output "PASS: $($extensions.Count) supported extensions, quoted tab/window commands, scoped registration, collisions, partial retry, exact removal and foreign-value preservation."
} finally {
    $base.DeleteSubKeyTree($registry,$false)
    $base.Dispose()
}
