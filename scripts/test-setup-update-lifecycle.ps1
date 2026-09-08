[CmdletBinding()]
param([Parameter(Mandatory)][string]$NsisArchive)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$pins = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/nsis-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$NsisArchive = (Resolve-Path -LiteralPath $NsisArchive).Path
if ((Get-Item -LiteralPath $NsisArchive).Length -ne $pins.archive.bytes -or (Get-FileHash -LiteralPath $NsisArchive).Hash -ne $pins.archive.sha256) { throw 'NSIS archive identity mismatch.' }
$trialRoot = Join-Path $repositoryRoot ('target/tmp/setup-update-lifecycle-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $trialRoot | Out-Null
$registrySubKey = 'Software\towavue\InstallerTests\' + [guid]::NewGuid().ToString('N')
$handoffReadyName = 'Local\towavue-handoff-ready-' + (Split-Path -Leaf $registrySubKey)
$handoffReleaseName = 'Local\towavue-handoff-release-' + (Split-Path -Leaf $registrySubKey)
$programs = Join-Path $trialRoot 'programs'
New-Item -ItemType Directory -Path $programs | Out-Null
$shortcut = Join-Path $programs 'towavue (local evaluation).lnk'
$utf8 = [Text.UTF8Encoding]::new($false)
$temporary = Join-Path $trialRoot 'temporary source with spaces'
New-Item -ItemType Directory -Path $temporary | Out-Null
Add-Type @'
using System.Text;
using System.Runtime.InteropServices;
public static class SetupTemporaryAliasFixture {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode)]
    public static extern uint GetShortPathNameW(string path, StringBuilder output, uint size);
}
'@
$shortTemporary = [Text.StringBuilder]::new(32768)
$shortLength = [SetupTemporaryAliasFixture]::GetShortPathNameW($temporary,$shortTemporary,32768)
if ($shortLength -gt 0 -and $shortTemporary.ToString() -ine $temporary) {
    $temporary = $shortTemporary.ToString()
    Write-Output 'Using a real DOS-short temporary directory for every native Setup invocation.'
} else { Write-Output 'SKIP: no distinct DOS temporary directory alias; native short-temp coverage unavailable.' }
function Assert-True([bool]$Condition,[string]$Message) { if (-not $Condition) { throw $Message } }
function Write-Trial([string]$Path,[string]$Text) {
    Assert-True ([IO.Path]::GetFullPath($Path).StartsWith($trialRoot + '\',[StringComparison]::OrdinalIgnoreCase)) 'Fixture escaped scratch.'
    New-Item -ItemType Directory -Path (Split-Path -Parent $Path) -Force | Out-Null
    [IO.File]::WriteAllText($Path,$Text,$utf8)
}
function Invoke-Trial([string]$Executable,[string]$Arguments,[int]$Expected) {
    $savedTemp = $env:TEMP
    $savedTmp = $env:TMP
    try {
        $env:TEMP = $temporary
        $env:TMP = $temporary
        $process = Start-Process -FilePath $Executable -ArgumentList $Arguments -WorkingDirectory $trialRoot -WindowStyle Hidden -PassThru
    } finally { $env:TEMP = $savedTemp; $env:TMP = $savedTmp }
    [void]$process.Handle
    if (-not $process.WaitForExit(15000)) {
        Write-Output "Update fixture remains live; observing PID $($process.Id): $Arguments"
        if (-not $process.WaitForExit(45000)) { throw "Owned update fixture still running; do not restart: PID $($process.Id), $Executable $Arguments" }
    }
    if ($process.ExitCode -ne $Expected) {
        foreach ($log in Get-ChildItem -LiteralPath $trialRoot -Recurse -File -Filter 'update-transcript.txt') { Write-Output $log.FullName; Get-Content -LiteralPath $log.FullName -Encoding UTF8 }
        throw "Wrong native update exit $($process.ExitCode), expected $Expected : $Executable $Arguments"
    }
}
function Get-Tree([string]$Directory) {
    $result = [ordered]@{}
    foreach ($file in Get-ChildItem -LiteralPath $Directory -Recurse -File | Sort-Object FullName) { $result[$file.FullName.Substring($Directory.Length)] = (Get-FileHash -LiteralPath $file.FullName).Hash }
    return $result | ConvertTo-Json -Compress
}
function Get-Registration {
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
    $key = $null
    try {
        $key = $base.OpenSubKey($registrySubKey)
        if (-not $key) { return $null }
        $result = [ordered]@{}
        foreach ($name in $key.GetValueNames() | Sort-Object) { $result[$name] = @($key.GetValueKind($name).ToString(),$key.GetValue($name)) }
        return $result | ConvertTo-Json -Depth 5 -Compress
    } finally { if ($key) { $key.Dispose() }; $base.Dispose() }
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead($NsisArchive)
try {
    foreach ($entry in $archive.Entries) {
        if ($entry.FullName -notmatch '^nsis-3\.12/' -or $entry.FullName -match '(^/|(^|/)\.\.(/|$)|:|\\)' -or (($entry.ExternalAttributes -shr 16) -band 0xf000) -eq 0xa000) { throw 'Unsafe compiler archive entry.' }
    }
} finally { $archive.Dispose() }
$toolDirectory = Join-Path $trialRoot 'toolchain'
[IO.Compression.ZipFile]::ExtractToDirectory($NsisArchive,$toolDirectory)
$compiler = Join-Path $toolDirectory 'nsis-3.12/makensis.exe'
$version = & $compiler /VERSION
Assert-True ($LASTEXITCODE -eq 0 -and $version -eq 'v3.12') 'Wrong compiler version.'
$productionKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue-evaluation'
$nsisSource = [IO.File]::ReadAllText((Join-Path $repositoryRoot 'packaging/windows/setup.nsi'))
# Private fixtures use the actual application branch with generated payloads,
# silent/prerequisite substitutions and isolated registry/shortcut targets.
$silentPattern = '(?m)  \$\{If\} \$\{Silent\}\r?\n    SetErrorLevel 2\r?\n    Quit\r?\n  \$\{EndIf\}'
Assert-True ([regex]::Matches($nsisSource,$silentPattern).Count -eq 1) 'Silent fixture substitution missed its boundary.'
$nsisSource = [regex]::Replace($nsisSource,$silentPattern,'  ; Silent execution is permitted only in this generated fixture.')
$prerequisitePattern = '(?ms)^Function CheckPrerequisite\r?\n.*?^FunctionEnd'
Assert-True ([regex]::Matches($nsisSource,$prerequisitePattern).Count -eq 1) 'Prerequisite fixture substitution missed its boundary.'
$nsisSource = [regex]::Replace($nsisSource,$prerequisitePattern,"Function CheckPrerequisite`n  StrCpy `$PrerequisiteResult 0`nFunctionEnd")
# Observe the values consumed by the real Finish page without claiming visual QA.
$nsisSource += @'

Function .onInstSuccess
  FileOpen $1 "${TRIAL_ROOT}\completion.txt" w
  FileWriteUTF16LE $1 "$CompletionTitle$\r$\n$CompletionText"
  FileClose $1
FunctionEnd
'@
$updateSources = @('scripts/get-setup-update-plan.ps1','scripts/setup-update-paths.ps1','scripts/setup-update-native.cs','scripts/setup-update-registration.ps1','scripts/setup-update-transaction.ps1','scripts/setup-registered-update.ps1','packaging/windows/registration-state.ps1','packaging/windows/operation-lock.ps1','packaging/windows/update.ps1')
function Build-Trial([string]$Name,[switch]$New,[switch]$Fault,[switch]$Restart,[switch]$Handoff) {
    $root = Join-Path $trialRoot $Name
    $payload = Join-Path $root 'payload'
    Write-Trial (Join-Path $payload 'towavue.exe') $(if ($New) { 'New generated text, never executed. ' + ('x' * 8192) } else { 'Old generated text, never executed.' })
    Write-Trial (Join-Path $payload 'unchanged.dll') 'Unchanged generated text.'
    Write-Trial (Join-Path $payload $(if ($New) { 'new-only.dll' } else { 'old-only.dll' })) 'Version-specific generated text.'
    $records = @(Get-ChildItem -LiteralPath $payload -File | Sort-Object Name | ForEach-Object { [pscustomobject]@{name=$_.Name;bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant()} })
    $inventoryPath = Join-Path $payload 'licenses/INSTALLED-FILES.json'
    Write-Trial $inventoryPath ([ordered]@{schema_version=1;files=$records} | ConvertTo-Json -Depth 5)
    $ownership = 'towavue-local-' + (Get-FileHash -LiteralPath $inventoryPath).Hash.ToLowerInvariant()
    $files = @(Get-ChildItem -LiteralPath $payload -File -Recurse | Sort-Object FullName)
    $size = [int][Math]::Ceiling(($files | Measure-Object Length -Sum).Sum / 1024)
    $lines = @(('!define OWNERSHIP_ID "' + $ownership + '"'),'!define PAYLOAD_MAX_PATH 40',('!define PAYLOAD_SIZE_KIB ' + $size),'!macro InstallApplicationFiles')
    foreach ($file in $files) {
        $relative = $file.FullName.Substring($payload.Length + 1)
        $parent = if ($relative.Contains('\')) { '\licenses' } else { '' }
        $lines += '  SetOutPath "$INSTDIR' + $parent + '"'
        $lines += '  File "${TRIAL_ROOT}\payload\' + $relative + '"'
    }
    $lines += @('!macroend','!macro RemoveApplicationFiles')
    foreach ($file in $files) { $lines += '  Delete "$INSTDIR\' + $file.FullName.Substring($payload.Length + 1) + '"' }
    $lines += @('!macroend','!macro CheckApplicationDirectories','  StrCpy $INSTDIR "$3\licenses"','  Call un.CheckPath','!macroend','!macro RemoveApplicationDirectories','  RMDir "$INSTDIR\licenses"','!macroend')
    Write-Trial (Join-Path $root 'payload.nsh') ($lines -join "`n")
    foreach ($name in $updateSources + @('packaging/windows/registration.ps1','packaging/windows/UnicodeShellLink.cs')) {
        $source = [IO.File]::ReadAllText((Join-Path $repositoryRoot $name)).Replace($productionKey,$registrySubKey)
        $source = $source.Replace('[Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)',("'" + $programs.Replace("'","''") + "'"))
        if ($name -eq 'packaging/windows/update.ps1') {
            $anchor = '$ErrorActionPreference = ''Stop'''
            Assert-True ($source.Contains($anchor)) 'Update transcript missed the CLI boundary.'
            $source = $source.Replace($anchor,($anchor + "`nStart-Transcript -LiteralPath '" + (Join-Path $root 'update-transcript.txt') + "' -Append | Out-Null"))
        }
        if ($Fault -and $name -eq 'packaging/windows/registration-state.ps1') {
            $write = "`$key.SetValue('EstimatedSize',`$SizeKiB,[Microsoft.Win32.RegistryValueKind]::DWord)"
            Assert-True ($source.Contains($write)) 'Native update fault missed its registry boundary.'
            $source = $source.Replace($write,($write + "; throw 'Injected native update interruption'"))
        }
        Write-Trial (Join-Path $root "update/$name") $source
        if ($name -in @('packaging/windows/registration.ps1','packaging/windows/registration-state.ps1','packaging/windows/UnicodeShellLink.cs')) { Write-Trial (Join-Path $root ('registration/' + (Split-Path -Leaf $name))) $source }
        if ($name -eq 'packaging/windows/operation-lock.ps1') { Write-Trial (Join-Path $root 'operation-lock.ps1') $source }
    }
    $source = if ($Restart) { $nsisSource.Replace('StrCpy $PrerequisiteResult 0','StrCpy $PrerequisiteResult 3010') } else { $nsisSource }
    if ($Handoff) {
        $anchor = '(?m)^  Call ReleaseOperation\r?\n  DetailPrint "Verifying files and performing'
        Assert-True ([regex]::Matches($source,$anchor).Count -eq 1) 'Handoff gate missed its pre-child boundary.'
        $gate = @'
  Call ReleaseOperation
  System::Call 'kernel32::OpenEventW(i 2, i 0, w "READY_NAME") p .r0'
  System::Call 'kernel32::SetEvent(p r0)'
  System::Call 'kernel32::CloseHandle(p r0)'
  System::Call 'kernel32::OpenEventW(i 0x100000, i 0, w "RELEASE_NAME") p .r0'
  System::Call 'kernel32::WaitForSingleObject(p r0, i 45000) i .r2'
  System::Call 'kernel32::CloseHandle(p r0)'
  ${If} $2 != 0
    SetErrorLevel 5
    Abort "Handoff fixture was not released. No child was started."
  ${EndIf}
  DetailPrint "Verifying files and performing
'@
        $gate = $gate.Replace('READY_NAME',$handoffReadyName).Replace('RELEASE_NAME',$handoffReleaseName)
        $source = [regex]::Replace($source,$anchor,[Text.RegularExpressions.MatchEvaluator]{ param($match) $gate })
    }
    Write-Trial (Join-Path $root 'setup.nsi') $source
    & $compiler /NOCONFIG /WX /V2 /DTOWAVUE_SETUP_APPLICATION ("/DTRIAL_ROOT=" + $root.Replace('$','$$')) (Join-Path $root 'setup.nsi')
    Assert-True ($LASTEXITCODE -eq 0) 'Native application-shaped fixture compilation failed.'
    return @{root=$root;setup=(Join-Path $root 'Setup-local.exe');payload=$payload;ownership=$ownership;size=$size}
}
function Assert-Updated($Build,[string]$InstallDirectory) {
    foreach ($file in Get-ChildItem -LiteralPath $Build.payload -File -Recurse) {
        $target = Join-Path $InstallDirectory $file.FullName.Substring($Build.payload.Length + 1)
        Assert-True ((Get-FileHash -LiteralPath $target).Hash -eq (Get-FileHash -LiteralPath $file.FullName).Hash) 'Updated native payload differs.'
    }
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $InstallDirectory 'old-only.dll'))) 'Removed payload file remains.'
    $state = Get-Registration | ConvertFrom-Json
    Assert-True ($state.TowavueOwnershipId[1] -ceq $Build.ownership -and $state.EstimatedSize[1] -eq $Build.size -and -not $state.PSObject.Properties['TowavuePendingUpdate']) 'Updated native registration differs or remains pending.'
    Assert-True ([IO.File]::ReadAllText((Join-Path $InstallDirectory 'user.txt')) -ceq 'Preserve user data.') 'Native update changed unrelated user data.'
}
function Assert-Completion($Build,[string]$Title,[string]$Text) {
    $completion = Get-Content -LiteralPath (Join-Path $Build.root 'completion.txt') -Raw -Encoding Unicode
    Assert-True ($completion.StartsWith($Title + "`r`n") -and $completion.Contains($Text)) 'Setup completion message misrepresents its outcome.'
}
function Assert-HandoffRefusal($Build,[string]$InstallDirectory) {
    . (Join-Path $repositoryRoot 'packaging/windows/operation-lock.ps1') -RegistrySubKey $registrySubKey
    $before = Get-Tree $InstallDirectory
    $registrationBefore = Get-Registration
    $ready = [Threading.EventWaitHandle]::new($false,[Threading.EventResetMode]::ManualReset,$handoffReadyName)
    $release = [Threading.EventWaitHandle]::new($false,[Threading.EventResetMode]::ManualReset,$handoffReleaseName)
    $competitor = $null
    try {
        $process = Start-Process -FilePath $Build.setup -ArgumentList "/S /D=$InstallDirectory" -WorkingDirectory $trialRoot -WindowStyle Hidden -PassThru
        [void]$process.Handle
        while (-not $ready.WaitOne(15000)) {
            if ($process.HasExited) { throw "Handoff fixture exited before the gate: $($process.ExitCode)" }
            Write-Output "Observing the same handoff fixture PID $($process.Id) before the child gate."
        }
        # Successfully acquiring here proves the parent released before mutation.
        $competitor = New-TowavueOperationLease $registrySubKey
        Assert-True ((Get-Tree $InstallDirectory) -ceq $before -and (Get-Registration) -ceq $registrationBefore) 'Parent mutated installed state before child acquisition.'
        [void]$release.Set()
        while (-not $process.WaitForExit(15000)) {
            Write-Output "Observing the same handoff fixture PID $($process.Id) while the competitor retains its lease."
        }
        Assert-True ($process.ExitCode -eq 5) 'Update child bypassed the competing operation lease.'
        Assert-True ((Get-Tree $InstallDirectory) -ceq $before -and (Get-Registration) -ceq $registrationBefore) 'Competing update/recovery changed files or pending registration.'
    } finally {
        if ($competitor) { $competitor.Dispose() }
        $ready.Dispose(); $release.Dispose()
    }
    Write-Output 'PASS: actual NSIS parent/child handoff refuses a competitor winning the released-lease gap, preserving files and the complete registration.'
}
$old = Build-Trial 'old'
$new = Build-Trial 'new' -New
$broken = Build-Trial 'broken' -New -Fault
$restart = Build-Trial 'restart' -New -Restart
$gated = Build-Trial 'handoff' -New -Handoff
$japanese = [string][char]0x65e5 + [char]0x672c
foreach ($recovery in @($false,$true)) {
    $installed = Join-Path $trialRoot ("installed $recovery & `$ $japanese")
    Invoke-Trial $old.setup "/S /D=$installed" 0
    Assert-Completion $old 'Installation completed' 'No application was launched.'
    Write-Trial (Join-Path $installed 'user.txt') 'Preserve user data.'
    $before = Get-Tree $installed
    $beforeRegistration = Get-Registration
    $shortcutHash = (Get-FileHash -LiteralPath $shortcut).Hash
    $oldDriver = Join-Path $trialRoot "old-driver-$recovery.exe"
    Copy-Item -LiteralPath (Join-Path $installed 'Uninstall.exe') -Destination $oldDriver
    if ($recovery) {
        Invoke-Trial $broken.setup "/S /D=$installed" 5
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $installed 'towavue.exe')) -and -not (Test-Path -LiteralPath (Join-Path $installed 'Uninstall.exe'))) 'Interrupted native update published executables.'
        $pending = (Get-Registration | ConvertFrom-Json).TowavuePendingUpdate[1] | ConvertFrom-Json
        $journal = Get-Content -LiteralPath (Join-Path $pending.TransactionDirectory 'journal.json') -Raw -Encoding UTF8 | ConvertFrom-Json
        $uninstallerIndex = $journal.entries.Count - 2
        $pendingDriver = Join-Path $trialRoot 'pending-driver.exe'
        Copy-Item -LiteralPath (Join-Path $pending.TransactionDirectory "$uninstallerIndex.new") -Destination $pendingDriver
        $partial = Get-Tree $installed
        $partialRegistration = Get-Registration
        Invoke-Trial $pendingDriver "/S _?=$installed" 3
        Assert-True ((Get-Tree $installed) -ceq $partial -and (Get-Registration) -ceq $partialRegistration) 'Pending native uninstaller changed files or registration.'
        Assert-HandoffRefusal $gated $installed
        Invoke-Trial $new.setup "/S /D=$installed" 0
        Assert-True ((Get-Tree $installed) -ceq $before -and (Get-Registration) -ceq $beforeRegistration) 'Native Setup recovery did not restore exact old states.'
        Assert-Completion $new 'Previous version restored' 'The new version has not been installed.'
    } else {
        Assert-HandoffRefusal $gated $installed
        Write-Trial (Join-Path $installed 'unchanged.dll') 'Preserve this user replacement.'
        $modified = Get-Tree $installed
        Invoke-Trial $new.setup "/S /D=$installed" 5
        Assert-True ((Get-Tree $installed) -ceq $modified -and (Get-Registration) -ceq $beforeRegistration) 'Rejected native update changed user files or registration.'
        Write-Trial (Join-Path $installed 'unchanged.dll') 'Unchanged generated text.'
    }
    $update = if ($recovery) { $new } else { $restart }
    Invoke-Trial $update.setup "/S /D=$installed" $(if ($recovery) { 0 } else { 3010 })
    Assert-Updated $update $installed
    Assert-Completion $update 'Update completed' 'Recovery files are retained beside the installation folder.'
    if (-not $recovery) { Assert-Completion $update 'Update completed' 'Restart manually when convenient; Setup will not restart this PC.' }
    Assert-True ((Get-FileHash -LiteralPath $shortcut).Hash -eq $shortcutHash) 'Native update changed the shortcut.'
    $updated = Get-Tree $installed
    Invoke-Trial $oldDriver "/S _?=$installed" 2
    Assert-True ((Get-Tree $installed) -ceq $updated) 'Old native uninstaller removed updated files.'
    $driver = Join-Path $trialRoot "new-driver-$recovery.exe"
    Copy-Item -LiteralPath (Join-Path $installed 'Uninstall.exe') -Destination $driver
    Invoke-Trial $driver "/S _?=$installed" 0
    Assert-True ($null -eq (Get-Registration) -and -not (Test-Path -LiteralPath $shortcut)) 'Native fixture registration or shortcut remains.'
    Assert-True (@(Get-ChildItem -LiteralPath $installed -Recurse -File).Count -eq 1 -and [IO.File]::ReadAllText((Join-Path $installed 'user.txt')) -ceq 'Preserve user data.') 'Native uninstall removed user data or retained owned files.'
}
$transcript = Get-Content -LiteralPath (Join-Path $new.root 'update-transcript.txt') -Raw -Encoding UTF8
foreach ($phase in @('Verifying installed and incoming file inventories.','Preparing independent recovery copies.','Recovery record saved.','Validating the retained Apply journal','Validating the retained Rollback journal','Verifying recovery files and acquiring installed-file handles.','Files and registration agree. Pending recovery record cleared.')) {
    Assert-True ($transcript.Contains($phase)) ('Missing update phase output: ' + $phase)
}
Write-Output 'PASS: distinct installation/update/restoration Finish-page values, synthetic prerequisite 3010 result/manual-restart guidance and child phase output; no percentage or remaining-time estimate. Rendered progress/Finish accessibility is not qualified by these silent fixtures.'
Write-Output "PASS: native application-shaped Setup install/update, modified-file refusal, parent/child lease reacquisition, interrupted registration, pending uninstall refusal, exact rollback/retry and updated uninstall. Test key/shortcut removed: $trialRoot"
Write-Output 'SKIP: real app/VC installation, original prerequisite consent, normal self-copy uninstall, interactive visual/accessibility QA and supported-Windows qualification. Private fixtures bypass silent/prerequisite restrictions only in generated source.'
