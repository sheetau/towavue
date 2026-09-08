[CmdletBinding()]
param([string]$CacheDirectory, [string]$RecipeDirectory)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$generator = Join-Path $PSScriptRoot 'prepare-native-source-supplements.ps1'
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-source-supplements.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$recipes = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-runtime-recipes.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if (-not $CacheDirectory) { $CacheDirectory = Join-Path $repositoryRoot 'vendor/msys2/source-supplements-20260908' }
if (-not $RecipeDirectory) { $RecipeDirectory = Join-Path $repositoryRoot 'vendor/msys2/runtime-recipes-20260908' }
$CacheDirectory = (Resolve-Path -LiteralPath $CacheDirectory).Path
$RecipeDirectory = (Resolve-Path -LiteralPath $RecipeDirectory).Path
$testDirectory = Join-Path $repositoryRoot ('target/tmp/source-supplements-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory | Out-Null
$expected = @{}
$timestamps = @{}
foreach ($package in $inventory.packages) {
    foreach ($file in $package.files) {
        $relative = $package.package + '/' + $file.name
        $expected[$relative] = $file.sha256
        $timestamps[$relative] = (Get-Item -LiteralPath (Join-Path $CacheDirectory $relative)).LastWriteTimeUtc
    }
    foreach ($document in $package.selected_documents) { $expected[$package.package + '/' + $document.name] = $document.sha256 }
    $expected[$package.package + '/PKGBUILD'] = $package.recipe_sha256
}
foreach ($name in @('native-runtime-recipes.json', 'native-runtime-package-audit.json')) {
    $expected[$name] = (Get-FileHash -LiteralPath (Join-Path $repositoryRoot "docs/$name")).Hash
}
$expected['INPUTS.json'] = (Get-FileHash -LiteralPath (Join-Path $repositoryRoot 'docs/native-source-supplements.json')).Hash
$expected['README.txt'] = (Get-FileHash -LiteralPath (Join-Path $repositoryRoot 'third-party/NATIVE-SOURCE-SUPPLEMENTS-README.txt')).Hash

Push-Location $testDirectory
try {
    foreach ($output in @('first', 'repeat')) {
        & $generator -OutputDirectory $output -CacheDirectory $CacheDirectory -RecipeDirectory $RecipeDirectory
        if (@(Get-ChildItem -LiteralPath $output -Recurse -File).Count -ne $expected.Count) { throw 'Unexpected source supplement file count.' }
        foreach ($relative in $expected.Keys) {
            if ((Get-FileHash -LiteralPath (Join-Path $output $relative)).Hash -ne $expected[$relative]) { throw "Supplement bytes changed: $relative" }
        }
    }
}
finally { Pop-Location }
foreach ($relative in $timestamps.Keys) {
    if ((Get-Item -LiteralPath (Join-Path $CacheDirectory $relative)).LastWriteTimeUtc -ne $timestamps[$relative]) { throw 'Source cache was rewritten.' }
}

function Assert-Rejected($Arguments, [string]$ExpectedMessage) {
    $rejected = $false
    try { & $generator @Arguments | Out-Null }
    catch {
        if ($_.Exception.Message -notlike $ExpectedMessage) { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw "Invalid supplement input accepted: $ExpectedMessage" }
}
$output = Join-Path $testDirectory 'first'
Assert-Rejected @{ OutputDirectory = $output; CacheDirectory = $CacheDirectory; RecipeDirectory = $RecipeDirectory } 'Use a fresh source supplement output directory*'
$missing = Join-Path $testDirectory 'missing-cache'
$rejectedOutput = Join-Path $testDirectory 'rejected'
Assert-Rejected @{ OutputDirectory = $rejectedOutput; CacheDirectory = $missing; RecipeDirectory = $RecipeDirectory } 'Missing source supplement input:*'
if (Test-Path -LiteralPath $missing) { throw 'Offline missing cache was created.' }
foreach ($arguments in @(
    @{ OutputDirectory = (Join-Path $CacheDirectory 'rejected-output'); CacheDirectory = $CacheDirectory; RecipeDirectory = $RecipeDirectory },
    @{ OutputDirectory = $missing; CacheDirectory = (Join-Path $missing 'nested-cache'); RecipeDirectory = $RecipeDirectory }
)) {
    Assert-Rejected $arguments 'Source supplement output overlaps an input directory.'
    if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Overlapping output was created.' }
}

$fixtureCache = Join-Path $testDirectory 'inputs'
$fixtureRecipes = Join-Path $testDirectory 'recipes'
New-Item -ItemType Directory -Path $fixtureCache, $fixtureRecipes | Out-Null
$paths = @()
foreach ($package in $inventory.packages) {
    $directory = Join-Path $fixtureCache $package.package
    New-Item -ItemType Directory -Path $directory | Out-Null
    foreach ($file in $package.files) {
        $path = Join-Path $directory $file.name
        Copy-Item -LiteralPath (Join-Path (Join-Path $CacheDirectory $package.package) $file.name) -Destination $path
        $paths += $path
    }
    $recipe = $recipes.recipes | Where-Object { $_.package -eq $package.package }
    $path = Join-Path $fixtureRecipes $recipe.name
    Copy-Item -LiteralPath (Join-Path $RecipeDirectory $recipe.name) -Destination $path
    $paths += $path
}
$arguments = @{ OutputDirectory = $rejectedOutput; CacheDirectory = $fixtureCache; RecipeDirectory = $fixtureRecipes }
foreach ($path in $paths) {
    $original = [IO.File]::ReadAllBytes($path)
    $backup = $path + '.saved'
    Move-Item -LiteralPath $path -Destination $backup
    try { Assert-Rejected $arguments 'Missing source supplement input:*' }
    finally { Move-Item -LiteralPath $backup -Destination $path }
    $corrupt = [byte[]]$original.Clone()
    $corrupt[0] = $corrupt[0] -bxor 1
    [IO.File]::WriteAllBytes($path, $corrupt)
    $hash = (Get-FileHash -LiteralPath $path).Hash
    try {
        Assert-Rejected $arguments 'Source supplement checksum mismatch:*'
        if ((Get-FileHash -LiteralPath $path).Hash -ne $hash) { throw 'Corrupt supplement was overwritten.' }
    }
    finally { [IO.File]::WriteAllBytes($path, $original) }
    if (Test-Path -LiteralPath $rejectedOutput) { throw 'Rejected preflight created output.' }
}

# A late corrupt input must stop Download before an earlier missing input is fetched.
$first = $paths[0]
$last = $paths[-1]
Move-Item -LiteralPath $first -Destination ($first + '.saved')
$original = [IO.File]::ReadAllBytes($last)
$corrupt = [byte[]]$original.Clone()
$corrupt[0] = $corrupt[0] -bxor 1
[IO.File]::WriteAllBytes($last, $corrupt)
try {
    $arguments.Download = $true
    Assert-Rejected $arguments 'Source supplement checksum mismatch:*'
    if (Test-Path -LiteralPath $first) { throw 'Download began before all cached inputs were checked.' }
}
finally {
    Move-Item -LiteralPath ($first + '.saved') -Destination $first
    [IO.File]::WriteAllBytes($last, $original)
}
foreach ($relative in $expected.Keys) {
    if ((Get-FileHash -LiteralPath (Join-Path $output $relative)).Hash -ne $expected[$relative]) { throw 'Existing output was changed.' }
}

# Source supplements with existing package notices must preserve their original bytes too.
$fixtureRepository = Join-Path $testDirectory 'repository'
foreach ($name in @('scripts/prepare-native-source-supplements.ps1', 'docs/native-source-supplements.json',
    'docs/native-runtime-recipes.json', 'docs/native-runtime-package-audit.json')) {
    $path = Join-Path $fixtureRepository $name
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repositoryRoot $name) -Destination $path
}
$generator = Join-Path $fixtureRepository 'scripts/prepare-native-source-supplements.ps1'
$manifestPath = Join-Path $fixtureRepository 'docs/native-source-supplements.json'
$manifestBytes = [IO.File]::ReadAllBytes($manifestPath)
$encoding = [Text.UTF8Encoding]::new($false)
$arguments = @{ OutputDirectory = $rejectedOutput; CacheDirectory = $CacheDirectory; RecipeDirectory = $RecipeDirectory }
foreach ($packageName in @('mingw-w64-x86_64-xz', 'mingw-w64-x86_64-freetype')) {
    foreach ($kind in @('missing', 'different', 'duplicate')) {
        $changed = $encoding.GetString($manifestBytes) | ConvertFrom-Json
        $package = $changed.packages | Where-Object { $_.package -eq $packageName }
        $notice = $package.selected_documents | Where-Object { $_.name -match '/(COPYING|docs/FTL.TXT)$' }
        switch ($kind) {
            'missing' { $package.selected_documents = @($package.selected_documents | Where-Object { $_.name -ne $notice.name }) }
            'different' { $notice.sha256 = '0' * 64 }
            'duplicate' { $package.selected_documents = @($package.selected_documents) + $notice }
        }
        [IO.File]::WriteAllText($manifestPath, ($changed | ConvertTo-Json -Depth 10), $encoding)
        try {
            Assert-Rejected $arguments 'Source supplement does not retain audited package notice:*'
            if (Test-Path -LiteralPath $rejectedOutput) { throw 'Notice mismatch created output.' }
        }
        finally { [IO.File]::WriteAllBytes($manifestPath, $manifestBytes) }
    }
}
Write-Output "Source supplement checks passed: $($expected.Count) exact output files, arbitrary cwd, repeated generation, $($paths.Count) missing/corrupt input pairs, six package-notice mismatches, cached-input/download and output preservation."
