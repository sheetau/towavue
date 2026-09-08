[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$planner = Join-Path $PSScriptRoot 'get-setup-update-plan.ps1'
$trialRoot = Join-Path $repositoryRoot ('target/tmp/update-plan-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $trialRoot | Out-Null
$utf8 = [Text.UTF8Encoding]::new($false)
function Assert-True([bool]$Condition,[string]$Message) { if (-not $Condition) { throw $Message } }
function Write-Trial([string]$Path,[string]$Text) {
    Assert-True ([IO.Path]::GetFullPath($Path).StartsWith($trialRoot + '\',[StringComparison]::OrdinalIgnoreCase)) 'Fixture escaped scratch.'
    New-Item -ItemType Directory -Path (Split-Path -Parent $Path) -Force | Out-Null
    [IO.File]::WriteAllText($Path,$Text,$utf8)
}
function Set-Inventory([string]$Root,$Records) {
    Write-Trial (Join-Path $Root 'licenses/INSTALLED-FILES.json') ([ordered]@{schema_version=1;files=@($Records)} | ConvertTo-Json -Depth 6)
    return 'towavue-local-' + (Get-FileHash -LiteralPath (Join-Path $Root 'licenses/INSTALLED-FILES.json')).Hash.ToLowerInvariant()
}
function Set-Marker([string]$Root,[string]$Identity) {
    [IO.File]::WriteAllText((Join-Path $Root 'towavue-install.ini'),"[installation]`r`nid=$Identity`r`ndirectory=$Root`r`n",[Text.Encoding]::Unicode)
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
        $identity = Set-Inventory $directory $records
        if ($old) { Set-Marker $directory $identity; Write-Trial (Join-Path $directory 'Uninstall.exe') 'uninstaller fixture, never executed' }
    }
    Write-Trial (Join-Path $installed 'user-media.txt') 'preserve user data'
    return @{InstallDirectory=$installed;IncomingPayloadDirectory=$incoming;IncomingOwnershipId=$identity}
}
function Get-Snapshot([hashtable]$Pair) {
    $snapshot = @{}
    foreach ($root in @($Pair.InstallDirectory,$Pair.IncomingPayloadDirectory)) {
        Get-ChildItem -LiteralPath $root -Recurse -File -Force | ForEach-Object { $snapshot.Add($_.FullName,(Get-FileHash -LiteralPath $_.FullName).Hash) }
    }
    $ordered = [ordered]@{}
    foreach ($name in @($snapshot.Keys | Sort-Object)) { $ordered[$name] = $snapshot[$name] }
    return $ordered | ConvertTo-Json -Compress
}
function Expect-Failure([hashtable]$Pair,[string]$Message) {
    $before = Get-Snapshot $Pair
    $rejected = $false
    try { & $planner @Pair | Out-Null }
    catch { if (-not $_.Exception.Message.Contains($Message)) { throw }; $rejected = $true }
    Assert-True $rejected "Unsafe update plan accepted: $Message"
    Assert-True ((Get-Snapshot $Pair) -ceq $before) 'Rejected plan modified files.'
}
$pair = New-Pair 'baseline'
$before = Get-Snapshot $pair
$plan = & $planner @pair | ConvertFrom-Json
foreach ($case in @(@('towavue.exe','replace'),@('keep.dll','keep'),@('remove.dll','remove'),@('added.dll','add'),@('licenses/INSTALLED-FILES.json','replace'))) {
    $action = @($plan.actions | Where-Object { $_.name -eq $case[0] })
    Assert-True ($action.Count -eq 1 -and $action[0].action -eq $case[1]) 'Wrong planned file action.'
}
Assert-True ($plan.actions.Count -eq 5 -and $plan.old_metadata.Count -eq 2 -and -not $plan.identical_payload) 'Wrong update coverage.'
Assert-True (-not @($plan.actions | Where-Object { $_.name -eq 'user-media.txt' }).Count) 'User file was claimed by updater.'
Assert-True ((Get-Snapshot $pair) -ceq $before) 'Successful plan modified files.'
$repeat = & $planner @pair
Push-Location $env:TEMP
try { Assert-True ((& $planner @pair) -ceq $repeat) 'Working directory changed the plan.' } finally { Pop-Location }
Copy-Item -LiteralPath (Join-Path $pair.IncomingPayloadDirectory 'towavue.exe') -Destination (Join-Path $pair.InstallDirectory 'towavue.exe')
Expect-Failure $pair 'differs from its recorded bytes'

foreach ($name in @('../escape.dll','C:/outside.dll','dir//x.dll','dir/../x.dll','bad:stream','bad*.dll','CON','LPT1.txt','dir/trailing.','Uninstall.exe','towavue-install.ini','licenses/INSTALLED-FILES.json')) {
    $pair = New-Pair ('unsafe-' + [guid]::NewGuid().ToString('N'))
    $inventory = Get-Content -LiteralPath (Join-Path $pair.IncomingPayloadDirectory 'licenses/INSTALLED-FILES.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $inventory.files[0].name = $name
    $pair.IncomingOwnershipId = Set-Inventory $pair.IncomingPayloadDirectory $inventory.files
    Expect-Failure $pair $(if ($name -in @('Uninstall.exe','towavue-install.ini','licenses/INSTALLED-FILES.json')) { 'reserved installer metadata' } else { 'Unsafe update inventory name' })
}
foreach ($kind in @('duplicate','case-duplicate','size','hash','no-app','file-directory','missing-file')) {
    $pair = New-Pair $kind
    $inventory = Get-Content -LiteralPath (Join-Path $pair.IncomingPayloadDirectory 'licenses/INSTALLED-FILES.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $message = switch ($kind) {
        'duplicate' { $inventory.files += $inventory.files[0]; 'Duplicate' }
        'case-duplicate' { $inventory.files += [pscustomobject]@{name='TOWAVUE.EXE';bytes=1;sha256=('a' * 64)}; 'Duplicate' }
        'size' { $inventory.files[0].bytes = '1'; 'file size' }
        'hash' { $inventory.files[0].sha256 = 'not-a-digest'; 'file identity' }
        'no-app' { $inventory.files = @($inventory.files | Where-Object { $_.name -ne 'towavue.exe' }); 'no application executable' }
        'file-directory' { $inventory.files[0].name = 'keep.dll/child'; 'file/directory collision' }
        'missing-file' { $inventory.files[0].name = 'missing.dll'; 'Missing update file' }
    }
    $pair.IncomingOwnershipId = Set-Inventory $pair.IncomingPayloadDirectory $inventory.files
    Expect-Failure $pair $message
}
$pair = New-Pair 'collision'
Write-Trial (Join-Path $pair.InstallDirectory 'added.dll') 'not owned by the installed version'
Expect-Failure $pair 'unowned path'
$pair = New-Pair 'extra-incoming'
Write-Trial (Join-Path $pair.IncomingPayloadDirectory 'unlisted.exe') 'not part of this candidate'
Expect-Failure $pair 'unlisted file'
$pair = New-Pair 'marker'
Set-Marker $pair.InstallDirectory ('towavue-local-' + ('b' * 64))
Expect-Failure $pair 'does not match its ownership identity'
Set-Marker $pair.InstallDirectory $pair.IncomingOwnershipId
$markerPath = Join-Path $pair.InstallDirectory 'towavue-install.ini'
[IO.File]::WriteAllText($markerPath,"[installation]`r`nid=$($pair.IncomingOwnershipId)`r`ndirectory=C:\elsewhere`r`n",[Text.Encoding]::Unicode)
Expect-Failure $pair 'another payload or directory'
Write-Trial $markerPath 'not a UTF16 marker'
Expect-Failure $pair 'UTF-16LE'
$pair = New-Pair 'locked'
$locked = [IO.File]::Open((Join-Path $pair.InstallDirectory 'towavue.exe'),[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
try {
    $rejected = $false
    try { & $planner @pair | Out-Null } catch { $rejected = $true }
    Assert-True $rejected 'Locked application was accepted for replacement.'
}
finally { $locked.Dispose() }
& $planner @pair | Out-Null
$pair = New-Pair 'junction'
$junction = Join-Path (Split-Path -Parent $pair.InstallDirectory) 'junction'
New-Item -ItemType Junction -Path $junction -Target $pair.InstallDirectory | Out-Null
$pair.InstallDirectory = $junction
$rejected = $false
try { & $planner @pair | Out-Null } catch { if (-not $_.Exception.Message.Contains('reparse points')) { throw }; $rejected = $true }
Assert-True $rejected 'Junction was accepted.'
$pair = New-Pair 'overlap'
$overlap = $pair.Clone(); $overlap.IncomingPayloadDirectory = $pair.InstallDirectory
$rejected = $false
try { & $planner @overlap | Out-Null } catch { if (-not $_.Exception.Message.Contains('must not overlap')) { throw }; $rejected = $true }
Assert-True $rejected 'Overlapping directories were accepted.'
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class UpdatePlanAliasFixture {
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode)]
    public static extern uint GetShortPathNameW(string path,StringBuilder output,uint size);
}
'@
$shortPath = [Text.StringBuilder]::new(32768)
$shortLength = [UpdatePlanAliasFixture]::GetShortPathNameW($pair.InstallDirectory,$shortPath,32768)
if ($shortLength -gt 0 -and $shortPath.ToString() -ne $pair.InstallDirectory) {
    $overlap.IncomingPayloadDirectory = $shortPath.ToString()
    $rejected = $false
    try { & $planner @overlap | Out-Null } catch { if (-not $_.Exception.Message.Contains('must not overlap')) { throw }; $rejected = $true }
    Assert-True $rejected 'DOS alias bypassed directory overlap protection.'
    $oldId = 'towavue-local-' + (Get-FileHash -LiteralPath (Join-Path $pair.InstallDirectory 'licenses/INSTALLED-FILES.json')).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText((Join-Path $pair.InstallDirectory 'towavue-install.ini'),"[installation]`r`nid=$oldId`r`ndirectory=$($shortPath.ToString())`r`n",[Text.Encoding]::Unicode)
    $aliasPlan = & $planner @pair | ConvertFrom-Json
    Assert-True ($aliasPlan.install_directory -eq $pair.InstallDirectory) 'A marker with an equivalent DOS directory alias was rejected.'
}
else { Write-Output 'SKIP: this fixture directory has no distinct DOS short path; alias normalization was not exercised.' }
Write-Output "PASS: read-only actions/metadata, replacement and user-file protection, repeat/cwd, twelve unsafe names, seven schema/identity cases, collision/extra payload, marker mismatch/encoding, lock/retry, overlap and junction refusal. Evidence: $trialRoot"
