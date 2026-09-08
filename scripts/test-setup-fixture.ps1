[CmdletBinding()]
param([Parameter(Mandatory)][string]$NsisArchive)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$manifest = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/nsis-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$NsisArchive = (Resolve-Path -LiteralPath $NsisArchive).Path
if ((Get-Item -LiteralPath $NsisArchive).Length -ne $manifest.archive.bytes -or
    (Get-FileHash -LiteralPath $NsisArchive).Hash -ne $manifest.archive.sha256) { throw 'NSIS archive identity mismatch.' }
$trialRoot = Join-Path $repositoryRoot ('target/tmp/setup-fixture-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $trialRoot | Out-Null
$utf8 = [Text.UTF8Encoding]::new($false)
function Assert-TrialPath([string]$Path) {
    $absolute = [IO.Path]::GetFullPath($Path)
    if (-not $absolute.StartsWith($trialRoot + '\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Path escapes the owned trial root.' }
}
function Write-TrialFile([string]$Path,[string]$Text) {
    Assert-TrialPath $Path
    $directory = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $directory)) { New-Item -ItemType Directory -Path $directory | Out-Null }
    [IO.File]::WriteAllText($Path,$Text,$utf8)
}
function Assert-True([bool]$Condition,[string]$Message) { if (-not $Condition) { throw $Message } }
function Get-Tree([string]$Directory) {
    $result = @{}
    Get-ChildItem -LiteralPath $Directory -File -Recurse -Force | ForEach-Object {
        $result[$_.FullName.Substring($Directory.Length + 1)] = (Get-FileHash -LiteralPath $_.FullName).Hash
    }
    return $result
}
function Assert-Tree([hashtable]$Before,[string]$Directory) {
    $after = Get-Tree $Directory
    Assert-True ($Before.Count -eq $after.Count) "Changed file count: $Directory"
    foreach ($name in $Before.Keys) { Assert-True ($Before[$name] -eq $after[$name]) "Changed file: $name" }
}
function Invoke-Fixture([string]$Executable,[string]$Arguments,[int]$Expected,[string]$WorkingDirectory=$trialRoot) {
    $process = Start-Process -FilePath $Executable -ArgumentList $Arguments -WorkingDirectory $WorkingDirectory -WindowStyle Hidden -PassThru
    [void]$process.Handle
    if (-not $process.WaitForExit(15000)) {
        Write-Output "Fixture remains live after 15 seconds; observing the same process: PID $($process.Id), $Executable $Arguments"
        if (-not $process.WaitForExit(45000)) { throw "Fixture still running after 60 seconds; do not restart: PID $($process.Id), started $($process.StartTime.ToUniversalTime().ToString('o')), $Executable $Arguments" }
    }
    Assert-True ($process.ExitCode -eq $Expected) "Wrong fixture exit $($process.ExitCode), expected $Expected : $Arguments"
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead($NsisArchive)
try {
    foreach ($entry in $archive.Entries) {
        if ($entry.FullName -notmatch '^nsis-3\.12/' -or $entry.FullName -match '(^/|(^|/)\.\.(/|$)|:|\\)' -or
            (($entry.ExternalAttributes -shr 16) -band 0xf000) -eq 0xa000) { throw 'Unsafe NSIS archive entry.' }
    }
}
finally { $archive.Dispose() }
$toolDirectory = Join-Path $trialRoot 'toolchain'
[IO.Compression.ZipFile]::ExtractToDirectory($NsisArchive,$toolDirectory)
$compiler = Join-Path $toolDirectory 'nsis-3.12/makensis.exe'
$version = & $compiler /VERSION
Assert-True ($LASTEXITCODE -eq 0 -and $version -eq 'v3.12') 'Wrong NSIS compiler version.'
$badArchive = Join-Path $trialRoot 'not-the-nsis-archive.zip'
Write-TrialFile $badArchive 'A deliberately invalid archive.'
$rejected = $false
try { & $PSCommandPath -NsisArchive $badArchive | Out-Null }
catch {
    if ($_.Exception.Message -ne 'NSIS archive identity mismatch.') { throw }
    $rejected = $true
}
Assert-True $rejected 'Unverified compiler archive accepted.'
Write-TrialFile (Join-Path $trialRoot 'payload/fixture.txt') "Harmless towavue installer lifecycle fixture, not the application.`r`n"
Write-TrialFile (Join-Path $trialRoot 'payload/licenses/START-HERE.html') '<!doctype html><html lang="en"><meta charset="utf-8"><title>Fixture only</title><h1>Installer test documents</h1><p>No app, FFmpeg or VC runtime is included.</p><a href="NSIS-COPYING.txt">Original NSIS notices</a></html>'
Copy-Item -LiteralPath (Join-Path $toolDirectory 'nsis-3.12/COPYING') -Destination (Join-Path $trialRoot 'payload/licenses/NSIS-COPYING.txt')
$payloadBefore = Get-Tree (Join-Path $trialRoot 'payload')
$script = Join-Path $repositoryRoot 'packaging/windows/setup.nsi'
Copy-Item -LiteralPath (Join-Path $repositoryRoot 'packaging/windows/operation-lock.ps1') -Destination (Join-Path $trialRoot 'operation-lock.ps1')
Invoke-Fixture $compiler ('/NOCONFIG /V2 "{0}"' -f $script) 1
# NSIS expands dollar signs in quoted literals; escape the generated define, not shell variables.
$compilerTrialRoot = $trialRoot.Replace('$','$$')
& $compiler /NOCONFIG /WX /V3 /DTOWAVUE_SETUP_FIXTURE "/DTRIAL_ROOT=$compilerTrialRoot" $script
Assert-True ($LASTEXITCODE -eq 0) 'NSIS fixture compilation failed.'
$setup = Join-Path $trialRoot 'Setup-fixture.exe'

# Only CHECKONLY may inspect broad or external paths; it exits before file operations.
$driveRoot = [IO.Path]::GetPathRoot($trialRoot)
Invoke-Fixture $setup "/S /CHECKONLY /CHECKPATH=$driveRoot" 2
Invoke-Fixture $setup '/S /CHECKONLY /CHECKPATH=\\localhost\not-a-towavue-test' 2
# An occupied cwd previously hid drive-root sanitization: NSIS stores C:\ as C:,
# which Windows resolves to the current directory. Exercise an empty cwd as well.
$emptyWorkingDirectory = Join-Path $trialRoot 'empty-working-directory'
New-Item -ItemType Directory -Path $emptyWorkingDirectory | Out-Null
foreach ($path in @($driveRoot,$driveRoot.TrimEnd('\'),($driveRoot.TrimEnd('\') + 'relative'),'relative-folder')) {
    Invoke-Fixture $setup "/S /CHECKONLY /CHECKPATH=$path" 2 $emptyWorkingDirectory
}
Assert-True (@(Get-ChildItem -LiteralPath $emptyWorkingDirectory -Force).Count -eq 0) 'Root/relative probe wrote to its working directory.'
Assert-True (-not (Test-Path -LiteralPath (Join-Path $trialRoot 'manual-trial'))) 'Read-only probe installed files.'

$occupied = Join-Path $trialRoot 'occupied'
Write-TrialFile (Join-Path $occupied 'keep.txt') 'Do not overwrite me.'
$occupiedBefore = Get-Tree $occupied
Invoke-Fixture $setup "/S /D=$occupied" 2
Assert-Tree $occupiedBefore $occupied
$fileParent = Join-Path $trialRoot 'not-a-directory'
Write-TrialFile $fileParent 'Do not replace this file.'
Invoke-Fixture $setup "/S /D=$fileParent\child" 2
Assert-True ([IO.File]::ReadAllText($fileParent) -eq 'Do not replace this file.') 'Changed parent file.'
Assert-True (-not (Test-Path -LiteralPath (Join-Path $trialRoot 'manual-trial'))) 'Invalid explicit destination silently used the default.'
$nonemptyDirectory = Join-Path $trialRoot 'nonempty-directory'
New-Item -ItemType Directory -Path (Join-Path $nonemptyDirectory 'empty-child') | Out-Null
Invoke-Fixture $setup "/S /D=$nonemptyDirectory" 2
Assert-True (@(Get-ChildItem -LiteralPath $nonemptyDirectory -Force).Count -eq 1) 'Changed occupied directory.'

# A real install/uninstall is restricted to fresh scratch folders with document fixtures.
$japaneseName = ([string][char]0x65e5) + [char]0x672c + [char]0x8a9e
$installed = Join-Path $trialRoot ("$japaneseName space & dollar `$ folder")
Invoke-Fixture $setup "/S /D=$installed" 0
foreach ($name in $payloadBefore.Keys) {
    Assert-True ((Get-FileHash -LiteralPath (Join-Path $installed $name)).Hash -eq $payloadBefore[$name]) "Installed payload mismatch: $name"
}
$installedBefore = Get-Tree $installed
Assert-True ($installedBefore.Count -eq 5) 'Wrong installed file count.'
Invoke-Fixture $setup "/S /D=$installed" 2
Assert-Tree $installedBefore $installed

# A copied driver avoids the NSIS self-copy parent's early exit. The explicit _?=
# directory keeps one waited process; it is never used outside this owned trial root.
$driver = Join-Path $trialRoot 'uninstall-driver.exe'
Copy-Item -LiteralPath (Join-Path $installed 'Uninstall-fixture.exe') -Destination $driver
$unowned = Join-Path $trialRoot 'unowned'
Write-TrialFile (Join-Path $unowned 'fixture.txt') 'This filename alone does not establish ownership.'
$unownedBefore = Get-Tree $unowned
Invoke-Fixture $driver "/S _?=$unowned" 2
Assert-Tree $unownedBefore $unowned
$marker = Join-Path $installed 'towavue-fixture.ini'
$markerBytes = [IO.File]::ReadAllBytes($marker)
# A valid marker from a different path must not grant ownership.
Copy-Item -LiteralPath $marker -Destination (Join-Path $unowned 'towavue-fixture.ini')
$movedMarkerBefore = Get-Tree $unowned
Invoke-Fixture $driver "/S _?=$unowned" 2
Assert-Tree $movedMarkerBefore $unowned
Write-TrialFile $marker '[fixture]'
Invoke-Fixture $driver "/S _?=$installed" 2
Assert-True ((Get-FileHash -LiteralPath (Join-Path $installed 'fixture.txt')).Hash -eq $payloadBefore['fixture.txt']) 'Bad marker allowed deletion.'
[IO.File]::WriteAllBytes($marker,$markerBytes)

$linkTarget = Join-Path $trialRoot 'junction-target'
New-Item -ItemType Directory -Path $linkTarget | Out-Null
$junction = Join-Path $trialRoot 'junction'
New-Item -ItemType Junction -Path $junction -Target $linkTarget | Out-Null
Invoke-Fixture $setup "/S /D=$junction\child" 2
Assert-True (@(Get-ChildItem -LiteralPath $linkTarget -Force).Count -eq 0) 'Installation followed a junction.'

# A nested replaced directory must fail before even deleting the top-level payload.
$licenses = Join-Path $installed 'licenses'
$savedLicenses = Join-Path $installed 'saved-licenses'
Assert-TrialPath $licenses
Assert-TrialPath $savedLicenses
Move-Item -LiteralPath $licenses -Destination $savedLicenses
New-Item -ItemType Junction -Path $licenses -Target $savedLicenses | Out-Null
Invoke-Fixture $driver "/S _?=$installed" 2
Assert-True ((Get-FileHash -LiteralPath (Join-Path $installed 'fixture.txt')).Hash -eq $payloadBefore['fixture.txt']) 'Uninstall started before the nested path check.'
# Directory.Delete without recursion removes only this verified junction, not its target.
Assert-True (((Get-Item -LiteralPath $licenses).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) 'Expected owned junction.'
Assert-TrialPath $licenses
[IO.Directory]::Delete($licenses)
Assert-TrialPath $savedLicenses
Assert-TrialPath $licenses
Move-Item -LiteralPath $savedLicenses -Destination $licenses

Write-TrialFile (Join-Path $installed 'user-media.txt') 'User media stays.'
Write-TrialFile (Join-Path $licenses 'user-note.txt') 'User notes stay.'
$locked = [IO.File]::Open((Join-Path $installed 'fixture.txt'),[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
try { Invoke-Fixture $driver "/S _?=$installed" 3 }
finally { $locked.Dispose() }
Assert-True (Test-Path -LiteralPath $marker) 'Failed uninstall removed recovery marker.'
Invoke-Fixture $driver "/S _?=$installed" 0
Assert-True ([IO.File]::ReadAllText((Join-Path $installed 'user-media.txt')) -eq 'User media stays.') 'User media was removed.'
Assert-True ([IO.File]::ReadAllText((Join-Path $licenses 'user-note.txt')) -eq 'User notes stay.') 'Nested user file was removed.'
Assert-True ((Get-Tree $installed).Count -eq 2) 'Owned files remain after retry.'

$empty = Join-Path $trialRoot 'empty-after-uninstall'
New-Item -ItemType Directory -Path $empty | Out-Null
Invoke-Fixture $setup "/S /D=$empty" 0
Invoke-Fixture $driver "/S _?=$empty" 0
Assert-True (-not (Test-Path -LiteralPath $empty)) 'Empty owned directory remains.'
$nested = Join-Path $trialRoot 'new-parent/new-child'
Invoke-Fixture $setup "/S /D=$nested\" 0
Invoke-Fixture $driver "/S _?=$nested" 0
Assert-True (-not (Test-Path -LiteralPath $nested)) 'New nested/trailing-slash directory did not uninstall.'

# The real NSIS sections must retain the same lease as the PowerShell entry
# point, not just perform a preflight. Gates exist only in a private script copy.
. (Join-Path $repositoryRoot 'packaging/windows/operation-lock.ps1')
$blockedDestination = Join-Path $trialRoot 'blocked-destination'
$lease = New-TowavueOperationLease 'towavue-setup-fixture-v1'
try { Invoke-Fixture $setup "/S /D=$blockedDestination" 4 }
finally { $lease.Dispose() }
Assert-True (-not (Test-Path -LiteralPath $blockedDestination)) 'Competing Setup created its destination.'
$gateRoot = Join-Path $trialRoot 'gated'
New-Item -ItemType Directory -Path $gateRoot | Out-Null
Copy-Item -LiteralPath (Join-Path $trialRoot 'payload') -Destination (Join-Path $gateRoot 'payload') -Recurse
Copy-Item -LiteralPath (Join-Path $trialRoot 'operation-lock.ps1') -Destination $gateRoot
$gateId = [guid]::NewGuid().ToString('N')
$readyName = 'Local\towavue-nsis-ready-' + $gateId
$releaseName = 'Local\towavue-nsis-release-' + $gateId
$ready = [Threading.EventWaitHandle]::new($false,[Threading.EventResetMode]::ManualReset,$readyName)
$release = [Threading.EventWaitHandle]::new($false,[Threading.EventResetMode]::ManualReset,$releaseName)
$gate = @'
  System::Call 'kernel32::OpenEventW(i 2, i 0, w "READY_NAME") p .r0'
  System::Call 'kernel32::SetEvent(p r0)'
  System::Call 'kernel32::CloseHandle(p r0)'
  System::Call 'kernel32::OpenEventW(i 0x100000, i 0, w "RELEASE_NAME") p .r0'
  System::Call 'kernel32::WaitForSingleObject(p r0, i 30000)'
  System::Call 'kernel32::CloseHandle(p r0)'
'@
$gate = $gate.Replace('READY_NAME',$readyName).Replace('RELEASE_NAME',$releaseName)
$gatedSource = [IO.File]::ReadAllText($script)
foreach ($call in @('  Call AcquireOperation','  Call un.AcquireOperation')) {
    Assert-True ($gatedSource.Contains($call)) 'NSIS lease gate missed its acquisition site.'
    $gatedSource = $gatedSource.Replace($call,($call + "`n" + $gate))
}
$gatedScript = Join-Path $gateRoot 'setup.nsi'
Write-TrialFile $gatedScript $gatedSource
& $compiler /NOCONFIG /WX /V2 /DTOWAVUE_SETUP_FIXTURE ("/DTRIAL_ROOT=" + $gateRoot.Replace('$','$$')) $gatedScript
Assert-True ($LASTEXITCODE -eq 0) 'Gated fixture compilation failed.'
$gatedSetup = Join-Path $gateRoot 'Setup-fixture.exe'
$gatedInstalled = Join-Path $gateRoot 'installed'
$gatedDriver = Join-Path $gateRoot 'uninstall-driver.exe'
try {
    foreach ($mode in @('install','uninstall','terminated-install')) {
        $ready.Reset() | Out-Null
        $release.Reset() | Out-Null
        $executable = if ($mode -eq 'uninstall') { $gatedDriver } else { $gatedSetup }
        $arguments = if ($mode -eq 'uninstall') { "/S _?=$gatedInstalled" } else { "/S /D=$gatedInstalled" }
        $process = Start-Process -FilePath $executable -ArgumentList $arguments -WorkingDirectory $gateRoot -WindowStyle Hidden -PassThru
        [void]$process.Handle
        Assert-True ($ready.WaitOne(15000)) "Owned gated NSIS process did not signal: $($process.Id)"
        $beforeGate = if (Test-Path -LiteralPath $gatedInstalled) { Get-Tree $gatedInstalled } else { $null }
        $rejected = $false
        try { $unexpectedLease = New-TowavueOperationLease 'towavue-setup-fixture-v1'; $unexpectedLease.Dispose() }
        catch { if (-not $_.Exception.Message.Contains('Another installation operation is active')) { throw }; $rejected = $true }
        Assert-True $rejected 'NSIS section did not retain the operation lease.'
        Invoke-Fixture $setup "/S /D=$blockedDestination" 4
        Invoke-Fixture $driver "/S _?=$gatedInstalled" 4
        Assert-True (-not (Test-Path -LiteralPath $blockedDestination)) 'Competing installer changed its destination.'
        if ($beforeGate) { Assert-Tree $beforeGate $gatedInstalled }
        else { Assert-True (-not (Test-Path -LiteralPath $gatedInstalled)) 'Competing process changed the gated installation.' }
        if ($mode -eq 'terminated-install') { $process.Kill() }
        else { $release.Set() | Out-Null }
        Assert-True ($process.WaitForExit(15000)) "Owned gated NSIS process still running: $($process.Id)"
        if ($mode -ne 'terminated-install') { Assert-True ($process.ExitCode -eq 0) 'Gated NSIS lifecycle failed.' }
        $lease = New-TowavueOperationLease 'towavue-setup-fixture-v1'
        $lease.Dispose()
        if ($mode -eq 'install') { Copy-Item -LiteralPath (Join-Path $gatedInstalled 'Uninstall-fixture.exe') -Destination $gatedDriver }
        else { Assert-True (-not (Test-Path -LiteralPath $gatedInstalled)) 'Gated removal/terminated pre-write install left files.' }
    }
    # Leave the private 30-second gate unsignalled. The 15-second observation
    # must retain this same process and accept its eventual normal completion.
    $ready.Reset() | Out-Null
    $release.Reset() | Out-Null
    $observation = [Diagnostics.Stopwatch]::StartNew()
    Invoke-Fixture $gatedSetup "/S /D=$gatedInstalled" 0
    Assert-True ($observation.Elapsed.TotalSeconds -ge 15) 'Slow fixture did not exercise continued observation.'
    Invoke-Fixture $driver "/S _?=$gatedInstalled" 0
} finally { $release.Set() | Out-Null; $ready.Dispose(); $release.Dispose() }
# A terminated operation leaves no stale kernel-object lease; retry normally.
Invoke-Fixture $setup "/S /D=$gatedInstalled" 0
Invoke-Fixture $driver "/S _?=$gatedInstalled" 0
Assert-True (-not (Test-Path -LiteralPath $gatedInstalled)) 'Retry after terminated Setup failed.'
Write-Output 'PASS: NSIS/PowerShell shared lease, competing installer/removal preservation, lease retained inside both NSIS sections, abrupt NSIS exit and retry. Only generated document fixtures were installed/removed.'
Assert-Tree $payloadBefore (Join-Path $trialRoot 'payload')
Assert-Tree $occupiedBefore $occupied
Assert-True ((Get-FileHash -LiteralPath $NsisArchive).Hash -eq $manifest.archive.sha256) 'NSIS archive changed.'
[pscustomobject]@{
    status='passed'; trial_root=$trialRoot; setup=$setup; setup_sha256=(Get-FileHash -LiteralPath $setup).Hash.ToLowerInvariant()
    scope='Fixture documents only. No app, runtime, registry, shortcut, upgrade, actual self-copy uninstall or supported-OS compatibility proof.'
} | ConvertTo-Json
