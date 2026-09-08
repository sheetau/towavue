[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [Parameter(Mandatory = $true)][string]$RuntimeDirectory,
    [Parameter(Mandatory = $true)][string]$CatalogDirectory,
    [Parameter(Mandatory = $true)][string]$ApplicationSource,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $repositoryRoot 'docs/candidate-material-inputs.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
if ($manifest.schema_version -ne 1 -or $manifest.catalog_files -ne 2875 -or
    $manifest.application_source_commit -notmatch '^[0-9a-f]{40}$') { throw 'Incomplete candidate material inventory.' }
foreach ($name in @('Executable','RuntimeDirectory','CatalogDirectory','ApplicationSource','OutputDirectory')) {
    Set-Variable -Name $name -Value ($ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath((Get-Variable -Name $name -ValueOnly)))
}
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh candidate material output directory.' }
foreach ($inputPath in @($Executable,$RuntimeDirectory,$CatalogDirectory,$ApplicationSource)) {
    if ($inputPath -eq $OutputDirectory -or
        $inputPath.StartsWith($OutputDirectory.TrimEnd('\','/') + '\',[StringComparison]::OrdinalIgnoreCase) -or
        $OutputDirectory.StartsWith($inputPath.TrimEnd('\','/') + '\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Candidate material output overlaps an input.' }
}
function Assert-Path([string]$Name) {
    if ($Name -notmatch '^[A-Za-z0-9_.-][A-Za-z0-9_./+~-]*$' -or $Name -match '(^|/)\.{1,2}(/|$)') { throw 'Invalid candidate material path.' }
}
function Assert-File([string]$Path, $Record) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing candidate material: $Path" }
    $item = Get-Item -LiteralPath $Path
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Candidate material links are not allowed.' }
    if ($item.Length -ne $Record.bytes -or (Get-FileHash -LiteralPath $Path).Hash -ne $Record.sha256) { throw "Candidate material checksum mismatch: $Path" }
}
function Get-Files([string]$Root) {
    $pending = [Collections.Generic.Queue[string]]::new()
    $pending.Enqueue($Root)
    $files = @{}
    while ($pending.Count) {
        $directory = $pending.Dequeue()
        if ((Get-Item -LiteralPath $directory).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Candidate material links are not allowed.' }
        foreach ($item in Get-ChildItem -LiteralPath $directory -Force) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Candidate material links are not allowed.' }
            if ($item.PSIsContainer) { $pending.Enqueue($item.FullName); continue }
            $name = $item.FullName.Substring($Root.Length + 1).Replace('\','/')
            Assert-Path $name
            $files[$name] = $item.FullName
        }
    }
    return $files
}
Assert-Path $manifest.application_source.name
if ($manifest.application_source.name.Contains('/')) { throw 'Invalid application source archive name.' }
Assert-File $ApplicationSource $manifest.application_source
$sourceBytes = [IO.File]::ReadAllBytes($ApplicationSource)
if ($sourceBytes.Length -lt 62 -or [BitConverter]::ToUInt32($sourceBytes,$sourceBytes.Length - 62) -ne 0x06054b50 -or
    [BitConverter]::ToUInt16($sourceBytes,$sourceBytes.Length - 42) -ne 40 -or
    [Text.Encoding]::ASCII.GetString($sourceBytes,$sourceBytes.Length - 40,40) -ne $manifest.application_source_commit) { throw 'Application source commit differs from archive.' }
Assert-File (Join-Path $CatalogDirectory 'FILES.json') $manifest.catalog_inventory
Assert-File (Join-Path $CatalogDirectory 'CATALOG.json') $manifest.catalog_marker
$catalogFiles = Get-Files $CatalogDirectory
$records = Get-Content -LiteralPath (Join-Path $CatalogDirectory 'FILES.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$records += @($manifest.catalog_inventory,$manifest.catalog_marker)
if ($catalogFiles.Count -ne $manifest.catalog_files -or $records.Count -ne $catalogFiles.Count) { throw 'Candidate catalog coverage mismatch.' }
$catalogNames = @{}
foreach ($record in $records) {
    Assert-Path $record.name
    if ($catalogNames.ContainsKey($record.name)) { throw 'Duplicate candidate catalog path.' }
    $catalogNames[$record.name] = $record
}
foreach ($name in @('materials/app-materials-v2/INPUTS.json','materials/native-ffmpeg-materials-v1/RUNTIME.json','native-runtime-package-audit.json')) {
    if (-not $catalogNames.ContainsKey($name)) { throw 'Missing candidate binding inventory.' }
    Assert-File (Join-Path $CatalogDirectory $name) $catalogNames[$name]
}
$app = Get-Content -LiteralPath (Join-Path $CatalogDirectory 'materials/app-materials-v2/INPUTS.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$runtime = Get-Content -LiteralPath (Join-Path $CatalogDirectory 'materials/native-ffmpeg-materials-v1/RUNTIME.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$audit = Get-Content -LiteralPath (Join-Path $CatalogDirectory 'native-runtime-package-audit.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$catalog = Get-Content -LiteralPath (Join-Path $CatalogDirectory 'CATALOG.json') -Raw -Encoding UTF8 | ConvertFrom-Json
Assert-File $Executable $app.candidate
$runtimeFiles = Get-Files $RuntimeDirectory
if ($runtime.Count -ne 94 -or $runtimeFiles.Count -ne 94) { throw 'Candidate runtime coverage mismatch.' }
$ffmpegNames = @('ffmpeg.exe','ffprobe.exe','avcodec-63.dll','avdevice-63.dll','avfilter-12.dll','avformat-63.dll','avutil-61.dll','swresample-7.dll','swscale-10.dll')
$bindings = [Collections.Generic.List[object]]::new()
$bindings.Add([ordered]@{name=$app.candidate.name;bytes=$app.candidate.bytes;sha256=$app.candidate.sha256;material='catalog/materials/app-materials-v2/README.txt';source=$manifest.application_source.name;source_commit=$manifest.application_source_commit})
$runtimeNames = @{}
$packageCount = 0
foreach ($file in $runtime) {
    Assert-Path $file.name
    if ($file.name.Contains('/') -or $runtimeNames.ContainsKey($file.name)) { throw 'Invalid candidate runtime name.' }
    $runtimeNames[$file.name] = $true
    Assert-File (Join-Path $RuntimeDirectory $file.name) $file
    if ($file.name -in $ffmpegNames) { $material = 'catalog/materials/native-ffmpeg-materials-v1/README.txt' }
    elseif ($file.name -eq 'libzvbi-0.dll') { $material = 'catalog/materials/zvbi-scoped-materials-v3/README.txt' }
    else {
        $baseline = @($audit.runtime | Where-Object { $_.name -eq $file.name -and $_.sha256 -eq $file.sha256 -and $_.bytes -eq $file.bytes })
        $owner = @($audit.packages | Where-Object { $_.runtime_dlls -contains $file.name -and $_.name -ne 'mingw-w64-x86_64-zvbi' })
        if ($baseline.Count -ne 1 -or $owner.Count -ne 1) { throw 'Candidate package binding mismatch.' }
        $material = 'catalog/packages/' + $owner[0].name + '/PKGBUILD'
        $packageCount++
    }
    if (-not (Test-Path -LiteralPath (Join-Path $CatalogDirectory $material.Substring(8)) -PathType Leaf)) { throw 'Missing candidate material binding.' }
    $bindings.Add([ordered]@{name=$file.name;bytes=$file.bytes;sha256=$file.sha256;material=$material;imports=$file.imports})
}
if ($packageCount -ne 84 -or @($runtimeNames.Keys | Where-Object { $_ -in $ffmpegNames }).Count -ne 9 -or
    -not $runtimeNames.ContainsKey('libzvbi-0.dll')) { throw 'Candidate replacement coverage mismatch.' }
foreach ($file in $runtime) {
    foreach ($import in $file.imports) {
        if ($import.kind -eq 'runtime' -and -not $runtimeNames.ContainsKey($import.name)) { throw 'Missing candidate runtime import.' }
    }
}
foreach ($record in $records) { Assert-File (Join-Path $CatalogDirectory $record.name) $record }

# All fixed inputs are verified before output. Copy sources/notices only, never candidate binaries.
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
foreach ($record in $records) {
    $path = Join-Path $OutputDirectory ('catalog/' + $record.name)
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $CatalogDirectory $record.name) -Destination $path
    Assert-File $path $record
}
$sourcePath = Join-Path $OutputDirectory $manifest.application_source.name
Copy-Item -LiteralPath $ApplicationSource -Destination $sourcePath
Assert-File $sourcePath $manifest.application_source
$utf8 = [Text.UTF8Encoding]::new($false)
function Html([string]$Text) { return [Net.WebUtility]::HtmlEncode($Text) }
function Link([string]$Path) { return (($Path.Split('/') | ForEach-Object { [uri]::EscapeDataString($_) }) -join '/') }
$page = @('<!doctype html>','<html lang="en"><meta charset="utf-8"><title>towavue sources and notices</title>',
    '<style>body{font:16px system-ui;max-width:1100px;margin:3em auto;padding:0 1em;line-height:1.6}table{border-collapse:collapse;width:100%}td,th{padding:.4em;text-align:left;border-bottom:1px solid #aaa}code{overflow-wrap:anywhere}a{overflow-wrap:anywhere}</style>',
    '<h1>towavue sources and notices</h1>',
    '<p>Local evaluation candidate &#8212; not a published or approved release. This directory contains source and notice materials, not an installer or the application binaries.</p>',
    '<h2>Application</h2>',
    ('<p><a href="' + (Link $manifest.application_source.name) + '">Application source ZIP</a> &middot; commit <code>' + $manifest.application_source_commit + '</code>. This is the source snapshot recorded for the evaluated executable, not a claim of a bit-for-bit reproducible build. It includes Cargo.lock and the pinned toolchain; dependency downloads and the separately supplied native build inputs are needed to rebuild.</p>'),
    '<p><a href="catalog/materials/app-materials-v2/LICENSE-MIT">MIT</a> OR <a href="catalog/materials/app-materials-v2/LICENSE-APACHE">Apache-2.0</a> applies to towavue. Third-party code, fonts and data retain their own notices and original alternatives.</p>',
    '<h2>Component sources and notices</h2><ul>')
foreach ($kit in $catalog.kits) {
    $page += '<li><a href="' + (Link ('catalog/materials/' + $kit.name + '/README.txt')) + '">' + (Html $kit.title) + '</a> &#8212; ' + (Html $kit.description) + '</li>'
}
$page += @('</ul>',
    '<p>Read the component README, then its INPUTS.json (SCOPED-INPUTS.json for scoped ZVBI) for original archives, patches and build instructions. <a href="catalog/PACKAGES.md">Package records and notices</a> and <a href="catalog/README.md">catalog scope</a> are also retained. Recipe links below identify package provenance, not complete linked-dependency or license classifications.</p>',
    '<p>Compatible FFmpeg DLL replacement is not blocked by this material binding: these hashes identify the evaluated candidate and are not an application startup allowlist. Microsoft Visual C++ prerequisites and Windows components are not included here. Installation, supported-Windows lifecycle, final quality, owner acceptance and same-release public delivery remain unverified.</p>',
    '<h2>Evaluated executable and runtime files</h2><p>84 package-matched DLLs, nine rebuilt FFmpeg files and scoped ZVBI. Historical baseline manifests inside the catalog do not replace this list. Runtime import records describe the observed PE closure, not every dynamic load or static dependency.</p>',
    '<table><tr><th>File / bytes</th><th>SHA256</th><th>Materials</th></tr>')
foreach ($binding in $bindings) { $page += '<tr><td>' + (Html $binding.name) + '<br>' + $binding.bytes + '</td><td><code>' + $binding.sha256 + '</code></td><td><a href="' + (Link $binding.material) + '">Originals / guide</a></td></tr>' }
$page += '</table></html>'
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'START-HERE.html'),($page -join "`n") + "`n",$utf8)
$binding = [ordered]@{schema_version=1;distribution_approved=$false;application_source_commit=$manifest.application_source_commit;catalog_inventory=$manifest.catalog_inventory;files=@($bindings)}
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'BINDING.json'),($binding | ConvertTo-Json -Depth 10) + "`n",$utf8)
# Written last so a failed copy cannot be mistaken for completed candidate materials.
Copy-Item -LiteralPath $manifestPath -Destination (Join-Path $OutputDirectory 'INPUTS.json')
Write-Output "Candidate materials: $OutputDirectory"
Write-Output '95 actual binaries verified; source snapshot and exact catalog copied. Open START-HERE.html. No binaries copied or release approved.'
