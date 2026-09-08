[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$library = Join-Path $PSScriptRoot 'setup-update-transaction.ps1'
. $library
$trialRoot = Join-Path $repositoryRoot ('target/tmp/update-transaction-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $trialRoot | Out-Null
$utf8 = [Text.UTF8Encoding]::new($false)
function Assert-True([bool]$Condition,[string]$Message) { if (-not $Condition) { throw $Message } }
function Write-Trial([string]$Path,[string]$Text) {
    Assert-True ([IO.Path]::GetFullPath($Path).StartsWith($trialRoot + '\',[StringComparison]::OrdinalIgnoreCase)) 'Fixture escaped scratch.'
    New-Item -ItemType Directory -Path (Split-Path -Parent $Path) -Force | Out-Null
    [IO.File]::WriteAllText($Path,$Text,$utf8)
}
function New-Pair([string]$Name) {
    $root = Join-Path $trialRoot $Name
    $installed = Join-Path $root ('installed space & $ ' + [char]0x65e5 + [char]0x672c)
    $incoming = Join-Path $root 'incoming'
    foreach ($directory in @($installed,$incoming)) {
        $old = $directory -eq $installed
        Write-Trial (Join-Path $directory 'towavue.exe') $(if ($old) { 'old fixture, never executed' } else { 'new fixture, never executed' })
        Write-Trial (Join-Path $directory 'keep.dll') 'unchanged fixture'
        Write-Trial (Join-Path $directory $(if ($old) { 'remove.dll' } else { 'added.dll' })) 'version-specific fixture'
        $records = @(Get-ChildItem -LiteralPath $directory -File | ForEach-Object { [pscustomobject]@{name=$_.Name;bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant()} })
        Write-Trial (Join-Path $directory 'licenses/INSTALLED-FILES.json') ([ordered]@{schema_version=1;files=$records} | ConvertTo-Json -Depth 6)
        $identity = 'towavue-local-' + (Get-FileHash -LiteralPath (Join-Path $directory 'licenses/INSTALLED-FILES.json')).Hash.ToLowerInvariant()
        if ($old) {
            [IO.File]::WriteAllText((Join-Path $directory 'towavue-install.ini'),"[installation]`r`nid=$identity`r`ndirectory=$directory`r`n",[Text.Encoding]::Unicode)
            Write-Trial (Join-Path $directory 'Uninstall.exe') 'old uninstaller fixture, never executed'
        }
    }
    Write-Trial (Join-Path $installed 'user-media.txt') 'preserve user data'
    $uninstaller = Join-Path $root 'new-uninstaller.exe'
    Write-Trial $uninstaller 'new uninstaller fixture, never executed'
    return @{InstallDirectory=$installed;IncomingPayloadDirectory=$incoming;IncomingOwnershipId=$identity;NewUninstaller=$uninstaller}
}
function Get-Snapshot([hashtable]$Pair) {
    $ordered = [ordered]@{}
    foreach ($root in @($Pair.InstallDirectory,$Pair.IncomingPayloadDirectory)) {
        foreach ($file in Get-ChildItem -LiteralPath $root -Recurse -File -Force | Sort-Object FullName) { $ordered[$file.FullName] = (Get-FileHash -LiteralPath $file.FullName).Hash }
    }
    return $ordered | ConvertTo-Json -Compress
}
function Get-Token($Token) { return @{TransactionDirectory=$Token.TransactionDirectory;JournalSha256=$Token.JournalSha256} }
function Expect-Failure([scriptblock]$Action,[string]$Message) {
    $rejected = $false
    try { & $Action | Out-Null } catch { if (-not $_.Exception.Message.Contains($Message)) { throw }; $rejected = $true }
    Assert-True $rejected "Unsafe transaction accepted: $Message"
}
function Assert-Applied($Pair,$Token) {
    $journal = Get-Content -LiteralPath (Join-Path $Token.TransactionDirectory 'journal.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    foreach ($entry in $journal.entries) {
        $path = Join-Path $Pair.InstallDirectory $entry.name
        if ($entry.after) { Assert-True (Test-UpdateRecord (Get-Record $Pair.InstallDirectory $entry.name) $entry.after) 'Applied bytes differ.' }
        else { Assert-True (-not (Test-Path -LiteralPath $path)) 'Removed file remains installed.' }
    }
    Assert-True ([IO.File]::ReadAllText((Join-Path $Pair.InstallDirectory 'user-media.txt')) -ceq 'preserve user data') 'User file changed.'
}
$pair = New-Pair 'baseline'
$before = Get-Snapshot $pair
$token = Get-Token (New-TowavueUpdateTransaction @pair)
Assert-True ((Get-Snapshot $pair) -ceq $before) 'Prepare modified original files.'
Assert-True (-not (Test-Path -LiteralPath (Join-Path $token.TransactionDirectory '1.old')) -and -not (Test-Path -LiteralPath (Join-Path $token.TransactionDirectory '1.new'))) 'Unchanged non-app file was copied unnecessarily.'
$result = Invoke-TowavueUpdateTransaction -Mode Apply @token
Assert-True ($result.state -eq 'payload_applied_registration_pending' -and $result.retained) 'File transaction claims registration or cleanup.'
Assert-Applied $pair $token
Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null
Assert-True ((Get-Snapshot $pair) -ceq $before) 'Rollback failed to restore original bytes.'
Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null
Assert-True ((Get-Snapshot $pair) -ceq $before) 'Rollback is not idempotent.'
Push-Location $env:TEMP
try { Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null }
finally { Pop-Location }
Expect-Failure { Invoke-TowavueUpdateTransaction -Mode Apply @token } 'already used'

$pair = New-Pair 'unchanged-executable'
Write-Trial (Join-Path $pair.IncomingPayloadDirectory 'towavue.exe') ([IO.File]::ReadAllText((Join-Path $pair.InstallDirectory 'towavue.exe')))
$records = @(Get-ChildItem -LiteralPath $pair.IncomingPayloadDirectory -File | ForEach-Object { Get-Record $pair.IncomingPayloadDirectory $_.Name })
Write-Trial (Join-Path $pair.IncomingPayloadDirectory 'licenses/INSTALLED-FILES.json') ([ordered]@{schema_version=1;files=$records} | ConvertTo-Json -Depth 6)
$pair.IncomingOwnershipId = 'towavue-local-' + (Get-FileHash -LiteralPath (Join-Path $pair.IncomingPayloadDirectory 'licenses/INSTALLED-FILES.json')).Hash.ToLowerInvariant()
$before = Get-Snapshot $pair
$token = Get-Token (New-TowavueUpdateTransaction @pair)
Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null
Assert-Applied $pair $token
$journal = Get-Content -LiteralPath (Join-Path $token.TransactionDirectory 'journal.json') -Raw -Encoding UTF8 | ConvertFrom-Json
Assert-True ($journal.entries[-1].action -eq 'keep') 'Fixture did not preserve the executable identity.'
Assert-True (Test-Path -LiteralPath (Join-Path $token.TransactionDirectory '6.apply-retired')) 'Unchanged app was not retired before DLL changes.'
Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null
Assert-True ((Get-Snapshot $pair) -ceq $before) 'Unchanged executable rollback differs.'

$pair = New-Pair 'hard-links'
$before = Get-Snapshot $pair
$links = @()
foreach ($name in @('towavue.exe','remove.dll')) {
    $path = Join-Path (Split-Path -Parent $pair.InstallDirectory) ("outside-$name")
    New-Item -ItemType HardLink -Path $path -Target (Join-Path $pair.InstallDirectory $name) | Out-Null
    $links += [pscustomobject]@{path=$path;hash=(Get-FileHash -LiteralPath $path).Hash}
}
$token = Get-Token (New-TowavueUpdateTransaction @pair)
Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null
Assert-Applied $pair $token
foreach ($link in $links) { Assert-True ((Get-FileHash -LiteralPath $link.path).Hash -ceq $link.hash) 'Apply wrote through an outside hard link.' }
Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null
Assert-True ((Get-Snapshot $pair) -ceq $before) 'Hard-link rollback differs.'
foreach ($link in $links) { Assert-True ((Get-FileHash -LiteralPath $link.path).Hash -ceq $link.hash) 'Rollback changed an outside hard link.' }
foreach ($link in $links) {
    Write-Trial $link.path 'identity probe after completed rollback'
    $name = (Split-Path -Leaf $link.path).Substring('outside-'.Length)
    Assert-True ([IO.File]::ReadAllText((Join-Path $pair.InstallDirectory $name)) -ceq 'identity probe after completed rollback') 'Rollback lost the original hard-link identity.'
}

$pair = New-Pair 'original-metadata'
$appPath = Join-Path $pair.InstallDirectory 'towavue.exe'
Assert-True ([IO.Path]::GetFullPath($appPath).StartsWith($trialRoot + '\')) 'Metadata fixture escaped scratch.'
Set-Content -LiteralPath $appPath -Stream 'towavue-fixture' -Value 'preserve original alternate stream' -Encoding UTF8 -NoNewline
[IO.File]::SetCreationTimeUtc($appPath,[datetime]::new(2005,2,3,4,5,6,[DateTimeKind]::Utc))
[IO.File]::SetLastWriteTimeUtc($appPath,[datetime]::new(2010,3,4,5,6,7,[DateTimeKind]::Utc))
$oldItem = Get-Item -LiteralPath $appPath
$oldCreation = $oldItem.CreationTimeUtc.Ticks
$oldWrite = $oldItem.LastWriteTimeUtc.Ticks
$oldSecurity = (Get-Acl -LiteralPath $appPath).Sddl
$token = Get-Token (New-TowavueUpdateTransaction @pair)
Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null
Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null
$restored = Get-Item -LiteralPath $appPath
Assert-True ($restored.CreationTimeUtc.Ticks -eq $oldCreation -and $restored.LastWriteTimeUtc.Ticks -eq $oldWrite) 'Original timestamps were not restored.'
Assert-True ((Get-Acl -LiteralPath $appPath).Sddl -ceq $oldSecurity) 'Original security descriptor was not restored.'
Assert-True ((Get-Content -LiteralPath $appPath -Stream 'towavue-fixture' -Encoding UTF8 -Raw) -ceq 'preserve original alternate stream') 'Original alternate stream was not restored.'

$pair = New-Pair 'offline-recovery'
$before = Get-Snapshot $pair
$token = Get-Token (New-TowavueUpdateTransaction @pair)
Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null
$offline = Join-Path (Split-Path -Parent $pair.IncomingPayloadDirectory) 'offline-incoming'
Assert-True ($pair.IncomingPayloadDirectory.StartsWith($trialRoot + '\') -and $offline.StartsWith($trialRoot + '\')) 'Offline trial move escaped scratch.'
[IO.Directory]::Move($pair.IncomingPayloadDirectory,$offline)
Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null
[IO.Directory]::Move($offline,$pair.IncomingPayloadDirectory)
Assert-True ((Get-Snapshot $pair) -ceq $before) 'Recovery depended on the original incoming directory.'

$alias = Join-Path $trialRoot ('.towavue-update-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Junction -Path $alias -Target $token.TransactionDirectory | Out-Null
Expect-Failure { Invoke-TowavueUpdateTransaction -Mode Rollback -TransactionDirectory $alias -JournalSha256 $token.JournalSha256 } 'reparse points'
Assert-True ((Get-Snapshot $pair) -ceq $before) 'Rejected transaction alias changed files.'

foreach ($mode in @('Apply','Rollback')) {
    $pair = New-Pair ("locked-$mode")
    $token = Get-Token (New-TowavueUpdateTransaction @pair)
    if ($mode -eq 'Rollback') { Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null }
    $before = Get-Snapshot $pair
    $lock = [IO.File]::Open((Join-Path $pair.InstallDirectory 'towavue.exe'),[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
    try { Expect-Failure { Invoke-TowavueUpdateTransaction -Mode $mode @token } 'being used by another process' }
    finally { $lock.Dispose() }
    Assert-True ((Get-Snapshot $pair) -ceq $before) 'Locked transaction partially changed files.'
    Invoke-TowavueUpdateTransaction -Mode $mode @token | Out-Null
}
foreach ($kind in @('journal','backup','stage','old-change','new-change','keep-change','new-missing','incoming-change','collision','retired-change','receipt-change')) {
    $pair = New-Pair $kind
    $token = Get-Token (New-TowavueUpdateTransaction @pair)
    $mode = 'Apply'
    $message = switch ($kind) {
        'journal' { [IO.File]::AppendAllText((Join-Path $token.TransactionDirectory 'journal.json'),' '); 'journal identity differs' }
        'backup' { Write-Trial (Join-Path $token.TransactionDirectory '2.old') 'corrupt'; 'recovery source differs' }
        'stage' { Write-Trial (Join-Path $token.TransactionDirectory '0.new') 'corrupt'; 'recovery source differs' }
        'old-change' { Write-Trial (Join-Path $pair.InstallDirectory 'towavue.exe') 'user replacement'; 'differs from its recorded bytes' }
        'new-change' { Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null; Write-Trial (Join-Path $pair.InstallDirectory 'towavue.exe') 'user replacement'; $mode = 'Rollback'; 'unknown state' }
        'keep-change' { Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null; Write-Trial (Join-Path $pair.InstallDirectory 'keep.dll') 'user replacement of an unchanged file'; $mode = 'Rollback'; 'unknown state' }
        'new-missing' { Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null; [IO.File]::Move((Join-Path $pair.InstallDirectory 'towavue.exe'),(Join-Path $trialRoot 'removed-by-user.exe')); $mode = 'Rollback'; 'unknown state' }
        'incoming-change' { Write-Trial (Join-Path $pair.IncomingPayloadDirectory 'towavue.exe') 'changed input'; 'differs from its recorded bytes' }
        'collision' { Write-Trial (Join-Path $pair.InstallDirectory 'added.dll') 'user addition'; 'unowned path' }
        'retired-change' { Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null; Write-Trial (Join-Path $token.TransactionDirectory '2.apply-retired') 'modified retired file'; $mode = 'Rollback'; 'Retired update file differs' }
        'receipt-change' { Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null; Write-Trial (Join-Path $token.TransactionDirectory '0.published') 'modified receipt'; $mode = 'Rollback'; 'Invalid update publication receipt' }
    }
    $before = Get-Snapshot $pair
    Expect-Failure { Invoke-TowavueUpdateTransaction -Mode $mode @token } $message
    Assert-True ((Get-Snapshot $pair) -ceq $before) "Rejected $kind changed installed files."
}

# Inject faults only into private generated copies; production has no test switch.
$injectionDirectory = Join-Path $trialRoot 'injected'
New-Item -ItemType Directory -Path $injectionDirectory | Out-Null
foreach ($name in @('setup-update-paths.ps1','setup-update-registration.ps1','setup-update-native.cs','get-setup-update-plan.ps1')) { [IO.File]::Copy((Join-Path $PSScriptRoot $name),(Join-Path $injectionDirectory $name)) }
$original = [IO.File]::ReadAllText($library)
$injectedPath = Join-Path $injectionDirectory 'setup-update-transaction.ps1'
$pair = New-Pair 'copy-denied-recovery'
$before = Get-Snapshot $pair
$token = Get-Token (New-TowavueUpdateTransaction @pair)
Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null
$noCopy = $original.Replace('[TowavueUpdateFiles]::Copy(',"throw 'Injected copy failure'; [TowavueUpdateFiles]::Copy(")
Assert-True ($noCopy -cne $original) 'Copy failure injection missed the copy primitive.'
Write-Trial $injectedPath $noCopy
. $injectedPath
Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null
. $library
Assert-True ((Get-Snapshot $pair) -ceq $before) 'Recovery allocated a replacement copy.'

$pair = New-Pair 'missing-retired-original'
$token = Get-Token (New-TowavueUpdateTransaction @pair)
Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null
$retired = Join-Path $token.TransactionDirectory '2.apply-retired'
$heldElsewhere = Join-Path $token.TransactionDirectory '2.user-moved-original'
Assert-True ($retired.StartsWith($trialRoot + '\') -and $heldElsewhere.StartsWith($trialRoot + '\')) 'Retirement trial escaped scratch.'
[IO.File]::Move($retired,$heldElsewhere)
$before = Get-Snapshot $pair
Expect-Failure { Invoke-TowavueUpdateTransaction -Mode Rollback @token } 'Original retired file'
Assert-True ((Get-Snapshot $pair) -ceq $before) 'Missing original caused a partial rollback.'
[IO.File]::Move($heldElsewhere,$retired)
$lock = [IO.File]::Open($retired,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
try { Expect-Failure { Invoke-TowavueUpdateTransaction -Mode Rollback @token } 'being used by another process' }
finally { $lock.Dispose() }
Assert-True ((Get-Snapshot $pair) -ceq $before) 'Locked original caused a partial rollback.'
Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null

$matches = [regex]::Matches($original,'(?m)^\s*\[TowavueUpdateFiles\]::Move\([^\r\n]+')
Assert-True ($matches.Count -eq 3) 'Fault injection no longer matches all mutation sites.'
foreach ($mode in @('Apply','Rollback')) {
foreach ($fault in 1..10) {
    $pair = New-Pair ("fault-$mode-$fault")
    $before = Get-Snapshot $pair
    $token = Get-Token (New-TowavueUpdateTransaction @pair)
    if ($mode -eq 'Rollback') { Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null }
    $injected = [regex]::Replace($original,'(?m)^(\s*\[TowavueUpdateFiles\]::Move\([^\r\n]+)','$1; Invoke-TrialFault')
    Write-Trial $injectedPath ($injected + "`nfunction Invoke-TrialFault { `$script:trialMoves++; if (`$script:trialMoves -eq $fault) { throw 'Injected interruption' } }`n`$script:trialMoves = 0`n")
    . $injectedPath
    Expect-Failure { Invoke-TowavueUpdateTransaction -Mode $mode @token } 'Injected interruption'
    . $library
    Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null
    Assert-True ((Get-Snapshot $pair) -ceq $before) "Failed recovery after $mode mutation $fault."
}
}

# Abrupt child exit releases OS handles without running transaction finally.
$pair = New-Pair 'process-exit'
$before = Get-Snapshot $pair
$token = Get-Token (New-TowavueUpdateTransaction @pair)
Write-Trial $injectedPath ($injected + "`nfunction Invoke-TrialFault { `$script:trialMoves++; if (`$script:trialMoves -eq 3) { [Diagnostics.Process]::GetCurrentProcess().Kill() } }`n`$script:trialMoves = 0`n")
$childPath = Join-Path $trialRoot 'interrupt-child.ps1'
$escape = { param($Value) $Value.Replace("'","''") }
Write-Trial $childPath (". '" + (& $escape $injectedPath) + "'`nInvoke-TowavueUpdateTransaction -Mode Apply -TransactionDirectory '" + (& $escape $token.TransactionDirectory) + "' -JournalSha256 '" + $token.JournalSha256 + "'`n")
$powerShell = (Get-Process -Id $PID).Path
$child = Start-Process -FilePath $powerShell -ArgumentList @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',('"' + $childPath + '"')) -WindowStyle Hidden -PassThru
$childHandle = $child.Handle
Assert-True ($child.WaitForExit(30000)) "Owned interruption child still running: $($child.Id)"
Assert-True ($child.ExitCode -ne 0) 'Interrupted child unexpectedly succeeded.'
Assert-True (-not (Test-Path -LiteralPath (Join-Path $pair.InstallDirectory 'licenses/INSTALLED-FILES.json'))) 'Child did not interrupt the retire/publish gap.'
Assert-True (-not (Test-Path -LiteralPath (Join-Path $pair.InstallDirectory 'towavue.exe'))) 'Interrupted installation can launch a mixed payload.'
Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null
Assert-True ((Get-Snapshot $pair) -ceq $before) 'Abrupt process exit was not recoverable.'

# Coordinate real, test-only registry values with the same file journal.
$registrationState = Join-Path $repositoryRoot 'packaging/windows/registration-state.ps1'
. $registrationState
$registrationSource = [IO.File]::ReadAllText($registrationState)
$privateRegistration = Join-Path $trialRoot 'packaging/windows/registration-state.ps1'
Write-Trial $privateRegistration $registrationSource
function Get-TrialRegistrationArguments($Pair) {
    return @{
        InstallDirectory=$Pair.InstallDirectory
        OwnershipId=('towavue-local-' + (Get-FileHash -LiteralPath (Join-Path $Pair.InstallDirectory 'licenses/INSTALLED-FILES.json')).Hash.ToLowerInvariant())
        SizeKiB=123
        RegistrySubKey=$Pair.Registration.RegistrySubKey
        ShortcutPath=$Pair.Registration.ShortcutPath
    }
}
function New-RegisteredPair([string]$Name) {
    $pair = New-Pair $Name
    $programs = Join-Path (Split-Path -Parent $pair.InstallDirectory) 'programs'
    New-Item -ItemType Directory -Path $programs | Out-Null
    $pair.Registration = @{RegistrySubKey=('Software\towavue\InstallerTests\' + [guid]::NewGuid().ToString('N'));ShortcutPath=(Join-Path $programs 'fixture.lnk')}
    $arguments = Get-TrialRegistrationArguments $pair
    Invoke-TowavueRegistration @arguments -Mode Install | Out-Null
    return $pair
}
function Get-TrialRegistrationSnapshot($Pair) {
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
    $key = $null
    try {
        $key = $base.OpenSubKey($Pair.Registration.RegistrySubKey)
        $values = [ordered]@{}
        foreach ($name in $key.GetValueNames() | Sort-Object) { $values[$name] = @($key.GetValueKind($name).ToString(),$key.GetValue($name)) }
        return [ordered]@{values=$values;shortcut=(Get-FileHash -LiteralPath $Pair.Registration.ShortcutPath).Hash} | ConvertTo-Json -Depth 5 -Compress
    } finally { if ($key) { $key.Dispose() }; $base.Dispose() }
}
function Remove-TrialRegistration($Pair) {
    $arguments = Get-TrialRegistrationArguments $Pair
    Invoke-TowavueRegistration @arguments -Mode Remove | Out-Null
    Assert-True (-not (Test-Path -LiteralPath $Pair.Registration.ShortcutPath)) 'Joint trial shortcut remains.'
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
    try { $key = $base.OpenSubKey($Pair.Registration.RegistrySubKey); if ($key) { $key.Dispose(); throw 'Joint trial registration remains.' } } finally { $base.Dispose() }
}
$pair = New-RegisteredPair 'joined-baseline'
$before = Get-Snapshot $pair
$beforeRegistration = Get-TrialRegistrationSnapshot $pair
$token = Get-Token (New-TowavueUpdateTransaction @pair)
$journal = Get-Content -LiteralPath (Join-Path $token.TransactionDirectory 'journal.json') -Raw -Encoding UTF8 | ConvertFrom-Json
Assert-True ($journal.schema_version -eq 2 -and $journal.registration.PreviousSizeKiB -eq 123 -and $journal.registration.OwnershipId -ceq $pair.IncomingOwnershipId) 'Typed registration capture is not versioned and bound to the payload.'
$result = Invoke-TowavueUpdateTransaction -Mode Apply @token
Assert-True ($result.state -eq 'files_and_registration_applied') 'Joined update returned a file-only result.'
Assert-Applied $pair $token
Invoke-TowavueUpdateRegistration $journal.registration 'Apply' -VerifyOnly
$appliedRegistration = Get-TrialRegistrationSnapshot $pair | ConvertFrom-Json
Assert-True ($appliedRegistration.values.TowavueOwnershipId[1] -ceq $journal.registration.OwnershipId -and $appliedRegistration.values.EstimatedSize[1] -eq $journal.registration.SizeKiB) 'Joined update did not publish the target registration values.'
$result = Invoke-TowavueUpdateTransaction -Mode Rollback @token
Assert-True ($result.state -eq 'files_and_registration_rolled_back') 'Joined rollback result differs.'
Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null
Assert-True ((Get-Snapshot $pair) -ceq $before -and (Get-TrialRegistrationSnapshot $pair) -ceq $beforeRegistration) 'Joined rollback did not restore both states.'
Remove-TrialRegistration $pair

foreach ($mode in @('Apply','Rollback')) {
foreach ($fault in 1..10) {
    $pair = New-RegisteredPair ("joined-file-$mode-$fault")
    $before = Get-Snapshot $pair
    $beforeRegistration = Get-TrialRegistrationSnapshot $pair
    $token = Get-Token (New-TowavueUpdateTransaction @pair)
    if ($mode -eq 'Rollback') { Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null }
    Write-Trial $injectedPath ($injected + "`nfunction Invoke-TrialFault { `$script:trialMoves++; if (`$script:trialMoves -eq $fault) { throw 'Injected joined interruption' } }`n`$script:trialMoves = 0`n")
    . $injectedPath
    Expect-Failure { Invoke-TowavueUpdateTransaction -Mode $mode @token } 'Injected joined interruption'
    . $library
    Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null
    Assert-True ((Get-Snapshot $pair) -ceq $before -and (Get-TrialRegistrationSnapshot $pair) -ceq $beforeRegistration) "Joined recovery failed after $mode move $fault."
    Remove-TrialRegistration $pair
}
foreach ($write in @("`$key.SetValue('EstimatedSize',`$SizeKiB,[Microsoft.Win32.RegistryValueKind]::DWord)","`$key.SetValue('TowavueOwnershipId',`$OwnershipId,[Microsoft.Win32.RegistryValueKind]::String)")) {
    $pair = New-RegisteredPair ('joined-registry-' + [guid]::NewGuid().ToString('N'))
    $before = Get-Snapshot $pair
    $beforeRegistration = Get-TrialRegistrationSnapshot $pair
    $token = Get-Token (New-TowavueUpdateTransaction @pair)
    if ($mode -eq 'Rollback') { Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null }
    Write-Trial $injectedPath $original
    $injectedRegistration = $registrationSource.Replace($write,($write + "; throw 'Injected registry interruption'"))
    Assert-True ($injectedRegistration -cne $registrationSource) 'Joined registry injection missed its write.'
    Write-Trial $privateRegistration $injectedRegistration
    . $injectedPath
    Expect-Failure { Invoke-TowavueUpdateTransaction -Mode $mode @token } 'Injected registry interruption'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $pair.InstallDirectory 'towavue.exe'))) 'Registry failure published the application executable.'
    . $library
    Write-Trial $privateRegistration $registrationSource
    Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null
    Assert-True ((Get-Snapshot $pair) -ceq $before -and (Get-TrialRegistrationSnapshot $pair) -ceq $beforeRegistration) 'Joined registry interruption failed to restore both states.'
    Remove-TrialRegistration $pair
}
}

$pair = New-RegisteredPair 'joined-unknown-registration'
$token = Get-Token (New-TowavueUpdateTransaction @pair)
Invoke-TowavueUpdateTransaction -Mode Apply @token | Out-Null
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
$key = $base.OpenSubKey($pair.Registration.RegistrySubKey,$true)
try { $key.SetValue('EstimatedSize',999,[Microsoft.Win32.RegistryValueKind]::DWord) } finally { $key.Dispose(); $base.Dispose() }
$before = Get-Snapshot $pair
$beforeRegistration = Get-TrialRegistrationSnapshot $pair
Expect-Failure { Invoke-TowavueUpdateTransaction -Mode Rollback @token } 'unknown identity or size'
Assert-True ((Get-Snapshot $pair) -ceq $before -and (Get-TrialRegistrationSnapshot $pair) -ceq $beforeRegistration) 'Unknown registration caused a partial file rollback.'
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
$key = $base.OpenSubKey($pair.Registration.RegistrySubKey,$true)
try { $key.SetValue('EstimatedSize',123,[Microsoft.Win32.RegistryValueKind]::DWord) } finally { $key.Dispose(); $base.Dispose() }
Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null
Remove-TrialRegistration $pair

$pair = New-RegisteredPair 'joined-process-exit'
$before = Get-Snapshot $pair
$beforeRegistration = Get-TrialRegistrationSnapshot $pair
$token = Get-Token (New-TowavueUpdateTransaction @pair)
$journal = Get-Content -LiteralPath (Join-Path $token.TransactionDirectory 'journal.json') -Raw -Encoding UTF8 | ConvertFrom-Json
Write-Trial $injectedPath $original
$write = "`$key.SetValue('EstimatedSize',`$SizeKiB,[Microsoft.Win32.RegistryValueKind]::DWord)"
Write-Trial $privateRegistration ($registrationSource.Replace($write,($write + '; [Diagnostics.Process]::GetCurrentProcess().Kill()')))
$childPath = Join-Path $trialRoot 'joined-interrupt-child.ps1'
Write-Trial $childPath (". '" + (& $escape $injectedPath) + "'`nInvoke-TowavueUpdateTransaction -Mode Apply -TransactionDirectory '" + (& $escape $token.TransactionDirectory) + "' -JournalSha256 '" + $token.JournalSha256 + "'`n")
$child = Start-Process -FilePath $powerShell -ArgumentList @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',('"' + $childPath + '"')) -WindowStyle Hidden -PassThru
$childHandle = $child.Handle
Assert-True ($child.WaitForExit(30000)) "Owned joined interruption child still running: $($child.Id)"
Assert-True ($child.ExitCode -ne 0 -and -not (Test-Path -LiteralPath (Join-Path $pair.InstallDirectory 'towavue.exe'))) 'Registry child did not stop before executable publication.'
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
$key = $base.OpenSubKey($pair.Registration.RegistrySubKey)
try { Assert-True ($key.GetValue('EstimatedSize') -eq $journal.registration.SizeKiB -and $key.GetValue('TowavueOwnershipId') -ceq $journal.registration.PreviousOwnershipId) 'Child did not stop at the intended partial registry state.' }
finally { $key.Dispose(); $base.Dispose() }
Write-Trial $privateRegistration $registrationSource
Invoke-TowavueUpdateTransaction -Mode Rollback @token | Out-Null
Assert-True ((Get-Snapshot $pair) -ceq $before -and (Get-TrialRegistrationSnapshot $pair) -ceq $beforeRegistration) 'Abrupt joined process exit was not recoverable.'
Remove-TrialRegistration $pair

$pair = New-RegisteredPair 'joined-wrong-binding'
$before = Get-Snapshot $pair
$beforeRegistration = Get-TrialRegistrationSnapshot $pair
$token = Get-Token (New-TowavueUpdateTransaction @pair)
$journalPath = Join-Path $token.TransactionDirectory 'journal.json'
$journal = Get-Content -LiteralPath $journalPath -Raw -Encoding UTF8 | ConvertFrom-Json
$journal.registration.InstallDirectory = Split-Path -Parent $pair.InstallDirectory
Write-Trial $journalPath ($journal | ConvertTo-Json -Depth 12)
# Even an explicitly supplied altered digest cannot bind another install path.
$token.JournalSha256 = (Get-FileHash -LiteralPath $journalPath).Hash.ToLowerInvariant()
Expect-Failure { Invoke-TowavueUpdateTransaction -Mode Apply @token } 'registration does not match the file journal'
Assert-True ((Get-Snapshot $pair) -ceq $before -and (Get-TrialRegistrationSnapshot $pair) -ceq $beforeRegistration) 'Invalid registration binding changed either state.'
Remove-TrialRegistration $pair

$pair = New-RegisteredPair 'joined-missing-registration'
Remove-TrialRegistration $pair
$before = Get-Snapshot $pair
Expect-Failure { New-TowavueUpdateTransaction @pair } 'existing update registration is missing'
Assert-True ((Get-Snapshot $pair) -ceq $before) 'Missing registration preparation changed files.'
Assert-True (-not @(Get-ChildItem -LiteralPath (Split-Path -Parent $pair.InstallDirectory) -Directory -Force -Filter '.towavue-update-*').Count) 'Missing registration left a prepared transaction.'
Write-Output 'PASS: one file/registration journal, typed capture, twenty joined move interruptions, four registry write interruptions, abrupt child exit between registry writes, executable publication order, journal binding, unknown/missing registration preservation and repeated rollback. Joint test keys/shortcuts removed.'
Write-Output "PASS: file-only prepare/apply/rollback, original identity/timestamps/security/alternate stream, copy-free recovery, preservation, locks, hard links, corrupt inputs, ten apply and ten rollback boundaries, abrupt child exit in a retire/publish gap: $trialRoot"
Write-Output 'SKIP: actual Setup lifecycle/uninstaller coordination, full-volume/ACL-denial recovery, OS power loss and hostile concurrent namespace changes are not exercised.'
