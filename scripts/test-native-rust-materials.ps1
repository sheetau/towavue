[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$SourceSupplementDirectory,
    [Parameter(Mandatory = $true)][string]$CrateCacheDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$SourceSupplementDirectory = (Resolve-Path -LiteralPath $SourceSupplementDirectory).Path
$CrateCacheDirectory = (Resolve-Path -LiteralPath $CrateCacheDirectory).Path
$inventoryPath = Join-Path $repositoryRoot 'docs/native-rust-dependencies.json'
$inventory = Get-Content -LiteralPath $inventoryPath -Raw -Encoding UTF8 | ConvertFrom-Json
$generator = Join-Path $PSScriptRoot 'prepare-native-rust-materials.ps1'
$testRoot = Join-Path $repositoryRoot ('target/tmp/native-rust-materials-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot | Out-Null
$expected = @{}
foreach ($component in $inventory.components) { $expected[$component.name + '/Cargo.lock'] = $component.cargo_lock.sha256 }
foreach ($crate in $inventory.crates) {
    $expected[$crate.component + '/' + $crate.archive.name] = $crate.archive.sha256
    foreach ($document in $crate.selected_documents) { $expected[$crate.component + '/' + $document.name] = $document.sha256 }
}
foreach ($notice in $inventory.additional_notices) { $expected['upstream-notices/' + $notice.name] = $notice.sha256 }
$expected['INPUTS.json'] = (Get-FileHash -LiteralPath $inventoryPath).Hash
$expected['README.txt'] = (Get-FileHash -LiteralPath (Join-Path $repositoryRoot 'third-party/NATIVE-RUST-MATERIALS-README.txt')).Hash

function Assert-Output([string]$Directory) {
    if (@(Get-ChildItem -LiteralPath $Directory -Recurse -File).Count -ne $expected.Count) { throw 'Unexpected native Rust output count.' }
    foreach ($relative in $expected.Keys) {
        if ((Get-FileHash -LiteralPath (Join-Path $Directory $relative)).Hash -ne $expected[$relative]) { throw "Native Rust output differs: $relative" }
    }
}
function Assert-Rejected($Arguments, [string]$Message) {
    $rejected = $false
    try { & $generator @Arguments | Out-Null }
    catch { if ($_.Exception.Message -notlike $Message) { throw }; $rejected = $true }
    if (-not $rejected) { throw "Native Rust invalid input accepted: $Message" }
}

Push-Location $testRoot
try {
    foreach ($name in @('first', 'repeat')) {
        & $generator -SourceSupplementDirectory $SourceSupplementDirectory -CrateCacheDirectory $CrateCacheDirectory -OutputDirectory $name
        Assert-Output (Join-Path $testRoot $name)
    }
}
finally { Pop-Location }
$arguments = @{ SourceSupplementDirectory=$SourceSupplementDirectory; CrateCacheDirectory=$CrateCacheDirectory; OutputDirectory=(Join-Path $testRoot 'first') }
Assert-Rejected $arguments 'Use a fresh native Rust material output directory.'
$arguments.OutputDirectory = Join-Path $CrateCacheDirectory 'rejected-output'
Assert-Rejected $arguments 'Native Rust output overlaps an input directory.'
if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Overlapping output was created.' }

$fixtureRoot = Join-Path $testRoot 'repository'
$paths = @()
foreach ($name in (@('scripts/prepare-native-rust-materials.ps1','docs/native-rust-dependencies.json','docs/native-runtime-package-audit.json',
    'docs/native-source-supplements.json','third-party/NATIVE-RUST-MATERIALS-README.txt') + @($inventory.additional_notices.repository_path))) {
    $path = Join-Path $fixtureRoot $name
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repositoryRoot $name) -Destination $path
    if ($name -in $inventory.additional_notices.repository_path) { $paths += $path }
}
$fixtureSources = Join-Path $testRoot 'sources'
$fixtureCrates = Join-Path $testRoot 'crates'
New-Item -ItemType Directory -Path $fixtureCrates | Out-Null
foreach ($component in $inventory.components) {
    foreach ($name in @($component.source_archive.name, $component.cargo_lock.name)) {
        $relative = $component.package + '/' + $name
        $path = Join-Path $fixtureSources $relative
        New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $SourceSupplementDirectory $relative) -Destination $path
        $paths += $path
    }
}
foreach ($crate in $inventory.crates) {
    $path = Join-Path $fixtureCrates $crate.archive.name
    Copy-Item -LiteralPath (Join-Path $CrateCacheDirectory $crate.archive.name) -Destination $path
    $paths += $path
}
$generator = Join-Path $fixtureRoot 'scripts/prepare-native-rust-materials.ps1'
$rejectedOutput = Join-Path $testRoot 'rejected'
$arguments = @{ SourceSupplementDirectory=$fixtureSources; CrateCacheDirectory=$fixtureCrates; OutputDirectory=$rejectedOutput }
foreach ($path in $paths) {
    $original = [IO.File]::ReadAllBytes($path)
    Move-Item -LiteralPath $path -Destination ($path + '.saved')
    try { Assert-Rejected $arguments 'Missing native Rust input:*' }
    finally { Move-Item -LiteralPath ($path + '.saved') -Destination $path }
    $corrupt = [byte[]]$original.Clone()
    $corrupt[0] = $corrupt[0] -bxor 1
    [IO.File]::WriteAllBytes($path, $corrupt)
    $corruptHash = (Get-FileHash -LiteralPath $path).Hash
    try {
        Assert-Rejected $arguments 'Native Rust input checksum mismatch:*'
        if ((Get-FileHash -LiteralPath $path).Hash -ne $corruptHash) { throw 'Corrupt native Rust input was overwritten.' }
    }
    finally { [IO.File]::WriteAllBytes($path, $original) }
    if (Test-Path -LiteralPath $rejectedOutput) { throw 'Rejected native Rust input created output.' }
}
$manifestPath = Join-Path $fixtureRoot 'docs/native-rust-dependencies.json'
$manifestBytes = [IO.File]::ReadAllBytes($manifestPath)
$encoding = [Text.UTF8Encoding]::new($false)
foreach ($kind in @('component','source','source-lock','duplicate','lock','path')) {
    $changed = $encoding.GetString($manifestBytes) | ConvertFrom-Json
    switch ($kind) {
        'component' { $changed.components[0].runtime.sha256 = '0' * 64; $message = 'Stale native Rust component mapping.' }
        'source' { $changed.components[0].source_archive.sha256 = '0' * 64; $message = 'Stale native Rust source mapping.' }
        'source-lock' { $changed.components[0].cargo_lock.sha256 = '0' * 64; $message = 'Stale native Rust source mapping.' }
        'duplicate' { $changed.crates[1] = $changed.crates[0]; $message = 'Stale native Rust crate/lock mapping.' }
        'lock' { $changed.crates[0].archive.sha256 = '0' * 64; $message = 'Stale native Rust crate/lock mapping.' }
        'path' { $changed.crates[0].selected_documents[0].name = '../outside'; $message = 'Invalid native Rust document path.' }
    }
    [IO.File]::WriteAllText($manifestPath, ($changed | ConvertTo-Json -Depth 20), $encoding)
    try { Assert-Rejected $arguments $message }
    finally { [IO.File]::WriteAllBytes($manifestPath, $manifestBytes) }
    if (Test-Path -LiteralPath $rejectedOutput) { throw 'Invalid native Rust mapping created output.' }
}
Assert-Output (Join-Path $testRoot 'first')
Write-Output "Native Rust material checks passed: $($expected.Count) exact files, arbitrary cwd/repeat, $($paths.Count) missing/corrupt input pairs, six mapping failures and input/output preservation."
