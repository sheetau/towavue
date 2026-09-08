[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$RuntimeDirectory,
    [Parameter(Mandatory)][string]$SourceCompanion,
    [Parameter(Mandatory)][string]$VcRedist,
    [Parameter(Mandatory)][string]$NsisArchive,
    [Parameter(Mandatory)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$manifest = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/setup-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$nsis = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/nsis-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($manifest.schema_version -ne 1 -or $manifest.distribution_approved -ne $false -or
    $manifest.binary_files -ne 95 -or $manifest.companion_files -ne 2879) { throw 'Invalid local Setup manifest.' }
function Assert-NoLink([string]$Path) {
    $current = $Path
    while ($current) {
        if ((Test-Path -LiteralPath $current) -and ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Setup inputs and output must not traverse reparse points.' }
        $parent = Split-Path -Parent $current
        if ($parent -eq $current) { break }
        $current = $parent
    }
}
foreach ($name in @('Executable','RuntimeDirectory','SourceCompanion','VcRedist','NsisArchive','OutputDirectory')) {
    $path = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath((Get-Variable -Name $name -ValueOnly))
    if ($path.StartsWith('\\')) { throw 'Use local drive paths for the Setup build.' }
    Assert-NoLink $path
    Set-Variable -Name $name -Value $path
}
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh Setup output directory.' }
if (-not (Test-Path -LiteralPath (Split-Path -Parent $OutputDirectory) -PathType Container)) { throw 'Setup output parent must exist.' }
foreach ($path in @($Executable,$RuntimeDirectory,$SourceCompanion,$VcRedist,$NsisArchive)) {
    if ($path -eq $OutputDirectory -or $path.StartsWith($OutputDirectory.TrimEnd('\') + '\',[StringComparison]::OrdinalIgnoreCase) -or
        $OutputDirectory.StartsWith($path.TrimEnd('\') + '\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Setup output overlaps an input.' }
}
function Assert-File([string]$Path,$Record) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing Setup input: $Path" }
    Assert-NoLink $Path
    if ((Get-Item -LiteralPath $Path).Length -ne $Record.bytes -or (Get-FileHash -LiteralPath $Path).Hash -ne $Record.sha256) { throw "Setup input identity mismatch: $Path" }
}
function Assert-Name([string]$Name) {
    if ($Name -notmatch '^[A-Za-z0-9_.-][A-Za-z0-9_./+~-]*$' -or $Name -match '(^|/)\.{1,2}(/|$)' -or $Name.EndsWith('/')) { throw 'Unsafe Setup payload path.' }
}
function Get-Record([string]$Path,[string]$Name) {
    return [pscustomobject]@{name=$Name;bytes=(Get-Item -LiteralPath $Path).Length;sha256=(Get-FileHash -LiteralPath $Path).Hash.ToLowerInvariant()}
}
Assert-File $SourceCompanion $manifest.companion
Assert-File $NsisArchive $nsis.archive
$vcState = & (Join-Path $PSScriptRoot 'get-vc-redist-status.ps1') -PackagePath $VcRedist | ConvertFrom-Json
if (-not $vcState.package_verified) { throw 'VC redistributable was not verified.' }

Add-Type -AssemblyName System.IO.Compression,System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead($SourceCompanion)
try {
    $entries = @{}
    foreach ($entry in $archive.Entries) {
        Assert-Name $entry.FullName
        if ((($entry.ExternalAttributes -shr 16) -band 0xf000) -eq 0xa000) { throw 'Source companion contains a link.' }
        $entries.Add($entry.FullName,$entry)
    }
    if ($entries.Count -ne $manifest.companion_files -or -not $entries.ContainsKey('BINDING.json')) { throw 'Source companion coverage mismatch.' }
    $reader = [IO.StreamReader]::new($entries['BINDING.json'].Open(),[Text.Encoding]::UTF8)
    try { $binding = $reader.ReadToEnd() | ConvertFrom-Json } finally { $reader.Dispose() }
    if ($binding.files.Count -ne $manifest.binary_files -or $binding.distribution_approved -ne $false) { throw 'Invalid candidate binary binding.' }
    $binaries = @{}
    foreach ($file in $binding.files) {
        Assert-Name $file.name
        if ($file.name.Contains('/')) { throw 'Runtime files must be adjacent to the application.' }
        $path = if ($file.name -eq 'towavue.exe') { $Executable } else { Join-Path $RuntimeDirectory $file.name }
        Assert-File $path $file
        $binaries.Add($file.name,$path)
    }
    $runtimeItems = @(Get-ChildItem -LiteralPath $RuntimeDirectory -Force)
    if (-not $binaries.ContainsKey('towavue.exe') -or $runtimeItems.Count -ne 94) { throw 'Setup runtime coverage mismatch.' }
    foreach ($item in $runtimeItems) {
        if ($item.PSIsContainer -or -not $binaries.ContainsKey($item.Name)) { throw 'Unexpected Setup runtime entry.' }
    }

    # All fixed archives, prerequisite identity and actual binaries pass before output.
    New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
    $payload = Join-Path $OutputDirectory 'payload'
    New-Item -ItemType Directory -Path $payload | Out-Null
    foreach ($name in $binaries.Keys) { Copy-Item -LiteralPath $binaries[$name] -Destination (Join-Path $payload $name) }
    $licenses = Join-Path $payload 'licenses'
    New-Item -ItemType Directory -Path $licenses | Out-Null
    $companionEntries = [Collections.Generic.List[object]]::new()
    $entryNames = [string[]]@($entries.Keys)
    [Array]::Sort($entryNames,[StringComparer]::Ordinal)
    foreach ($name in $entryNames) {
        # Preserve every non-source-archive original, including Rust's notice ZIP.
        # Catalog inventories describe the full companion, not this installed subset.
        if ($name -ne 'BINDING.json' -and -not $name.StartsWith('catalog/')) { continue }
        if ($name -match '\.(tar|gz|xz|tgz|lz|zst|crate)$') { continue }
        # Short deterministic names keep normal Windows installation paths usable.
        # Original relative names are mapped in the installed inventory and guide.
        $relative = 'companion-records/' + ('{0:D4}-' -f $companionEntries.Count) + (Split-Path -Leaf $name)
        $companionEntries.Add([ordered]@{installed=$relative;companion=$name})
        $destination = Join-Path $licenses $relative
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entries[$name],$destination,$false)
    }
}
finally { $archive.Dispose() }
$bindingEntry = @($companionEntries | Where-Object { $_.companion -eq 'BINDING.json' })
Assert-File (Join-Path $licenses $bindingEntry[0].installed) $manifest.binding

$toolArchive = [IO.Compression.ZipFile]::OpenRead($NsisArchive)
try {
    foreach ($entry in $toolArchive.Entries) {
        if ($entry.FullName -notmatch '^nsis-3\.12/' -or $entry.FullName -match '(^/|(^|/)\.\.(/|$)|:|\\)' -or
            (($entry.ExternalAttributes -shr 16) -band 0xf000) -eq 0xa000) { throw 'Unsafe NSIS archive entry.' }
    }
}
finally { $toolArchive.Dispose() }
$toolDirectory = Join-Path $OutputDirectory 'toolchain'
[IO.Compression.ZipFile]::ExtractToDirectory($NsisArchive,$toolDirectory)
$compiler = Join-Path $toolDirectory 'nsis-3.12/makensis.exe'
$version = & $compiler /VERSION
if ($LASTEXITCODE -ne 0 -or $version -ne 'v3.12') { throw 'Wrong NSIS compiler version.' }
Copy-Item -LiteralPath (Join-Path $toolDirectory 'nsis-3.12/COPYING') -Destination (Join-Path $licenses 'NSIS-COPYING.txt')
$prerequisite = Join-Path $OutputDirectory 'prerequisite'
New-Item -ItemType Directory -Path (Join-Path $prerequisite 'scripts'),(Join-Path $prerequisite 'docs') | Out-Null
foreach ($name in @('get-vc-redist-status.ps1','vc-redist-state.ps1')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $prerequisite 'scripts') }
Copy-Item -LiteralPath (Join-Path $repositoryRoot 'packaging/windows/prerequisite.ps1') -Destination (Join-Path $prerequisite 'scripts')
Copy-Item -LiteralPath (Join-Path $repositoryRoot 'docs/vc-redist-inputs.json') -Destination (Join-Path $prerequisite 'docs')
Copy-Item -LiteralPath $VcRedist -Destination (Join-Path $prerequisite 'vc_redist.x64.exe')

$utf8 = [Text.UTF8Encoding]::new($false)
$noticeFiles = @(Get-ChildItem -LiteralPath $licenses -File -Recurse -Force | ForEach-Object { $_.FullName.Substring($licenses.Length + 1).Replace('\','/') })
[Array]::Sort($noticeFiles,[StringComparer]::Ordinal)
$page = [Collections.Generic.List[string]]::new()
$page.Add('<!doctype html><html lang="en"><meta charset="utf-8"><title>towavue licenses and sources</title><style>body{font:16px system-ui;max-width:1000px;margin:3em auto;padding:0 1em;line-height:1.6}code,a{overflow-wrap:anywhere}</style><h1>towavue licenses and sources</h1>')
$page.Add('<p>Local evaluation only, not an approved or published release. towavue is MIT OR Apache-2.0. Third-party components retain their original terms. Compatible FFmpeg DLL replacement is not blocked by a startup hash allowlist.</p>')
$mit = @($companionEntries | Where-Object { $_.companion -eq 'catalog/materials/app-materials-v2/LICENSE-MIT' })[0].installed
$apache = @($companionEntries | Where-Object { $_.companion -eq 'catalog/materials/app-materials-v2/LICENSE-APACHE' })[0].installed
$page.Add('<p><a href="' + $mit + '">MIT</a> OR <a href="' + $apache + '">Apache-2.0</a> &middot; <a href="NSIS-COPYING.txt">NSIS notices</a> &middot; <a href="INSTALLED-FILES.json">Installed payload inventory</a></p>')
$page.Add('<h2>Corresponding sources</h2><p>The complete application and native sources, patches, build instructions and original notices are in the separate companion <code>' + $manifest.companion.name + '</code> (' + $manifest.companion.bytes + ' bytes), SHA256 <code>' + $manifest.companion.sha256 + '</code>. Extract it into a separate folder and start with its START-HERE.html. This local evaluation has no public download URL. A public release must provide this matching companion alongside Setup. Installer sources are not in this older app/native companion; installer source delivery remains a release gate.</p>')
$page.Add('<p>The companion-records folder below retains every non-source-archive original under short numbered names. Link labels and the installed inventory map each file to its original companion path. Upstream READMEs and inventories refer to the complete companion, including archives intentionally not installed here. Use the extracted companion for those references; do not interpret its FILES.json as this installed tree. The Rust runtime notice ZIP is retained. No license text has been shortened or replaced.</p>')
$page.Add('<p>The shared Microsoft Visual C++ prerequisite is a separate Microsoft component. Its original installer displays its terms when required; towavue removal never removes the shared runtime. No app startup, browser launch or source download happens automatically.</p><h2>Component guides</h2><ul>')
foreach ($entry in $companionEntries) {
    if ($entry.companion -notmatch '^catalog/materials/([^/]+)/README\.txt$') { continue }
    $page.Add('<li><a href="' + $entry.installed + '">' + [Net.WebUtility]::HtmlEncode($Matches[1]) + '</a></li>')
}
$page.Add('</ul><details><summary>All local original notices and records</summary><ul>')
$originalNames = @{}
foreach ($entry in $companionEntries) { $originalNames.Add($entry.installed,$entry.companion) }
foreach ($name in $noticeFiles) {
    $link = (($name.Split('/') | ForEach-Object { [uri]::EscapeDataString($_) }) -join '/')
    $label = if ($originalNames.ContainsKey($name)) { $originalNames[$name] } else { $name }
    $page.Add('<li><a href="' + $link + '">' + [Net.WebUtility]::HtmlEncode($label) + '</a></li>')
}
$page.Add('</ul></details></html>')
[IO.File]::WriteAllText((Join-Path $licenses 'START-HERE.html'),($page -join "`n") + "`n",$utf8)

$records = @(Get-ChildItem -LiteralPath $payload -File -Recurse -Force | ForEach-Object { Get-Record $_.FullName $_.FullName.Substring($payload.Length + 1).Replace('\','/') })
$records = @($records | Sort-Object name)
$buildSources = @('packaging/windows/setup.nsi','packaging/windows/prerequisite.ps1','scripts/build-local-setup.ps1','scripts/get-vc-redist-status.ps1','scripts/vc-redist-state.ps1','docs/setup-inputs.json','docs/nsis-inputs.json','docs/vc-redist-inputs.json')
$sourceRecords = @($buildSources | ForEach-Object { Get-Record (Join-Path $repositoryRoot $_) $_ })
$inventory = [ordered]@{schema_version=1;scope='Installed payload only; excludes this inventory itself, generated uninstaller and path-bound marker. Local evaluation, not release approval.';companion=$manifest.companion;companion_entries=@($companionEntries);build_sources=$sourceRecords;files=$records}
$inventoryPath = Join-Path $licenses 'INSTALLED-FILES.json'
[IO.File]::WriteAllText($inventoryPath,($inventory | ConvertTo-Json -Depth 8) + "`n",$utf8)
$records += Get-Record $inventoryPath 'licenses/INSTALLED-FILES.json'
$names = [string[]]@($records | ForEach-Object { $_.name })
[Array]::Sort($names,[StringComparer]::Ordinal)
$directories = @{}
foreach ($name in $names) {
    $parent = $name
    while ($parent.Contains('/')) { $parent = $parent.Substring(0,$parent.LastIndexOf('/')); $directories[$parent] = $true }
}
$directoryNames = [string[]]@($directories.Keys)
[Array]::Sort($directoryNames,[StringComparer]::Ordinal)
function Nsis-Literal([string]$Text) { return $Text.Replace('$','$$').Replace('/','\') }
$include = [Collections.Generic.List[string]]::new()
$include.Add('!define OWNERSHIP_ID "towavue-local-' + (Get-FileHash -LiteralPath $inventoryPath).Hash.ToLowerInvariant() + '"')
$include.Add('!define PAYLOAD_MAX_PATH ' + (($names | ForEach-Object { $_.Length + 1 } | Measure-Object -Maximum).Maximum))
$include.Add('!macro InstallApplicationFiles')
$lastDirectory = $null
foreach ($name in $names) {
    $directory = if ($name.Contains('/')) { '\' + (Nsis-Literal $name.Substring(0,$name.LastIndexOf('/'))) } else { '' }
    if ($directory -ne $lastDirectory) { $include.Add('  SetOutPath "$INSTDIR' + $directory + '"'); $lastDirectory = $directory }
    $include.Add('  File "${TRIAL_ROOT}\payload\' + (Nsis-Literal $name) + '"')
}
$include.Add('!macroend')
$include.Add('!macro RemoveApplicationFiles')
foreach ($name in $names) { $include.Add('  Delete "$INSTDIR\' + (Nsis-Literal $name) + '"') }
$include.Add('!macroend')
$include.Add('!macro CheckApplicationDirectories')
foreach ($name in $directoryNames) {
    $include.Add('  ${If} $PathError == ""')
    $include.Add('    StrCpy $INSTDIR "$3\' + (Nsis-Literal $name) + '"')
    $include.Add('    Call un.CheckPath')
    $include.Add('  ${EndIf}')
}
$include.Add('!macroend')
$include.Add('!macro RemoveApplicationDirectories')
[Array]::Reverse($directoryNames)
foreach ($name in $directoryNames) { $include.Add('  RMDir "$INSTDIR\' + (Nsis-Literal $name) + '"') }
$include.Add('!macroend')
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'payload.nsh'),($include -join "`n") + "`n",$utf8)
& $compiler /NOCONFIG /WX /V2 /DTOWAVUE_SETUP_APPLICATION ("/DTRIAL_ROOT=" + $OutputDirectory.Replace('$','$$')) (Join-Path $repositoryRoot 'packaging/windows/setup.nsi')
if ($LASTEXITCODE -ne 0) { throw 'Local application Setup compilation failed; output is incomplete.' }
foreach ($record in $records) { Assert-File (Join-Path $payload $record.name) $record }
foreach ($file in $binding.files) { Assert-File $binaries[$file.name] $file }
foreach ($record in $sourceRecords) { Assert-File (Join-Path $repositoryRoot $record.name) $record }
Assert-File $SourceCompanion $manifest.companion
Assert-File $NsisArchive $nsis.archive
$setup = Get-Record (Join-Path $OutputDirectory 'Setup-local.exe') 'Setup-local.exe'
$result = [ordered]@{schema_version=1;scope='Compiled only; not installed, published or approved. Update, registration and supported-Windows lifecycle are pending.';setup=$setup;payload_files=$records.Count;payload_bytes=($records | Measure-Object bytes -Sum).Sum;source_companion=$manifest.companion}
# Completion is recorded only after compilation and final input/staging verification.
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'BUILD.json'),($result | ConvertTo-Json -Depth 6) + "`n",$utf8)
$result | ConvertTo-Json -Depth 6
