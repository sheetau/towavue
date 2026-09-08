[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$getter = Join-Path $PSScriptRoot 'get-native-runtime-recipes.ps1'
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-runtime-recipes.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$cache = Join-Path $repositoryRoot 'vendor/msys2/runtime-recipes-20260908'
$testDirectory = Join-Path $repositoryRoot ('target/tmp/runtime-recipes-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory | Out-Null
$timestamps = @{}
foreach ($recipe in $inventory.recipes) { $timestamps[$recipe.name] = (Get-Item -LiteralPath (Join-Path $cache $recipe.name)).LastWriteTimeUtc }
Push-Location $testDirectory
try { & $getter -CacheDirectory $cache }
finally { Pop-Location }
foreach ($recipe in $inventory.recipes) {
    if ((Get-Item -LiteralPath (Join-Path $cache $recipe.name)).LastWriteTimeUtc -ne $timestamps[$recipe.name]) { throw 'Cached recipe was rewritten.' }
}
foreach ($kind in @('missing', 'corrupt')) {
    $directory = Join-Path $testDirectory $kind
    if ($kind -eq 'corrupt') {
        New-Item -ItemType Directory -Path $directory | Out-Null
        $first = $inventory.recipes[0]
        $path = Join-Path $directory $first.name
        Copy-Item -LiteralPath (Join-Path $cache $first.name) -Destination $path
        $bytes = [IO.File]::ReadAllBytes($path)
        $bytes[0] = $bytes[0] -bxor 1
        [IO.File]::WriteAllBytes($path, $bytes)
        $hash = (Get-FileHash -LiteralPath $path).Hash
    }
    $rejected = $false
    try { & $getter -CacheDirectory $directory -Download:($kind -eq 'corrupt') | Out-Null }
    catch {
        $expected = if ($kind -eq 'corrupt') { 'Runtime recipe checksum mismatch:*' } else { 'Missing runtime recipe:*' }
        if ($_.Exception.Message -notlike $expected) { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw "Invalid recipe input accepted: $kind" }
    if ($kind -eq 'missing' -and (Test-Path -LiteralPath $directory)) { throw 'Offline missing input created a cache.' }
    if ($kind -eq 'corrupt' -and ((Get-FileHash -LiteralPath $path).Hash -ne $hash -or @(Get-ChildItem -LiteralPath $directory).Count -ne 1)) {
        throw 'Corrupt recipe was changed or followed by more downloads.'
    }
}
Write-Output 'Runtime recipe cache, arbitrary cwd, offline missing-input and corrupt-input preservation checks passed.'
