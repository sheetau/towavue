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
$programs = Join-Path $trialRoot 'programs'
New-Item -ItemType Directory -Path $programs | Out-Null
$shortcut = Join-Path $programs 'towavue (local evaluation).lnk'
$utf8 = [Text.UTF8Encoding]::new($false)
function Assert-True([bool]$Condition,[string]$Message) { if (-not $Condition) { throw $Message } }
function Write-Trial([string]$Path,[string]$Text) {
    Assert-True ([IO.Path]::GetFullPath($Path).StartsWith($trialRoot + '\',[StringComparison]::OrdinalIgnoreCase)) 'Fixture escaped scratch.'
    New-Item -ItemType Directory -Path (Split-Path -Parent $Path) -Force | Out-Null
    [IO.File]::WriteAllText($Path,$Text,$utf8)
}
function Invoke-Trial([string]$Executable,[string]$Arguments,[int]$Expected) {
    $process = Start-Process -FilePath $Executable -ArgumentList $Arguments -WorkingDirectory $trialRoot -WindowStyle Hidden -PassThru
    [void]$process.Handle
    if (-not $process.WaitForExit(15000)) {
        Write-Output "Update fixture remains live; observing PID $($process.Id): $Arguments"
        if (-not $process.WaitForExit(45000)) { throw "Owned update fixture still running; do not restart: PID $($process.Id), $Executable $Arguments" }
    }
    if ($process.ExitCode -ne $Expected) {
        foreach ($log in Get-ChildItem -LiteralPath $trialRoot -Recurse -File -Filter 'update-calls.txt') { Write-Output $log.FullName; Get-Content -LiteralPath $log.FullName -Encoding Unicode }
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
$updateResultPattern = '(?m)  Pop \$UpdateResult\r?\n  Pop \$0'
Assert-True ([regex]::Matches($nsisSource,$updateResultPattern).Count -eq 1) 'Native update diagnostics missed the result boundary.'
$diagnostic = @'
  Pop $UpdateResult
  Pop $0
  FileOpen $1 "${TRIAL_ROOT}\update-calls.txt" a
  FileSeek $1 0 END
  FileWriteUTF16LE $1 "$UpdateMode ($UpdateResult): $0$\r$\n"
  FileClose $1
'@
$nsisSource = [regex]::Replace($nsisSource,$updateResultPattern,[Text.RegularExpressions.MatchEvaluator]{ param($match) $diagnostic })
$updateSources = @('scripts/get-setup-update-plan.ps1','scripts/setup-update-paths.ps1','scripts/setup-update-native.cs','scripts/setup-update-registration.ps1','scripts/setup-update-transaction.ps1','scripts/setup-registered-update.ps1','packaging/windows/registration-state.ps1','packaging/windows/operation-lock.ps1','packaging/windows/update.ps1')
function Build-Trial([string]$Name,[switch]$New,[switch]$Fault) {
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
        if ($Fault -and $name -eq 'packaging/windows/registration-state.ps1') {
            $write = "`$key.SetValue('EstimatedSize',`$SizeKiB,[Microsoft.Win32.RegistryValueKind]::DWord)"
            Assert-True ($source.Contains($write)) 'Native update fault missed its registry boundary.'
            $source = $source.Replace($write,($write + "; throw 'Injected native update interruption'"))
        }
        Write-Trial (Join-Path $root "update/$name") $source
        if ($name -in @('packaging/windows/registration.ps1','packaging/windows/registration-state.ps1','packaging/windows/UnicodeShellLink.cs')) { Write-Trial (Join-Path $root ('registration/' + (Split-Path -Leaf $name))) $source }
        if ($name -eq 'packaging/windows/operation-lock.ps1') { Write-Trial (Join-Path $root 'operation-lock.ps1') $source }
    }
    Write-Trial (Join-Path $root 'setup.nsi') $nsisSource
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
$old = Build-Trial 'old'
$new = Build-Trial 'new' -New
$broken = Build-Trial 'broken' -New -Fault
$japanese = [string][char]0x65e5 + [char]0x672c
foreach ($recovery in @($false,$true)) {
    $installed = Join-Path $trialRoot ("installed $recovery & `$ $japanese")
    Invoke-Trial $old.setup "/S /D=$installed" 0
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
        Invoke-Trial $new.setup "/S /D=$installed" 0
        Assert-True ((Get-Tree $installed) -ceq $before -and (Get-Registration) -ceq $beforeRegistration) 'Native Setup recovery did not restore exact old states.'
    } else {
        Write-Trial (Join-Path $installed 'unchanged.dll') 'Preserve this user replacement.'
        $modified = Get-Tree $installed
        Invoke-Trial $new.setup "/S /D=$installed" 5
        Assert-True ((Get-Tree $installed) -ceq $modified -and (Get-Registration) -ceq $beforeRegistration) 'Rejected native update changed user files or registration.'
        Write-Trial (Join-Path $installed 'unchanged.dll') 'Unchanged generated text.'
    }
    Invoke-Trial $new.setup "/S /D=$installed" 0
    Assert-Updated $new $installed
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
Write-Output "PASS: native application-shaped Setup install/update, modified-file refusal, parent/child lease reacquisition, interrupted registration, pending uninstall refusal, exact rollback/retry and updated uninstall. Test key/shortcut removed: $trialRoot"
Write-Output 'SKIP: real app/VC installation, original prerequisite consent, normal self-copy uninstall, interactive visual/accessibility QA and supported-Windows qualification. Private fixtures bypass silent/prerequisite restrictions only in generated source.'
