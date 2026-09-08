[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [Parameter(Mandatory = $true)][string]$RuntimeDirectory,
    [Parameter(Mandatory = $true)][string]$CatalogDirectory,
    [Parameter(Mandatory = $true)][string]$ApplicationSource
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
foreach ($name in @('Executable','RuntimeDirectory','CatalogDirectory','ApplicationSource')) {
    Set-Variable -Name $name -Value ((Resolve-Path -LiteralPath (Get-Variable -Name $name -ValueOnly)).Path)
}
$testRoot = Join-Path $repositoryRoot ('target/tmp/candidate-material-test-' + [guid]::NewGuid().ToString('N'))
$fixtureRoot = Join-Path $testRoot 'repository'
foreach ($name in @('scripts/prepare-candidate-materials.ps1','docs/candidate-material-inputs.json')) {
    $path = Join-Path $fixtureRoot $name
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repositoryRoot $name) -Destination $path
}
$generator = Join-Path $fixtureRoot 'scripts/prepare-candidate-materials.ps1'
$manifestPath = Join-Path $fixtureRoot 'docs/candidate-material-inputs.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$arguments = @{Executable=$Executable;RuntimeDirectory=$RuntimeDirectory;CatalogDirectory=$CatalogDirectory;ApplicationSource=$ApplicationSource}
function Get-Hashes([string]$Root) {
    $result = @{}
    foreach ($file in Get-ChildItem -LiteralPath $Root -Recurse -File -Force) { $result[$file.FullName.Substring($Root.Length + 1).Replace('\','/')] = (Get-FileHash -LiteralPath $file.FullName).Hash }
    return $result
}
$catalogHashes = Get-Hashes $CatalogDirectory
$runtimeHashes = Get-Hashes $RuntimeDirectory
$exeHash = (Get-FileHash -LiteralPath $Executable).Hash
$sourceHash = (Get-FileHash -LiteralPath $ApplicationSource).Hash
$sourceTree = @{}
foreach ($line in (& git -C $repositoryRoot ls-tree -r $manifest.application_source_commit)) {
    if ($line -notmatch '^100644 blob ([0-9a-f]{40})\t(.+)$') { throw 'Unexpected application source tree member.' }
    $sourceTree['towavue/' + $Matches[2]] = $Matches[1]
}
if ($LASTEXITCODE -ne 0 -or $sourceTree.Count -ne 224) { throw 'Application source commit unavailable or incomplete.' }
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead($ApplicationSource)
$sha1 = [Security.Cryptography.SHA1]::Create()
try {
    $members = @($archive.Entries | Where-Object { -not $_.FullName.EndsWith('/') })
    if ($members.Count -ne $sourceTree.Count) { throw 'Application source ZIP coverage differs from Git.' }
    foreach ($entry in $members) {
        $stream = $entry.Open()
        $buffer = [IO.MemoryStream]::new()
        try { $stream.CopyTo($buffer); $bytes = $buffer.ToArray() }
        finally { $stream.Dispose(); $buffer.Dispose() }
        $blob = [Text.Encoding]::ASCII.GetBytes('blob ' + $bytes.Length + [char]0) + $bytes
        $hash = [BitConverter]::ToString($sha1.ComputeHash($blob)).Replace('-','').ToLowerInvariant()
        if (-not $sourceTree.ContainsKey($entry.FullName) -or $sourceTree[$entry.FullName] -ne $hash) { throw 'Application source ZIP differs from committed blob.' }
        $sourceTree.Remove($entry.FullName)
    }
    if ($sourceTree.Count) { throw 'Application source ZIP omitted committed files.' }
} finally { $archive.Dispose(); $sha1.Dispose() }
function Assert-Output([string]$Directory) {
    $hashes = Get-Hashes $Directory
    if ($hashes.Count -ne 2879) { throw 'Unexpected candidate material output count.' }
    foreach ($name in $catalogHashes.Keys) { if ($hashes['catalog/' + $name] -ne $catalogHashes[$name]) { throw 'Candidate catalog copy changed.' } }
    if ($hashes[$manifest.application_source.name] -ne $sourceHash) { throw 'Application source copy changed.' }
    if (@($hashes.Keys | Where-Object { $_ -match '\.(exe|dll|lib|a)$' -and $_ -notmatch '/COPYING3?\.LIB$' }).Count) { throw 'Candidate binary copied into source materials.' }
    $binding = Get-Content -LiteralPath (Join-Path $Directory 'BINDING.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($binding.files.Count -ne 95 -or $binding.distribution_approved -ne $false -or
        $binding.application_source_commit -ne $manifest.application_source_commit) { throw 'Incorrect candidate binding scope.' }
    $names = @{}
    foreach ($file in $binding.files) {
        if ($names.ContainsKey($file.name)) { throw 'Duplicate candidate binding.' }
        $names[$file.name] = $true
        $expected = if ($file.name -eq 'towavue.exe') { $exeHash } else { $runtimeHashes[$file.name] }
        if (-not $expected -or $file.sha256 -ne $expected) { throw 'Candidate binding differs from actual binary.' }
        if (-not (Test-Path -LiteralPath (Join-Path $Directory $file.material) -PathType Leaf)) { throw 'Broken candidate binding link.' }
        if ($file.material -like '*packages/mingw-w64-x86_64-zvbi/*') { throw 'Old ZVBI binding returned.' }
    }
    $html = Get-Content -LiteralPath (Join-Path $Directory 'START-HERE.html') -Raw -Encoding UTF8
    $links = [regex]::Matches($html,'href="([^"]+)"')
    if ($links.Count -ne 113 -or [regex]::Matches($html,'<tr>').Count -ne 96) { throw 'Incomplete candidate HTML guide.' }
    foreach ($link in $links) {
        $relative = [uri]::UnescapeDataString($link.Groups[1].Value)
        if ($relative -match '(^/|:|\\|\.\.)' -or -not (Test-Path -LiteralPath (Join-Path $Directory $relative) -PathType Leaf)) { throw 'Broken or nonlocal candidate HTML link.' }
    }
    if ($html -match '[A-Za-z]:[/\\]' -or (Get-Content -LiteralPath (Join-Path $Directory 'BINDING.json') -Raw -Encoding UTF8) -match '[A-Za-z]:[/\\]') { throw 'Machine path in candidate entry.' }
}
function Assert-Rejected([string]$Message) {
    $rejected = $false
    try { & $generator @arguments | Out-Null }
    catch { if ($_.Exception.Message -notlike $Message) { throw }; $rejected = $true }
    if (-not $rejected) { throw "Invalid candidate input accepted: $Message" }
}
Push-Location $testRoot
try {
    foreach ($name in @('first','repeat')) {
        & $generator @arguments -OutputDirectory (Join-Path $testRoot $name) | Out-Null
        Assert-Output (Join-Path $testRoot $name)
    }
} finally { Pop-Location }
$firstHashes = Get-Hashes (Join-Path $testRoot 'first')
$repeatHashes = Get-Hashes (Join-Path $testRoot 'repeat')
foreach ($name in $firstHashes.Keys) { if ($firstHashes[$name] -ne $repeatHashes[$name]) { throw 'Candidate output is not repeatable.' } }
$arguments.OutputDirectory = Join-Path $testRoot 'first'
Assert-Rejected 'Use a fresh candidate material output directory.'
foreach ($root in @($RuntimeDirectory,$CatalogDirectory)) {
    $arguments.OutputDirectory = Join-Path $root 'nested-output'
    Assert-Rejected 'Candidate material output overlaps an input.'
}
# Fault injection touches fixture copies only, never evaluated binaries or the original catalog.
Copy-Item -LiteralPath $RuntimeDirectory -Destination (Join-Path $testRoot 'runtime') -Recurse
Copy-Item -LiteralPath $CatalogDirectory -Destination (Join-Path $testRoot 'catalog') -Recurse
Copy-Item -LiteralPath $Executable -Destination (Join-Path $testRoot 'towavue.exe')
Copy-Item -LiteralPath $ApplicationSource -Destination (Join-Path $testRoot $manifest.application_source.name)
$arguments.RuntimeDirectory = Join-Path $testRoot 'runtime'
$arguments.CatalogDirectory = Join-Path $testRoot 'catalog'
$arguments.Executable = Join-Path $testRoot 'towavue.exe'
$arguments.ApplicationSource = Join-Path $testRoot $manifest.application_source.name
$arguments.OutputDirectory = Join-Path $testRoot 'rejected'
$paths = @($arguments.Executable,$arguments.ApplicationSource)
$paths += @($runtimeHashes.Keys | ForEach-Object { Join-Path $arguments.RuntimeDirectory $_ })
$paths += @('FILES.json','CATALOG.json','materials/app-materials-v2/INPUTS.json','materials/native-ffmpeg-materials-v1/RUNTIME.json','native-runtime-package-audit.json','materials/native-data-materials-v1/UNICODE-LICENSE.txt') | ForEach-Object { Join-Path $arguments.CatalogDirectory $_ }
$utf8 = [Text.UTF8Encoding]::new($false)
$pairs = 0
foreach ($path in $paths) {
    $bytes = [IO.File]::ReadAllBytes($path)
    Move-Item -LiteralPath $path -Destination ($path + '.saved')
    try { Assert-Rejected '*' }
    finally { Move-Item -LiteralPath ($path + '.saved') -Destination $path }
    $changed = [byte[]]$bytes.Clone()
    $changed[0] = $changed[0] -bxor 1
    [IO.File]::WriteAllBytes($path,$changed)
    $changedHash = (Get-FileHash -LiteralPath $path).Hash
    try {
        Assert-Rejected 'Candidate material checksum mismatch:*'
        if ((Get-FileHash -LiteralPath $path).Hash -ne $changedHash) { throw 'Corrupt candidate input was repaired.' }
    } finally { [IO.File]::WriteAllBytes($path,$bytes) }
    if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Failed candidate preflight created output.' }
    $pairs++
    if ($pairs % 20 -eq 0) { Write-Output "Candidate missing/corrupt input pairs checked: $pairs" }
}
foreach ($root in @($arguments.RuntimeDirectory,$arguments.CatalogDirectory)) {
    $extra = Join-Path $root 'unexpected.txt'
    [IO.File]::WriteAllText($extra,'fixture',$utf8)
    try { Assert-Rejected 'Candidate * coverage mismatch.' }
    finally { Move-Item -LiteralPath $extra -Destination (Join-Path $testRoot ([guid]::NewGuid().ToString('N') + '.saved')) }
}
$original = [IO.File]::ReadAllBytes($manifestPath)
$changed = $utf8.GetString($original) | ConvertFrom-Json
$changed.application_source_commit = '0' * 40
[IO.File]::WriteAllText($manifestPath,($changed | ConvertTo-Json -Depth 6),$utf8)
try { Assert-Rejected 'Application source commit differs from archive.' }
finally { [IO.File]::WriteAllBytes($manifestPath,$original) }
$catalogPath = $arguments.CatalogDirectory
$link = Join-Path $testRoot 'linked-catalog'
New-Item -ItemType Junction -Path $link -Target $catalogPath | Out-Null
try { $arguments.CatalogDirectory = $link; Assert-Rejected 'Candidate material links are not allowed.' }
finally { $arguments.CatalogDirectory = $catalogPath; [IO.Directory]::Delete($link) }
$generatorBytes = [IO.File]::ReadAllBytes($generator)
$generatorText = $utf8.GetString($generatorBytes)
$copyLine = 'Copy-Item -LiteralPath $ApplicationSource -Destination $sourcePath'
if (-not $generatorText.Contains($copyLine)) { throw 'Missing candidate copy-failure fixture target.' }
[IO.File]::WriteAllText($generator,$generatorText.Replace($copyLine,'throw "Injected candidate copy failure."'),$utf8)
$arguments.OutputDirectory = Join-Path $testRoot 'partial'
try { Assert-Rejected 'Injected candidate copy failure.' }
finally { [IO.File]::WriteAllBytes($generator,$generatorBytes) }
if (-not (Test-Path -LiteralPath $arguments.OutputDirectory) -or (Test-Path -LiteralPath (Join-Path $arguments.OutputDirectory 'INPUTS.json'))) { throw 'Invalid failed-copy completion state.' }
Assert-Output (Join-Path $testRoot 'first')
foreach ($pair in @(@($CatalogDirectory,$catalogHashes),@($RuntimeDirectory,$runtimeHashes))) {
    $actual = Get-Hashes $pair[0]
    if ($actual.Count -ne $pair[1].Count) { throw 'Original candidate input coverage changed.' }
    foreach ($name in $actual.Keys) { if ($actual[$name] -ne $pair[1][$name]) { throw 'Original candidate input changed.' } }
}
if ((Get-FileHash -LiteralPath $Executable).Hash -ne $exeHash -or (Get-FileHash -LiteralPath $ApplicationSource).Hash -ne $sourceHash) { throw 'Original candidate binary/source changed.' }
Write-Output "Candidate material checks passed: 2879 exact files, 95 binary bindings, 113 local HTML links, repeat/cwd, $pairs missing/corrupt input pairs, extra files, stale source commit, overlap/junction, partial-copy marker and original preservation. Evidence: $testRoot"
