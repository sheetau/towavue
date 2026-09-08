[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BuildDirectory,
    [Parameter(Mandatory)][string]$SourceCompanion,
    [Parameter(Mandatory)][string]$NsisArchive
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$BuildDirectory = (Resolve-Path -LiteralPath $BuildDirectory).Path
$SourceCompanion = (Resolve-Path -LiteralPath $SourceCompanion).Path
$NsisArchive = (Resolve-Path -LiteralPath $NsisArchive).Path
$build = Get-Content -LiteralPath (Join-Path $BuildDirectory 'BUILD.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$pins = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/setup-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$payload = Join-Path $BuildDirectory 'payload'
$licenses = Join-Path $payload 'licenses'
$inventoryPath = Join-Path $licenses 'INSTALLED-FILES.json'
$inventory = Get-Content -LiteralPath $inventoryPath -Raw -Encoding UTF8 | ConvertFrom-Json
$trialRoot = Join-Path $repositoryRoot ('target/tmp/local-setup-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $trialRoot | Out-Null
function Assert-True([bool]$Condition,[string]$Message) { if (-not $Condition) { throw $Message } }
function Assert-File([string]$Path,$Record) {
    Assert-True ((Get-Item -LiteralPath $Path).Length -eq $Record.bytes -and (Get-FileHash -LiteralPath $Path).Hash -eq $Record.sha256) "File identity mismatch: $Path"
}
Assert-File $SourceCompanion $pins.companion
$setup = Join-Path $BuildDirectory 'Setup-local.exe'
Assert-File $setup $build.setup
$actual = @(Get-ChildItem -LiteralPath $payload -File -Recurse -Force)
Assert-True ($actual.Count -eq $inventory.files.Count + 1 -and $actual.Count -eq $build.payload_files) 'Installed payload coverage mismatch.'
$names = @{}
foreach ($file in $inventory.files) { $names.Add($file.name,$true); Assert-File (Join-Path $payload $file.name) $file }
$names.Add('licenses/INSTALLED-FILES.json',$true)
foreach ($file in $actual) { Assert-True ($names.ContainsKey($file.FullName.Substring($payload.Length + 1).Replace('\','/'))) 'Unexpected payload file.' }
foreach ($file in $inventory.build_sources) { Assert-File (Join-Path $repositoryRoot $file.name) $file }
foreach ($name in @('registration.ps1','registration-state.ps1','UnicodeShellLink.cs')) {
    $record = @($inventory.build_sources | Where-Object { $_.name -eq "packaging/windows/$name" })[0]
    Assert-File (Join-Path $BuildDirectory "registration/$name") $record
}
$bindingEntry = @($inventory.companion_entries | Where-Object { $_.companion -eq 'BINDING.json' })
Assert-True ($bindingEntry.Count -eq 1) 'Missing original candidate binding.'
$bindingPath = Join-Path $licenses $bindingEntry[0].installed
Assert-File $bindingPath $pins.binding
$binding = Get-Content -LiteralPath $bindingPath -Raw -Encoding UTF8 | ConvertFrom-Json
Assert-True ($binding.files.Count -eq 95) 'Wrong binary count.'
foreach ($file in $binding.files) { Assert-File (Join-Path $payload $file.name) $file }

Add-Type -AssemblyName System.IO.Compression,System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead($SourceCompanion)
try {
    $selected = @{}
    foreach ($entry in $archive.Entries) {
        if (($entry.FullName -eq 'BINDING.json' -or $entry.FullName.StartsWith('catalog/')) -and
            $entry.FullName -notmatch '\.(tar|gz|xz|tgz|lz|zst|crate)$') { $selected.Add($entry.FullName,$entry) }
    }
    Assert-True ($selected.Count -eq $inventory.companion_entries.Count) 'Missing non-archive original.'
    $originalNames = @{}
    foreach ($mapping in $inventory.companion_entries) {
        Assert-True ($selected.ContainsKey($mapping.companion)) 'Unknown companion mapping.'
        $originalNames.Add($mapping.companion,$true)
        $entry = $selected[$mapping.companion]
        $stream = $entry.Open()
        $sha = [Security.Cryptography.SHA256]::Create()
        try { $hash = [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','') }
        finally { $sha.Dispose(); $stream.Dispose() }
        Assert-File (Join-Path $licenses $mapping.installed) @{bytes=$entry.Length;sha256=$hash}
    }
    Assert-True ($originalNames.ContainsKey('catalog/materials/app-materials-v2/TOWAVUE-RUST-RUNTIME-NOTICES.zip')) 'Required Rust runtime notice ZIP was omitted.'
}
finally { $archive.Dispose() }
$html = Get-Content -LiteralPath (Join-Path $licenses 'START-HERE.html') -Raw -Encoding UTF8
$links = [regex]::Matches($html,'href="([^"]+)"')
foreach ($link in $links) {
    $relative = [uri]::UnescapeDataString($link.Groups[1].Value)
    $path = [IO.Path]::GetFullPath((Join-Path $licenses $relative))
    Assert-True ($path.StartsWith($licenses + '\',[StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $path -PathType Leaf)) 'Guide link is missing or escapes installed licenses.'
}
Assert-True ($html.Contains($pins.companion.name) -and $html.Contains($pins.companion.sha256)) 'Source companion identity is not visible.'
$include = Get-Content -LiteralPath (Join-Path $BuildDirectory 'payload.nsh') -Raw -Encoding UTF8
$installed = [regex]::Matches($include,'(?m)^  File "\$\{TRIAL_ROOT\}\\payload\\([^"]+)"$')
$removed = [regex]::Matches($include,'(?m)^  Delete "\$INSTDIR\\([^"]+)"$')
Assert-True ($installed.Count -eq $names.Count -and $removed.Count -eq $names.Count) 'Installer/deletion inventory count differs.'
foreach ($matches in @($installed,$removed)) {
    $seen = @{}
    foreach ($match in $matches) {
        $name = $match.Groups[1].Value.Replace('\','/')
        Assert-True ($names.ContainsKey($name)) 'Generated installer owns an unexpected path.'
        $seen.Add($name,$true)
    }
}
Assert-True ($include.Contains('towavue-local-' + (Get-FileHash -LiteralPath $inventoryPath).Hash.ToLowerInvariant())) 'Ownership marker is not bound to this payload inventory.'

function Invoke-Probe([string]$Arguments,[int]$Expected) {
    $process = Start-Process -FilePath $setup -ArgumentList $Arguments -WorkingDirectory $trialRoot -WindowStyle Hidden -PassThru
    [void]$process.Handle
    if (-not $process.WaitForExit(15000)) { throw "Read-only Setup probe still running; do not restart: PID $($process.Id), start UTC $($process.StartTime.ToUniversalTime().ToString('o'))." }
    Assert-True ($process.ExitCode -eq $Expected) "Wrong read-only Setup result: $($process.ExitCode), expected $Expected : $Arguments"
}
$destination = Join-Path $trialRoot 'must-not-be-installed'
Invoke-Probe "/S /CHECKONLY /CHECKPATH=$destination" 0
Invoke-Probe ('/S /CHECKONLY /CHECKPATH=' + [IO.Path]::GetPathRoot($trialRoot)) 2
Invoke-Probe '/S /CHECKONLY /CHECKPATH=\\localhost\not-a-towavue-test' 2
Invoke-Probe ('/S /CHECKONLY /CHECKPATH=' + (Join-Path $trialRoot ('x' * 100))) 2
Invoke-Probe "/S /D=$destination" 2
Assert-True (-not (Test-Path -LiteralPath $destination)) 'Read-only/silent-refusal probe installed application files.'
# Inspect the packaged reader/package only. Never pass Install on the host.
$package = Join-Path $BuildDirectory 'prerequisite/vc_redist.x64.exe'
$readerPath = Join-Path $BuildDirectory 'prerequisite/scripts/get-vc-redist-status.ps1'
$before = & $readerPath -PackagePath $package | ConvertFrom-Json
Assert-True $before.package_verified 'Packaged Microsoft binary was not verified.'
foreach ($relative in @('System32','SysWOW64')) {
    $powershell = Join-Path $env:SystemRoot "$relative/WindowsPowerShell/v1.0/powershell.exe"
    $wrapper = Join-Path $BuildDirectory 'prerequisite/scripts/prerequisite.ps1'
    $arguments = '-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "{0}" -Mode Inspect -PackagePath "{1}"' -f $wrapper,$package
    $process = Start-Process -FilePath $powershell -ArgumentList $arguments -WorkingDirectory $trialRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $trialRoot "$relative-inspect.txt") -RedirectStandardError (Join-Path $trialRoot "$relative-error.txt")
    [void]$process.Handle
    if (-not $process.WaitForExit(15000)) { throw "Read-only prerequisite still running; do not restart: PID $($process.Id), start UTC $($process.StartTime.ToUniversalTime().ToString('o'))." }
    $expected = if ($before.state -eq 'satisfied') { 0 } elseif ($before.state -eq 'required') { 10 } else { 20 }
    Assert-True ($process.ExitCode -eq $expected) 'Packaged prerequisite wrapper differs from the registry reader.'
    $registrationWrapper = Join-Path $BuildDirectory 'registration/registration.ps1'
    $ownership = 'towavue-local-' + (Get-FileHash -LiteralPath $inventoryPath).Hash.ToLowerInvariant()
    $sizeKiB = [int][Math]::Ceiling($build.payload_bytes / 1024)
    $registrationArguments = '-STA -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "{0}" -Mode Inspect -InstallDirectory "{1}" -OwnershipId {2} -SizeKiB {3}' -f $registrationWrapper,$payload,$ownership,$sizeKiB
    $registrationOutput = Join-Path $trialRoot "$relative-registration.txt"
    $process = Start-Process -FilePath $powershell -ArgumentList $registrationArguments -WorkingDirectory $trialRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput $registrationOutput -RedirectStandardError (Join-Path $trialRoot "$relative-registration-error.txt")
    [void]$process.Handle
    if (-not $process.WaitForExit(15000)) { throw "Read-only registration still running; do not restart: PID $($process.Id), start UTC $($process.StartTime.ToUniversalTime().ToString('o'))." }
    if ($process.ExitCode -eq 20 -and (Get-Content -LiteralPath $registrationOutput -Raw).Contains('existing registration or shortcut')) {
        Write-Output 'SKIP: the real per-user registration destination is occupied; its read-only refusal is expected. No registration was changed.'
    }
    else { Assert-True ($process.ExitCode -eq 0) 'Packaged read-only registration preflight failed.' }
}
$after = & $readerPath -PackagePath $package | ConvertFrom-Json
Assert-True (($before | ConvertTo-Json -Compress) -eq ($after | ConvertTo-Json -Compress)) 'Live prerequisite state changed during read-only inspection.'
foreach ($name in @('get-vc-redist-status.ps1','vc-redist-state.ps1','prerequisite.ps1')) {
    $original = if ($name -eq 'prerequisite.ps1') { Join-Path $repositoryRoot 'packaging/windows/prerequisite.ps1' } else { Join-Path $PSScriptRoot $name }
    Assert-True ((Get-FileHash -LiteralPath $original).Hash -eq (Get-FileHash -LiteralPath (Join-Path $BuildDirectory "prerequisite/scripts/$name")).Hash) 'Staged prerequisite script differs from its source.'
}
# Reject altered/missing inputs before output, without compiling another installer.
$runtime = Join-Path $trialRoot 'runtime'
New-Item -ItemType Directory -Path $runtime | Out-Null
foreach ($file in $binding.files) {
    if ($file.name -ne 'towavue.exe') { Copy-Item -LiteralPath (Join-Path $payload $file.name) -Destination (Join-Path $runtime $file.name) }
}
$badInput = Join-Path $trialRoot 'invalid-input.bin'
[IO.File]::WriteAllText($badInput,'Deliberately invalid local Setup input.',[Text.UTF8Encoding]::new($false))
$builder = Join-Path $PSScriptRoot 'build-local-setup.ps1'
$arguments = @{Executable=(Join-Path $payload 'towavue.exe');RuntimeDirectory=$runtime;SourceCompanion=$SourceCompanion;VcRedist=$package;NsisArchive=$NsisArchive;OutputDirectory=(Join-Path $trialRoot 'rejected-output')}
$failures = @(
    @{field='OutputDirectory';value=$BuildDirectory;message='Use a fresh Setup output directory.'},
    @{field='OutputDirectory';value=(Join-Path $runtime 'nested-output');message='Setup output overlaps an input.'},
    @{field='SourceCompanion';value=(Join-Path $trialRoot 'missing.zip');message='Missing Setup input:'},
    @{field='SourceCompanion';value=$badInput;message='Setup input identity mismatch:'},
    @{field='NsisArchive';value=$badInput;message='Setup input identity mismatch:'},
    @{field='Executable';value=$badInput;message='Setup input identity mismatch:'},
    @{field='VcRedist';value=$badInput;message='VC redistributable package identity mismatch.'}
)
foreach ($failure in $failures) {
    $invocation = $arguments.Clone()
    $invocation[$failure.field] = $failure.value
    $rejected = $false
    try { & $builder @invocation | Out-Null }
    catch { if (-not $_.Exception.Message.StartsWith($failure.message)) { throw }; $rejected = $true }
    Assert-True $rejected "Bad Setup input accepted: $($failure.field)"
    Assert-True (-not (Test-Path -LiteralPath $arguments.OutputDirectory)) 'Rejected input produced an output directory.'
}
$extra = Join-Path $runtime 'unlisted-user-file.txt'
[IO.File]::WriteAllText($extra,'Preserve this extra file.',[Text.UTF8Encoding]::new($false))
$rejected = $false
try { & $builder @arguments | Out-Null }
catch { if ($_.Exception.Message -ne 'Setup runtime coverage mismatch.') { throw }; $rejected = $true }
Assert-True ($rejected -and -not (Test-Path -LiteralPath $arguments.OutputDirectory)) 'Extra runtime entry accepted or output created.'
Assert-True ([IO.File]::ReadAllText($extra) -eq 'Preserve this extra file.') 'Rejected runtime input was changed.'
foreach ($file in $binding.files) { Assert-File (Join-Path $payload $file.name) $file }
Assert-File $setup $build.setup
Assert-File $SourceCompanion $pins.companion
Write-Output "PASS: 95 binary hashes, $($selected.Count) original mappings/bytes, $($links.Count) local links, $($names.Count) explicit install/delete paths, eight input rejections, five non-installing Setup probes and read-only packaged prerequisite inspection. Evidence: $trialRoot"
Write-Output 'SKIP: actual application installation, native prerequisite UI/UAC/reboot and self-copy uninstall require an isolated supported-Windows environment. No application or redistributable was installed.'
