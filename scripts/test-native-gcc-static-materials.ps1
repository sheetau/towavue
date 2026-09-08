[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$CacheDirectory)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$CacheDirectory = (Resolve-Path -LiteralPath $CacheDirectory).Path
$inventoryPath = Join-Path $repositoryRoot 'docs/native-gcc-static-inputs.json'
$inventory = Get-Content -LiteralPath $inventoryPath -Raw -Encoding UTF8 | ConvertFrom-Json
$generator = Join-Path $PSScriptRoot 'prepare-native-gcc-static-materials.ps1'
$testRoot = Join-Path $repositoryRoot ('target/tmp/native-gcc-static-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot | Out-Null
$expected = @{}
foreach ($archive in $inventory.archives) {
    foreach ($document in $archive.documents) { $expected[$archive.output + '/' + $document.name] = $document.sha256 }
}
foreach ($inputRecord in $inventory.inputs | Where-Object output) { $expected[$inputRecord.output] = $inputRecord.sha256 }
$expected['INPUTS.json'] = (Get-FileHash -LiteralPath $inventoryPath).Hash
$expected['README.txt'] = (Get-FileHash -LiteralPath (Join-Path $repositoryRoot 'third-party/NATIVE-GCC-STATIC-README.txt')).Hash
function Assert-Output([string]$Directory) {
    if (@(Get-ChildItem -LiteralPath $Directory -Recurse -File).Count -ne $expected.Count) { throw 'Unexpected native GCC static file count.' }
    foreach ($name in $expected.Keys) {
        if ((Get-FileHash -LiteralPath (Join-Path $Directory $name)).Hash -ne $expected[$name]) { throw "Native GCC static output differs: $name" }
    }
}
function Assert-Rejected($Arguments, [string]$Message) {
    $rejected = $false
    try { & $generator @Arguments | Out-Null }
    catch { if ($_.Exception.Message -notlike $Message) { throw }; $rejected = $true }
    if (-not $rejected) { throw "Invalid native GCC static input accepted: $Message" }
}
Push-Location $testRoot
try {
    foreach ($name in @('first','repeat')) {
        & $generator -CacheDirectory $CacheDirectory -OutputDirectory $name
        Assert-Output (Join-Path $testRoot $name)
    }
}
finally { Pop-Location }
$arguments = @{CacheDirectory=$CacheDirectory;OutputDirectory=(Join-Path $testRoot 'first')}
Assert-Rejected $arguments 'Use a fresh native GCC static output directory.'
$arguments.OutputDirectory = Join-Path $CacheDirectory 'rejected-output'
Assert-Rejected $arguments 'Native GCC static output overlaps its cache.'
if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Overlapping output was created.' }

$fixtureRoot = Join-Path $testRoot 'repository'
foreach ($name in @('scripts/prepare-native-gcc-static-materials.ps1','docs/native-gcc-static-inputs.json',
    'docs/native-runtime-package-audit.json','third-party/NATIVE-GCC-STATIC-README.txt')) {
    $path = Join-Path $fixtureRoot $name
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repositoryRoot $name) -Destination $path
}
$fixtureCache = Join-Path $testRoot 'cache'
foreach ($record in $inventory.inputs) {
    $path = Join-Path $fixtureCache $record.name
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $CacheDirectory $record.name) -Destination $path
}
$generator = Join-Path $fixtureRoot 'scripts/prepare-native-gcc-static-materials.ps1'
$rejectedOutput = Join-Path $testRoot 'rejected'
$arguments = @{CacheDirectory=$fixtureCache;OutputDirectory=$rejectedOutput}
foreach ($record in $inventory.inputs) {
    $path = Join-Path $fixtureCache $record.name
    Move-Item -LiteralPath $path -Destination ($path + '.saved')
    try { Assert-Rejected $arguments 'Missing native GCC static input:*' }
    finally { Move-Item -LiteralPath ($path + '.saved') -Destination $path }
    # Change one byte in the isolated fixture without duplicating large archives in memory.
    $stream = [IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite)
    try { $original = $stream.ReadByte(); $stream.Position = 0; $stream.WriteByte($original -bxor 1) }
    finally { $stream.Dispose() }
    $corruptHash = (Get-FileHash -LiteralPath $path).Hash
    try {
        Assert-Rejected $arguments 'Native GCC static checksum mismatch:*'
        if ((Get-FileHash -LiteralPath $path).Hash -ne $corruptHash) { throw 'Corrupt runtime input was overwritten.' }
    }
    finally {
        $stream = [IO.File]::OpenWrite($path)
        try { $stream.WriteByte($original) }
        finally { $stream.Dispose() }
    }
    if (Test-Path -LiteralPath $rejectedOutput) { throw 'Rejected runtime input created output.' }
}
$manifestPath = Join-Path $fixtureRoot 'docs/native-gcc-static-inputs.json'
$manifestBytes = [IO.File]::ReadAllBytes($manifestPath)
$encoding = [Text.UTF8Encoding]::new($false)
foreach ($kind in @('consumer','recipe','package','dependency','duplicate','path','archive','header')) {
    $changed = $encoding.GetString($manifestBytes) | ConvertFrom-Json
    switch ($kind) {
        'consumer' { $changed.consumer.package = 'unrelated'; $message = 'Stale native GCC static consumer mapping.' }
        'recipe' { $changed.source = 'gcc-16.1.0.tar.xz.sig'; $message = 'Stale native GCC static recipe mapping.' }
        'package' { $changed.archives[0].input = $changed.archives[1].input; $message = 'Stale native GCC static package mapping.' }
        'dependency' { $changed.version = 'unrelated'; $message = 'Stale native GCC static dependency mapping.' }
        'duplicate' { $changed.inputs[1] = $changed.inputs[0]; $message = 'Duplicate native GCC static input.' }
        'path' { $changed.archives[0].documents[0].name = '../outside'; $message = 'Invalid native GCC static material path.' }
        'archive' { $changed.archives[0].input = 'unrelated'; $message = 'Stale native GCC static archive mapping.' }
        'header' { $changed.matching_headers[0].source_member = 'unrelated'; $message = 'Stale native GCC static header mapping.' }
    }
    [IO.File]::WriteAllText($manifestPath, ($changed | ConvertTo-Json -Depth 20), $encoding)
    try { Assert-Rejected $arguments $message }
    finally { [IO.File]::WriteAllBytes($manifestPath, $manifestBytes) }
    if (Test-Path -LiteralPath $rejectedOutput) { throw 'Invalid GCC static mapping created output.' }
}
$changed = $encoding.GetString($manifestBytes) | ConvertFrom-Json
$changed.archives[0].documents[0].sha256 = '0' * 64
[IO.File]::WriteAllText($manifestPath, ($changed | ConvertTo-Json -Depth 20), $encoding)
$arguments.OutputDirectory = Join-Path $testRoot 'partial-extraction'
try { Assert-Rejected $arguments 'Native GCC static checksum mismatch:*' }
finally { [IO.File]::WriteAllBytes($manifestPath, $manifestBytes) }
if (-not (Test-Path -LiteralPath $arguments.OutputDirectory) -or
    (Test-Path -LiteralPath (Join-Path $arguments.OutputDirectory 'INPUTS.json'))) { throw 'Failed extraction has incorrect completion state.' }
foreach ($record in $inventory.inputs) {
    foreach ($root in @($CacheDirectory,$fixtureCache)) {
        if ((Get-FileHash -LiteralPath (Join-Path $root $record.name)).Hash -ne $record.sha256) { throw 'Original runtime input changed.' }
    }
}
Assert-Output (Join-Path $testRoot 'first')
Write-Output "Native GCC static checks passed: $($expected.Count) exact files, arbitrary cwd/repeat, 22 missing/corrupt pairs, eight mapping failures, incomplete-extraction marker protection and input/output preservation."
