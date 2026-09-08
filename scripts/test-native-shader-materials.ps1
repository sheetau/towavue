[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$CacheDirectory,
    [string]$PackageDirectory = (Join-Path (Split-Path -Parent $PSScriptRoot) 'vendor/msys2/packages-20260908')
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$CacheDirectory = (Resolve-Path -LiteralPath $CacheDirectory).Path
$PackageDirectory = (Resolve-Path -LiteralPath $PackageDirectory).Path
$inventoryPath = Join-Path $repositoryRoot 'docs/native-shader-inputs.json'
$inventory = Get-Content -LiteralPath $inventoryPath -Raw -Encoding UTF8 | ConvertFrom-Json
$audit = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-runtime-package-audit.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$generator = Join-Path $PSScriptRoot 'prepare-native-shader-materials.ps1'
$testRoot = Join-Path $repositoryRoot ('target/tmp/native-shader-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot | Out-Null
$inputs = @{}
$expected = @{}
foreach ($component in $inventory.components) {
    foreach ($record in @($component.package_archive, $component.package_signature)) {
        $inputs['packages/' + $record.name] = @{Root=$PackageDirectory;Record=$record}
    }
    foreach ($record in @($component.recipe, $component.source) + @($component.patches)) {
        $inputs['cache/' + $record.name] = @{Root=$CacheDirectory;Record=$record}
        $expected[$component.name + '/' + $record.name] = $record.sha256
    }
    foreach ($record in $component.package_documents) { $expected[$component.name + '/package/' + $record.name] = $record.sha256 }
    foreach ($record in $component.selected_documents) { $expected[$component.name + '/source/' + $record.name] = $record.sha256 }
    $parent = $audit.packages | Where-Object name -eq $component.consumer
    $record = if ($parent) {
        @{name=$parent.archive_url.Split('/')[-1];bytes=$parent.archive_bytes;sha256=$parent.archive_sha256}
    } else {
        ($inventory.components | Where-Object package -eq $component.consumer).package_archive
    }
    $inputs['packages/' + $record.name] = @{Root=$PackageDirectory;Record=$record}
}
$expected['INPUTS.json'] = (Get-FileHash -LiteralPath $inventoryPath).Hash
$expected['README.txt'] = (Get-FileHash -LiteralPath (Join-Path $repositoryRoot 'third-party/NATIVE-SHADER-MATERIALS-README.txt')).Hash
function Assert-Output([string]$Directory) {
    if (@(Get-ChildItem -LiteralPath $Directory -Recurse -File -Force).Count -ne $expected.Count) { throw 'Unexpected native shader file count.' }
    foreach ($name in $expected.Keys) {
        if ((Get-FileHash -LiteralPath (Join-Path $Directory $name)).Hash -ne $expected[$name]) { throw "Native shader output differs: $name" }
    }
}
function Assert-Rejected($Arguments, [string]$Message) {
    $rejected = $false
    try { & $generator @Arguments | Out-Null }
    catch { if ($_.Exception.Message -notlike $Message) { throw }; $rejected = $true }
    if (-not $rejected) { throw "Invalid native shader input accepted: $Message" }
}
Push-Location $testRoot
try {
    foreach ($name in @('first', 'repeat')) {
        & $generator -CacheDirectory $CacheDirectory -PackageDirectory $PackageDirectory -OutputDirectory $name
        Assert-Output (Join-Path $testRoot $name)
    }
}
finally { Pop-Location }
$arguments = @{CacheDirectory=$CacheDirectory;PackageDirectory=$PackageDirectory;OutputDirectory=(Join-Path $testRoot 'first')}
Assert-Rejected $arguments 'Use a fresh native shader output directory.'
foreach ($root in @($CacheDirectory, $PackageDirectory)) {
    $arguments.OutputDirectory = Join-Path $root 'rejected-output'
    Assert-Rejected $arguments 'Native shader output overlaps an input directory.'
    if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Overlapping output was created.' }
}
$fixtureRoot = Join-Path $testRoot 'repository'
foreach ($name in @('scripts/prepare-native-shader-materials.ps1', 'docs/native-shader-inputs.json',
    'docs/native-runtime-package-audit.json', 'third-party/NATIVE-SHADER-MATERIALS-README.txt')) {
    $path = Join-Path $fixtureRoot $name
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repositoryRoot $name) -Destination $path
}
foreach ($name in $inputs.Keys) {
    $path = Join-Path $testRoot $name
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $inputs[$name].Root $inputs[$name].Record.name) -Destination $path
}
$generator = Join-Path $fixtureRoot 'scripts/prepare-native-shader-materials.ps1'
$rejectedOutput = Join-Path $testRoot 'rejected'
$arguments = @{CacheDirectory=(Join-Path $testRoot 'cache');PackageDirectory=(Join-Path $testRoot 'packages');OutputDirectory=$rejectedOutput}
foreach ($name in $inputs.Keys) {
    $path = Join-Path $testRoot $name
    Move-Item -LiteralPath $path -Destination ($path + '.saved')
    try { Assert-Rejected $arguments 'Missing native shader input:*' }
    finally { Move-Item -LiteralPath ($path + '.saved') -Destination $path }
    # Mutate only an isolated test fixture; preserve its original byte afterwards.
    $stream = [IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite)
    try { $original = $stream.ReadByte(); $stream.Position = 0; $stream.WriteByte($original -bxor 1) }
    finally { $stream.Dispose() }
    $corruptHash = (Get-FileHash -LiteralPath $path).Hash
    try {
        Assert-Rejected $arguments 'Native shader checksum mismatch:*'
        if ((Get-FileHash -LiteralPath $path).Hash -ne $corruptHash) { throw 'Corrupt shader input was overwritten.' }
    }
    finally {
        $stream = [IO.File]::OpenWrite($path)
        try { $stream.WriteByte($original) }
        finally { $stream.Dispose() }
    }
    if (Test-Path -LiteralPath $rejectedOutput) { throw 'Rejected shader input created output.' }
}
$manifestPath = Join-Path $fixtureRoot 'docs/native-shader-inputs.json'
$manifestBytes = [IO.File]::ReadAllBytes($manifestPath)
$encoding = [Text.UTF8Encoding]::new($false)
foreach ($kind in @('consumer', 'dependency', 'nested', 'historical_header', 'package', 'recipe', 'duplicate', 'path')) {
    $changed = $encoding.GetString($manifestBytes) | ConvertFrom-Json
    switch ($kind) {
        'consumer' { $changed.components[0].consumer = 'unrelated'; $message = 'Stale native shader consumer mapping.' }
        'dependency' { $changed.components[0].version = 'unrelated'; $message = 'Stale native shader dependency mapping.' }
        'nested' { ($changed.components | Where-Object name -eq 'spirv-tools-glslang-build').consumer = 'mingw-w64-x86_64-shaderc'; $message = 'Stale native shader dependency mapping.' }
        'historical_header' { ($changed.components | Where-Object name -eq 'vulkan-headers-350').consumer = 'mingw-w64-x86_64-vulkan-loader'; $message = 'Stale native shader dependency mapping.' }
        'package' { $changed.components[0].package_archive = $changed.components[1].package_archive; $message = 'Stale native shader package mapping.' }
        'recipe' { $changed.components[0].source = $changed.components[1].source; $message = 'Stale native shader recipe mapping.' }
        'duplicate' { $changed.components[1].name = $changed.components[0].name; $message = 'Duplicate native shader component.' }
        'path' { $changed.components[0].selected_documents[0].name = '../outside'; $message = 'Invalid native shader material path.' }
    }
    [IO.File]::WriteAllText($manifestPath, ($changed | ConvertTo-Json -Depth 20), $encoding)
    try { Assert-Rejected $arguments $message }
    finally { [IO.File]::WriteAllBytes($manifestPath, $manifestBytes) }
    if (Test-Path -LiteralPath $rejectedOutput) { throw 'Invalid shader mapping created output.' }
}
foreach ($kind in @('package_documents', 'selected_documents')) {
    $changed = $encoding.GetString($manifestBytes) | ConvertFrom-Json
    $changed.components[0].$kind[0].sha256 = '0' * 64
    [IO.File]::WriteAllText($manifestPath, ($changed | ConvertTo-Json -Depth 20), $encoding)
    $arguments.OutputDirectory = Join-Path $testRoot ('partial-' + $kind)
    try { Assert-Rejected $arguments 'Native shader checksum mismatch:*' }
    finally { [IO.File]::WriteAllBytes($manifestPath, $manifestBytes) }
    if (-not (Test-Path -LiteralPath $arguments.OutputDirectory) -or
        (Test-Path -LiteralPath (Join-Path $arguments.OutputDirectory 'INPUTS.json'))) { throw 'Failed shader extraction has incorrect completion state.' }
}
foreach ($name in $inputs.Keys) {
    foreach ($path in @((Join-Path $testRoot $name), (Join-Path $inputs[$name].Root $inputs[$name].Record.name))) {
        if ((Get-FileHash -LiteralPath $path).Hash -ne $inputs[$name].Record.sha256) { throw 'Original shader input changed.' }
    }
}
Assert-Output (Join-Path $testRoot 'first')
Write-Output "Native shader checks passed: $($expected.Count) exact files, arbitrary cwd/repeat, $($inputs.Count) missing/corrupt pairs, eight mapping failures, two incomplete-extraction marker cases and input/output preservation. Evidence: $testRoot"
