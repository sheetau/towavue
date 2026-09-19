[CmdletBinding()]
param([string]$NsisArchive)
$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'setup-update-transaction.ps1')
. (Join-Path $repository 'packaging/windows/registration-state.ps1')
$root = Join-Path $repository ('target/tmp/production-update-' + [Guid]::NewGuid().ToString('N'))
$installed = Join-Path $root 'installed'
$incoming = Join-Path $root 'incoming'
$programs = Join-Path $root 'programs'
$keyName = 'Software\towavue\InstallerTests\' + [Guid]::NewGuid().ToString('N')
$shortcut = Join-Path $programs 'towavue.lnk'
$utf8 = [Text.UTF8Encoding]::new($false)
[IO.Directory]::CreateDirectory($programs) | Out-Null
function Assert-True([bool]$Condition,[string]$Message) { if (-not $Condition) { throw $Message } }
function Write-Text([string]$Path,[string]$Text) { [IO.File]::WriteAllText($Path,$Text,$utf8) }
function Refused([scriptblock]$Action,[string]$Message) {
    $failed=$false
    try { & $Action | Out-Null } catch { if (-not $_.Exception.Message.Contains($Message)) { throw }; $failed=$true }
    Assert-True $failed ('Expected refusal: ' + $Message)
}
function Inventory([string]$Directory,[string]$Version) {
    $files = @(Get-ChildItem -LiteralPath $Directory -File | Where-Object Name -in @('towavue.exe','support.txt') | ForEach-Object { [ordered]@{name=$_.Name;bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant()} })
    $path=Join-Path $Directory 'licenses/INSTALLED-FILES.json'
    Write-Text $path ([ordered]@{schema_version=2;channel='stable';platform='windows-x64';product_version=$Version;files=$files} | ConvertTo-Json -Depth 5)
    'towavue-release-' + (Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant()
}
function Snapshot {
    $files=[ordered]@{}
    foreach ($file in Get-ChildItem -LiteralPath $installed -Recurse -File | Sort-Object FullName) { $files[$file.FullName]=(Get-FileHash -LiteralPath $file.FullName).Hash }
    $key=$base.OpenSubKey($keyName)
    try {
        $values=[ordered]@{}
        foreach ($name in $key.GetValueNames() | Sort-Object) { $values[$name]=@($key.GetValueKind($name).ToString(),$key.GetValue($name)) }
    } finally { $key.Dispose() }
    [ordered]@{files=$files;values=$values;shortcut=(Get-FileHash -LiteralPath $shortcut).Hash} | ConvertTo-Json -Depth 6 -Compress
}
foreach ($directory in @($installed,$incoming)) {
    [IO.Directory]::CreateDirectory((Join-Path $directory 'licenses')) | Out-Null
    $version=if ($directory -eq $installed) { '1.0.0' } else { '1.0.1' }
    $source='using System.Reflection; [assembly: AssemblyFileVersion("' + $version + '")] [assembly: AssemblyInformationalVersion("' + $version + '")] public static class Fixture' + [Guid]::NewGuid().ToString('N') + ' { public static void Main() {} }'
    Add-Type -TypeDefinition $source -OutputAssembly (Join-Path $directory 'towavue.exe') -OutputType WindowsApplication
    Write-Text (Join-Path $directory 'support.txt') ('support ' + $version)
    $identity=Inventory $directory $version
    if ($directory -eq $installed) {
        $oldId=$identity
        [IO.File]::WriteAllText((Join-Path $installed 'towavue-install.ini'),"[installation]`r`nid=$identity`r`ndirectory=$installed`r`n",[Text.Encoding]::Unicode)
        Write-Text (Join-Path $installed 'Uninstall.exe') 'old uninstaller fixture; never executed'
    } else { $newId=$identity }
}
Write-Text (Join-Path $installed 'user-media.txt') 'preserve this file'
$uninstaller=Join-Path $root 'new-uninstaller.exe'
Write-Text $uninstaller 'new uninstaller fixture; never executed'
$registration=@{RegistrySubKey=$keyName;ShortcutPath=$shortcut}
$arguments=@{InstallDirectory=$installed;OwnershipId=$oldId;RegistrySubKey=$keyName;ShortcutPath=$shortcut;SizeKiB=10;ProductVersion='1.0.0'}
$pair=@{InstallDirectory=$installed;IncomingPayloadDirectory=$incoming;IncomingOwnershipId=$newId;NewUninstaller=$uninstaller;Registration=$registration}
$base=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
try {
    Invoke-TowavueRegistration @arguments -Mode Install | Out-Null
    $key=$base.OpenSubKey($keyName,$true)
    try { $key.SetValue('UserValue','preserve'); Assert-True ($key.GetValue('DisplayName') -eq 'towavue') 'Wrong product name.' } finally { $key.Dispose() }
    $before=Snapshot
    $token=New-TowavueUpdateTransaction @pair
    $journalPath=Join-Path $token.TransactionDirectory 'journal.json'
    $journal=Get-Content -LiteralPath $journalPath -Encoding UTF8 -Raw | ConvertFrom-Json
    Assert-True ($journal.schema_version -eq 4 -and $journal.plan.schema_version -eq 2 -and $journal.registration.PreviousProductVersion -ceq '1.0.0' -and $journal.registration.ProductVersion -ceq '1.0.1') 'Production version binding is incomplete.'
    $invoke=@{TransactionDirectory=$token.TransactionDirectory;JournalSha256=$token.JournalSha256}
    Invoke-TowavueUpdateTransaction @invoke -Mode Apply | Out-Null
    $key=$base.OpenSubKey($keyName)
    try { Assert-True ($key.GetValue('DisplayVersion') -ceq '1.0.1' -and (Get-Item -LiteralPath (Join-Path $installed 'towavue.exe')).VersionInfo.ProductVersion -ceq '1.0.1') 'EXE and registration versions did not advance together.' } finally { $key.Dispose() }
    Invoke-TowavueUpdateTransaction @invoke -Mode Rollback | Out-Null
    Assert-True ((Snapshot) -ceq $before) 'Production rollback did not restore exact files and typed registration.'

    # Each independently written field can be interrupted in either direction.
    $statePath=Join-Path $repository 'packaging/windows/registration-state.ps1'
    $stateSource=[IO.File]::ReadAllText($statePath,[Text.Encoding]::UTF8)
    $injectedPath=Join-Path $root 'registration-interrupted.ps1'
    $update=@{InstallDirectory=$installed;OwnershipId=$newId;RegistrySubKey=$keyName;ShortcutPath=$shortcut;SizeKiB=$journal.registration.SizeKiB;ProductVersion='1.0.1';PreviousOwnershipId=$oldId;PreviousSizeKiB=10;PreviousProductVersion='1.0.0'}
    $rollback=@{InstallDirectory=$installed;OwnershipId=$oldId;RegistrySubKey=$keyName;ShortcutPath=$shortcut;SizeKiB=10;ProductVersion='1.0.0';PreviousOwnershipId=$newId;PreviousSizeKiB=$journal.registration.SizeKiB;PreviousProductVersion='1.0.1'}
    foreach ($direction in @('Apply','Rollback')) {
        foreach ($statement in @("`$key.SetValue('EstimatedSize',`$SizeKiB,[Microsoft.Win32.RegistryValueKind]::DWord)","`$key.SetValue('DisplayVersion',`$ProductVersion,[Microsoft.Win32.RegistryValueKind]::String)","`$key.SetValue('TowavueOwnershipId',`$OwnershipId,[Microsoft.Win32.RegistryValueKind]::String)")) {
            if ($direction -eq 'Rollback') { Invoke-TowavueRegistration @update -Mode Update | Out-Null }
            Assert-True ($stateSource.Contains($statement)) 'Missing interruption boundary.'
            Write-Text $injectedPath ($stateSource.Replace($statement,($statement + "; throw 'Injected version interruption'")))
            . $injectedPath
            Refused { if ($direction -eq 'Apply') { Invoke-TowavueRegistration @update -Mode Update } else { Invoke-TowavueRegistration @rollback -Mode Update } } 'Injected version interruption'
            . $statePath
            Invoke-TowavueRegistration @rollback -Mode Update | Out-Null
            Assert-True ((Snapshot) -ceq $before) 'A partial three-field transition did not recover exactly.'
        }
    }
    foreach ($value in @('9.0.0',7)) {
        $key=$base.OpenSubKey($keyName,$true)
        try { if ($value -is [int]) { $key.SetValue('DisplayVersion',$value,[Microsoft.Win32.RegistryValueKind]::DWord) } else { $key.SetValue('DisplayVersion',$value) } } finally { $key.Dispose() }
        $unknown=Snapshot
        Refused { Invoke-TowavueRegistration @update -Mode Update } 'unknown product version'
        Assert-True ((Snapshot) -ceq $unknown) 'Rejected version changed registration.'
        $key=$base.OpenSubKey($keyName,$true)
        try { $key.SetValue('DisplayVersion','1.0.0',[Microsoft.Win32.RegistryValueKind]::String) } finally { $key.Dispose() }
    }
    $wrong=$arguments.Clone(); $wrong.RegistrySubKey='Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue-evaluation'
    Refused { Invoke-TowavueRegistration @wrong -Mode Inspect } 'cannot take over'
    $wrong.RegistrySubKey='Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue'; $wrong.OwnershipId='towavue-local-' + ('a'*64)
    Refused { Invoke-TowavueRegistration @wrong -Mode Inspect } 'cannot take over'
    foreach ($version in @('01.0.0','1.0.0-beta','65536.0.0')) { Refused { Assert-TowavueProductVersion $version } $(if ($version -eq '65536.0.0') { 'resource limits' } else { 'canonical stable' }) }

    $originalJournal=[IO.File]::ReadAllText($journalPath,[Text.Encoding]::UTF8)
    foreach ($kind in @('version','schema')) {
        $changed=$originalJournal | ConvertFrom-Json
        if ($kind -eq 'version') { $changed.registration.ProductVersion='1.0.2' } else { $changed.schema_version=3 }
        Write-Text $journalPath ($changed | ConvertTo-Json -Depth 12)
        Refused { Invoke-TowavueUpdateTransaction -Mode Rollback -TransactionDirectory $token.TransactionDirectory -JournalSha256 (Get-FileHash -LiteralPath $journalPath).Hash.ToLowerInvariant() } $(if ($kind -eq 'version') { 'versions do not match' } else { 'schema cannot describe' })
        Assert-True ((Snapshot) -ceq $before) 'Rejected journal changed files or registration.'
    }
    Write-Text $journalPath $originalJournal
    $badId=Inventory $incoming '1.0.2'
    Refused { & (Join-Path $PSScriptRoot 'get-setup-update-plan.ps1') -InstallDirectory $installed -IncomingPayloadDirectory $incoming -IncomingOwnershipId $badId } 'version differs'
    $newId=Inventory $incoming '1.0.1'
    Assert-True ((Snapshot) -ceq $before) 'Validation altered the installed product.'
    if ($NsisArchive) {
        $pins=Get-Content -LiteralPath (Join-Path $repository 'docs/nsis-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
        $NsisArchive=(Resolve-Path -LiteralPath $NsisArchive).Path
        Assert-True ((Get-Item -LiteralPath $NsisArchive).Length -eq $pins.archive.bytes -and (Get-FileHash -LiteralPath $NsisArchive).Hash -eq $pins.archive.sha256) 'NSIS input differs.'
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $tools=Join-Path $root 'tools'
        [IO.Compression.ZipFile]::ExtractToDirectory($NsisArchive,$tools)
        $compiler=Join-Path $tools 'nsis-3.12/makensis.exe'
        $build=Join-Path $root 'setup'
        [IO.Directory]::CreateDirectory($build) | Out-Null
        Copy-Item -LiteralPath $incoming -Destination (Join-Path $build 'payload') -Recurse
        $files=@(Get-ChildItem -LiteralPath $incoming -File -Recurse | Sort-Object FullName)
        $size=[int][Math]::Ceiling(($files | Measure-Object Length -Sum).Sum/1024)
        $lines=@(('!define OWNERSHIP_ID "' + $newId + '"'),'!define PAYLOAD_MAX_PATH 40',('!define PAYLOAD_SIZE_KIB ' + $size),'!macro InstallApplicationFiles')
        foreach ($file in $files) {
            $relative=$file.FullName.Substring($incoming.Length+1)
            $parent=if ($relative.Contains('\')) { '\licenses' } else { '' }
            $lines+='  SetOutPath "$INSTDIR' + $parent + '"'
            $lines+='  File "${TRIAL_ROOT}\payload\' + $relative + '"'
        }
        $lines+=@('!macroend','!macro RemoveApplicationFiles')
        foreach ($file in $files) { $lines+='  Delete "$INSTDIR\' + $file.FullName.Substring($incoming.Length+1) + '"' }
        $lines+=@('!macroend','!macro CheckApplicationDirectories','  StrCpy $INSTDIR "$3\licenses"','  Call un.CheckPath','!macroend','!macro RemoveApplicationDirectories','  RMDir "$INSTDIR\licenses"','!macroend')
        Write-Text (Join-Path $build 'payload.nsh') ($lines -join "`n")
        $productKey='Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue'
        $sources=@('scripts/get-setup-update-plan.ps1','scripts/setup-update-paths.ps1','scripts/setup-update-native.cs','scripts/setup-update-registration.ps1','scripts/setup-update-transaction.ps1','scripts/setup-registered-update.ps1','packaging/windows/registration-state.ps1','packaging/windows/operation-lock.ps1','packaging/windows/update.ps1','packaging/windows/registration.ps1','packaging/windows/UnicodeShellLink.cs')
        foreach ($name in $sources) {
            $text=[IO.File]::ReadAllText((Join-Path $repository $name),[Text.Encoding]::UTF8).Replace($productKey,$keyName)
            $text=$text.Replace('[Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)',("'" + $programs.Replace("'","''") + "'"))
            $destination=Join-Path $build ('update/' + $name)
            [IO.Directory]::CreateDirectory((Split-Path -Parent $destination)) | Out-Null
            Write-Text $destination $text
            if ($name -in @('packaging/windows/registration.ps1','packaging/windows/registration-state.ps1','packaging/windows/UnicodeShellLink.cs')) {
                [IO.Directory]::CreateDirectory((Join-Path $build 'registration')) | Out-Null
                Write-Text (Join-Path $build ('registration/' + (Split-Path -Leaf $name))) $text
            }
            if ($name -eq 'packaging/windows/operation-lock.ps1') { Write-Text (Join-Path $build 'operation-lock.ps1') $text }
        }
        $nsis=[IO.File]::ReadAllText((Join-Path $repository 'packaging/windows/setup.nsi'),[Text.Encoding]::UTF8).Replace($productKey,$keyName)
        $pattern='(?ms)^Function CheckPrerequisite\r?\n.*?^FunctionEnd'
        Assert-True ([regex]::Matches($nsis,$pattern).Count -eq 1) 'Prerequisite fixture substitution missed its boundary.'
        # Only this fixture supplies a satisfied prerequisite; product silent and
        # identity/version gates are compiled unchanged, with a private HKCU key.
        $nsis=[regex]::Replace($nsis,$pattern,"Function CheckPrerequisite`n  StrCpy `$PrerequisiteResult 0`nFunctionEnd")
        Write-Text (Join-Path $build 'setup.nsi') $nsis
        & $compiler /NOCONFIG /WX /V2 /DTOWAVUE_SETUP_APPLICATION /DTOWAVUE_SETUP_RELEASE /DPRODUCT_VERSION=1.0.1 ("/DTRIAL_ROOT=" + $build.Replace('$','$$')) (Join-Path $build 'setup.nsi')
        Assert-True ($LASTEXITCODE -eq 0) 'Production NSIS compilation failed.'
        $setup=Join-Path $build 'towavue-1.0.1-windows-x64-setup.exe'
        Assert-True ((Get-Item -LiteralPath $setup).VersionInfo.ProductVersion -ceq '1.0.1') 'Setup version resource differs.'
        foreach ($osCase in @('windows10','server','arm64')) {
            $negative=Join-Path $build ('refuse-' + $osCase + '.exe')
            $changed=$nsis.Replace('OutFile "${TRIAL_ROOT}\towavue-${PRODUCT_VERSION}-windows-x64-setup.exe"',('OutFile "${TRIAL_ROOT}\refuse-' + $osCase + '.exe"'))
            $changed=switch ($osCase) {
                'windows10' { $changed.Replace('GetWinVer $0 Build','StrCpy $0 19045') }
                'server' { $changed.Replace('${OrIf} ${IsServerOS}','${OrIf} 1 == 1') }
                'arm64' { $changed.Replace('${IfNot} ${IsNativeAMD64}','${If} 1 == 1') }
            }
            Write-Text (Join-Path $build ('refuse-' + $osCase + '.nsi')) $changed
            & $compiler /NOCONFIG /WX /V2 /DTOWAVUE_SETUP_APPLICATION /DTOWAVUE_SETUP_RELEASE /DPRODUCT_VERSION=1.0.1 ("/DTRIAL_ROOT=" + $build.Replace('$','$$')) (Join-Path $build ('refuse-' + $osCase + '.nsi'))
            Assert-True ($LASTEXITCODE -eq 0) 'OS refusal fixture compilation failed.'
            $snapshot=Snapshot
            $process=Start-Process -FilePath $negative -ArgumentList ('/S /TOWAVUEUPDATE=1 /D=' + $installed) -WindowStyle Hidden -PassThru
            $null=$process.Handle
            try { Assert-True ($process.WaitForExit(30000) -and $process.ExitCode -eq 2) ('Unsupported OS fixture was accepted: ' + $osCase) } finally { $process.Dispose() }
            Assert-True ((Snapshot) -ceq $snapshot) 'Unsupported OS changed the installation.'
        }
        foreach ($case in @('no-flag','fresh','update','repeat')) {
            $destination=if ($case -eq 'fresh') { Join-Path $root 'empty-destination' } else { $installed }
            $command=if ($case -eq 'no-flag') { '/S /D=' + $destination } else { '/S /TOWAVUEUPDATE=1 /D=' + $destination }
            $snapshot=Snapshot
            $process=Start-Process -FilePath $setup -ArgumentList $command -WindowStyle Hidden -PassThru
            $null=$process.Handle
            try {
                if (-not $process.WaitForExit(60000)) { throw "Native production fixture remains running: PID $($process.Id). Do not restart." }
                $expected=if ($case -eq 'update') { 0 } else { 2 }
                Assert-True ($process.ExitCode -eq $expected) "Production Setup case $case returned $($process.ExitCode), expected $expected."
            } finally { $process.Dispose() }
            if ($case -eq 'update') {
                $key=$base.OpenSubKey($keyName)
                try { Assert-True ($key.GetValue('DisplayVersion') -ceq '1.0.1' -and $key.GetValue('TowavueOwnershipId') -ceq $newId -and -not $key.GetValueNames().Contains('TowavuePendingUpdate')) 'Production Setup registration or recovery state differs.' } finally { $key.Dispose() }
                Assert-True ((Get-FileHash -LiteralPath (Join-Path $installed 'towavue.exe')).Hash -eq (Get-FileHash -LiteralPath (Join-Path $incoming 'towavue.exe')).Hash) 'Production Setup installed the wrong executable.'
                Assert-True ([IO.File]::ReadAllText((Join-Path $installed 'user-media.txt'),[Text.Encoding]::UTF8) -ceq 'preserve this file') 'Production Setup changed user data.'
            } else { Assert-True ((Snapshot) -ceq $snapshot) 'Refused automatic installation changed existing state.' }
            if ($case -eq 'fresh') { Assert-True (-not (Test-Path -LiteralPath $destination)) 'Automatic update created a fresh install directory.' }
        }
        Write-Output 'PASS: actual production NSIS branch/version resource, no-flag/fresh-install/same-version refusal and authenticated-handoff-shaped A-to-B update through the real transaction. Prerequisite is a satisfied fixture; no real product registration was used.'
    }
    Write-Output "PASS: production version-bound inventory/plan/schema-4 journal, real PE version replacement and exact rollback, six registration interruption boundaries, unknown typed versions, namespace separation, journal binding and PE mismatch refusal. Isolated evidence: $root"
} finally {
    $base.DeleteSubKeyTree($keyName,$false); $base.Dispose()
    if (Test-Path -LiteralPath $shortcut) { [IO.File]::Delete($shortcut) }
}
