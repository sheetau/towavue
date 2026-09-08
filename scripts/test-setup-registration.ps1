[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $repositoryRoot 'packaging/windows/registration-state.ps1')
$id = [guid]::NewGuid().ToString('N')
$trialRoot = Join-Path $repositoryRoot "target/tmp/setup-registration-$id"
$install = Join-Path $trialRoot ('app space & $ ' + [char]0x65e5 + [char]0x672c)
$programs = Join-Path $trialRoot 'programs'
New-Item -ItemType Directory -Path $install,$programs | Out-Null
$keyName = "Software\towavue\InstallerTests\$id"
$shortcut = Join-Path $programs 'towavue (local evaluation).lnk'
$arguments = @{InstallDirectory=$install;OwnershipId=('towavue-local-' + ('a' * 64));RegistrySubKey=$keyName;ShortcutPath=$shortcut;SizeKiB=123}
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
$before = $base.OpenSubKey($keyName)
if ($before) { $before.Dispose(); $base.Dispose(); throw 'Fresh registration test key already exists.' }
$utf8 = [Text.UTF8Encoding]::new($false)
foreach ($name in @('towavue.exe','Uninstall.exe')) { [IO.File]::WriteAllText((Join-Path $install $name),'Not executable: harmless registration fixture.',$utf8) }
function Assert-True([bool]$Condition,[string]$Message) { if (-not $Condition) { throw $Message } }
function Expect-Failure([hashtable]$Invocation,[string]$Message) {
    $rejected = $false
    try { Invoke-TowavueRegistration @Invocation | Out-Null }
    catch { if (-not $_.Exception.Message.Contains($Message)) { throw }; $rejected = $true }
    Assert-True $rejected "Expected registration rejection: $Message"
}
function Reset-TrialShortcut {
    $resolved = [IO.Path]::GetFullPath($shortcut)
    Assert-True ($resolved.StartsWith($trialRoot + '\',[StringComparison]::OrdinalIgnoreCase)) 'Test shortcut escaped scratch.'
    if (Test-Path -LiteralPath $resolved) { [IO.File]::Delete($resolved) }
}
function Get-RegistrationSnapshot {
    $key = $base.OpenSubKey($keyName)
    try {
        $snapshot = [ordered]@{}
        foreach ($name in $key.GetValueNames() | Sort-Object) { $snapshot[$name] = @($key.GetValueKind($name).ToString(),$key.GetValue($name)) }
        return $snapshot | ConvertTo-Json -Depth 4 -Compress
    } finally { $key.Dispose() }
}
try {
    Invoke-TowavueRegistration @arguments -Mode Inspect | Out-Null
    Assert-True (-not (Test-Path -LiteralPath $shortcut)) 'Inspection wrote a shortcut.'
    Invoke-TowavueRegistration @arguments -Mode Install | Out-Null
    $key = $base.OpenSubKey($keyName)
    try {
        Assert-True ($key.GetValue('InstallLocation') -ceq $install) 'Japanese install path was changed.'
        Assert-True ($key.GetValue('UninstallString') -ceq ('"' + (Join-Path $install 'Uninstall.exe') + '"')) 'Uninstall command is not quoted.'
        Assert-True ($key.GetValue('NoModify') -eq 1 -and $key.GetValueKind('EstimatedSize') -eq 'DWord' -and $key.GetValue('EstimatedSize') -eq 123) 'Wrong registration values or types.'
        Assert-True ($key.GetValue('TowavueShortcutSha256') -eq (Get-FileHash -LiteralPath $shortcut).Hash) 'Shortcut ownership digest differs.'
    }
    finally { $key.Dispose() }
    $link = [TowavueInstallerShellLink]::Read($shortcut)
    Assert-True ($link[0] -eq (Join-Path $install 'towavue.exe') -and $link[1] -eq $install -and $link[2] -eq '') 'Native Shell link points at the wrong application.'
    $shortcutHash = (Get-FileHash -LiteralPath $shortcut).Hash
    $key = $base.OpenSubKey($keyName,$true)
    try { $key.SetValue('UpdateSentinel','preserve extra registry data') } finally { $key.Dispose() }
    $beforeUpdate = Get-RegistrationSnapshot
    $update = $arguments.Clone()
    $update.Mode = 'Update'; $update.PreviousOwnershipId = $arguments.OwnershipId; $update.PreviousSizeKiB = $arguments.SizeKiB
    $update.OwnershipId = 'towavue-local-' + ('b' * 64); $update.SizeKiB = 456
    $rollback = $arguments.Clone()
    $rollback.Mode = 'Update'; $rollback.PreviousOwnershipId = $update.OwnershipId; $rollback.PreviousSizeKiB = $update.SizeKiB
    $verify = $update.Clone(); $verify.Mode = 'VerifyUpdate'
    Invoke-TowavueRegistration @verify | Out-Null
    Assert-True ((Get-RegistrationSnapshot) -ceq $beforeUpdate) 'Update verification changed the registration.'
    Invoke-TowavueRegistration @update | Out-Null
    $afterUpdate = Get-RegistrationSnapshot
    $key = $base.OpenSubKey($keyName)
    try { Assert-True ($key.GetValue('TowavueOwnershipId') -ceq $update.OwnershipId -and $key.GetValue('EstimatedSize') -eq 456 -and $key.GetValue('UpdateSentinel') -eq 'preserve extra registry data') 'Wrong update result.' }
    finally { $key.Dispose() }
    Invoke-TowavueRegistration @update | Out-Null
    Assert-True ((Get-RegistrationSnapshot) -ceq $afterUpdate) 'Repeated registration update changed values.'
    Invoke-TowavueRegistration @rollback | Out-Null
    Invoke-TowavueRegistration @rollback | Out-Null
    Assert-True ((Get-RegistrationSnapshot) -ceq $beforeUpdate) 'Registration rollback did not restore exact typed values.'

    $statePath = Join-Path $repositoryRoot 'packaging/windows/registration-state.ps1'
    $stateSource = [IO.File]::ReadAllText($statePath)
    $injectedPath = Join-Path $trialRoot 'registration-interrupted.ps1'
    foreach ($direction in @('Apply','Rollback')) {
    foreach ($write in @("`$key.SetValue('EstimatedSize',`$SizeKiB,[Microsoft.Win32.RegistryValueKind]::DWord)","`$key.SetValue('TowavueOwnershipId',`$OwnershipId,[Microsoft.Win32.RegistryValueKind]::String)")) {
        if ($direction -eq 'Rollback') { Invoke-TowavueRegistration @update | Out-Null }
        $injected = $stateSource.Replace($write,($write + "; throw 'Injected registration interruption'"))
        Assert-True ($injected -cne $stateSource) 'Registration injection missed its write.'
        [IO.File]::WriteAllText($injectedPath,$injected,$utf8)
        . $injectedPath
        Expect-Failure $(if ($direction -eq 'Apply') { $update } else { $rollback }) 'Injected registration interruption'
        . $statePath
        Invoke-TowavueRegistration @verify | Out-Null
        Invoke-TowavueRegistration @rollback | Out-Null
        Assert-True ((Get-RegistrationSnapshot) -ceq $beforeUpdate) 'Interrupted registration did not roll back.'
    }
    }
    foreach ($case in @('identity','size','size-type','path','missing-previous')) {
        $invalid = $update.Clone()
        $key = $base.OpenSubKey($keyName,$true)
        try {
            switch ($case) {
                'identity' { $key.SetValue('TowavueOwnershipId',('towavue-local-' + ('c' * 64))) }
                'size' { $key.SetValue('EstimatedSize',789,[Microsoft.Win32.RegistryValueKind]::DWord) }
                'size-type' { $key.SetValue('EstimatedSize','123',[Microsoft.Win32.RegistryValueKind]::String) }
                'path' { $key.SetValue('InstallLocation',$programs) }
                'missing-previous' { $invalid.Remove('PreviousOwnershipId') }
            }
        } finally { $key.Dispose() }
        $beforeInvalid = Get-RegistrationSnapshot
        $message = if ($case -eq 'missing-previous') { 'recorded previous' } elseif ($case -in @('size-type','path')) { 'path or value type' } else { 'unknown identity or size' }
        Expect-Failure $invalid $message
        Assert-True ((Get-RegistrationSnapshot) -ceq $beforeInvalid) 'Rejected registration update changed values.'
        $key = $base.OpenSubKey($keyName,$true)
        try {
            $key.SetValue('TowavueOwnershipId',$arguments.OwnershipId,[Microsoft.Win32.RegistryValueKind]::String)
            $key.SetValue('EstimatedSize',123,[Microsoft.Win32.RegistryValueKind]::DWord)
            $key.SetValue('InstallLocation',$install,[Microsoft.Win32.RegistryValueKind]::String)
        } finally { $key.Dispose() }
    }
    Assert-True ((Get-FileHash -LiteralPath $shortcut).Hash -eq $shortcutHash) 'Registration update or recovery changed the shortcut.'
    $key = $base.OpenSubKey($keyName,$true)
    try { $key.DeleteValue('UpdateSentinel') } finally { $key.Dispose() }
    $invocation = $arguments.Clone(); $invocation.Mode = 'Install'
    Expect-Failure $invocation 'existing registration or shortcut'
    foreach ($mode in @('VerifyRemoval','Remove')) {
        $invocation = $arguments.Clone(); $invocation.Mode = $mode; $invocation.OwnershipId = 'towavue-local-' + ('b' * 64)
        Expect-Failure $invocation 'another installation'
        $invocation = $arguments.Clone(); $invocation.Mode = $mode; $invocation.InstallDirectory = $programs
        Expect-Failure $invocation 'another installation'
    }
    Assert-True ((Get-FileHash -LiteralPath $shortcut).Hash -eq $shortcutHash) 'Rejected operation changed the shortcut.'
    Invoke-TowavueRegistration @arguments -Mode VerifyRemoval | Out-Null
    $locked = [IO.File]::Open($shortcut,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None)
    try {
        $rejected = $false
        try { Invoke-TowavueRegistration @arguments -Mode Remove | Out-Null } catch { $rejected = $true }
        Assert-True $rejected 'Locked shortcut removal succeeded.'
        $key = $base.OpenSubKey($keyName)
        try { Assert-True ($key.GetValue('TowavueOwnershipId') -eq $arguments.OwnershipId) 'Locked failure lost retry identity.' } finally { $key.Dispose() }
    }
    finally { $locked.Dispose() }
    Invoke-TowavueRegistration @arguments -Mode Remove | Out-Null
    Assert-True (-not (Test-Path -LiteralPath $shortcut) -and -not $base.OpenSubKey($keyName)) 'Normal removal left owned registration or shortcut.'
    Expect-Failure $update 'existing update registration is missing'
    Invoke-TowavueRegistration @arguments -Mode Remove | Out-Null

    Invoke-TowavueRegistration @arguments -Mode Install | Out-Null
    [IO.File]::WriteAllText($shortcut,'A user replacement must remain byte-for-byte.',$utf8)
    $replacementHash = (Get-FileHash -LiteralPath $shortcut).Hash
    $key = $base.OpenSubKey($keyName,$true)
    try {
        $key.SetValue('UserAdded','keep')
        $child = $key.CreateSubKey('user-subkey')
        try { $child.SetValue('keep','user data') } finally { $child.Dispose() }
    }
    finally { $key.Dispose() }
    Invoke-TowavueRegistration @arguments -Mode Remove | Out-Null
    Assert-True ((Get-FileHash -LiteralPath $shortcut).Hash -eq $replacementHash) 'User replacement shortcut was removed or altered.'
    $key = $base.OpenSubKey($keyName,$true)
    try {
        Assert-True ($key.GetValue('UserAdded') -eq 'keep' -and $null -eq $key.GetValue('DisplayName') -and $key.SubKeyCount -eq 1) 'Unknown registration data was removed.'
        $child = $key.OpenSubKey('user-subkey',$true)
        try { Assert-True ($child.GetValue('keep') -eq 'user data') 'Unknown child value changed.'; $child.DeleteValue('keep') } finally { $child.Dispose() }
        $key.DeleteSubKey('user-subkey')
        $key.DeleteValue('UserAdded')
    }
    finally { $key.Dispose() }
    $base.DeleteSubKey($keyName)
    Invoke-TowavueRegistration @arguments -Mode Remove | Out-Null
    Assert-True ((Get-FileHash -LiteralPath $shortcut).Hash -eq $replacementHash) 'Missing registration inferred ownership of a shortcut.'
    $invocation = $arguments.Clone(); $invocation.Mode = 'Inspect'
    Expect-Failure $invocation 'existing registration or shortcut'
    Reset-TrialShortcut

    $key = $base.CreateSubKey($keyName)
    $key.Dispose()
    Expect-Failure $invocation 'existing registration or shortcut'
    $invocation.Mode = 'Remove'
    Expect-Failure $invocation 'another installation'
    $base.DeleteSubKey($keyName)
    $invocation = $arguments.Clone(); $invocation.Mode = 'Install'; $invocation.InstallDirectory = $programs
    Expect-Failure $invocation 'Application files must exist'
    $invocation = $arguments.Clone(); $invocation.Mode = 'Inspect'; $invocation.RegistrySubKey = 'Software'
    Expect-Failure $invocation 'outside the owned namespace'
    $invocation = $arguments.Clone(); $invocation.Mode = 'Inspect'; $invocation.InstallDirectory = 'C:relative'
    Expect-Failure $invocation 'absolute and normalized'
    $junction = Join-Path $trialRoot 'junction'
    New-Item -ItemType Junction -Path $junction -Target $programs | Out-Null
    $invocation = $arguments.Clone(); $invocation.Mode = 'Inspect'; $invocation.ShortcutPath = Join-Path $junction 'fixture.lnk'
    Expect-Failure $invocation 'reparse points'
    Assert-True (-not $base.OpenSubKey($keyName) -and -not (Test-Path -LiteralPath $shortcut)) 'Failure tests left a registration or shortcut.'
    foreach ($name in @('towavue.exe','Uninstall.exe')) { Assert-True ([IO.File]::ReadAllText((Join-Path $install $name)) -eq 'Not executable: harmless registration fixture.') 'Original fixture file changed.' }
    Write-Output "PASS: native shortcut fields, typed registry values, reversible two-field updates and write interruptions, Unicode paths, collision/identity/refusal, lock/retry, replacement and extra-data preservation, missing state and junction guards. Test GUID registry key and generated shortcuts removed. Evidence: $trialRoot"
}
finally { $base.Dispose() }
