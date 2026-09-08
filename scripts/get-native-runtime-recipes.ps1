[CmdletBinding()]
param([switch]$Download, [string]$CacheDirectory)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-runtime-recipes.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$audit = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-runtime-package-audit.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($inventory.schema_version -ne 1 -or $inventory.recipes.Count -ne $audit.packages.Count) { throw 'Incomplete runtime recipe inventory.' }
if (-not $CacheDirectory) { $CacheDirectory = Join-Path $repositoryRoot 'vendor/msys2/runtime-recipes-20260908' }
$CacheDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($CacheDirectory)
foreach ($package in $audit.packages) {
    $recipe = @($inventory.recipes | Where-Object { $_.package -eq $package.name })
    if ($recipe.Count -ne 1 -or $recipe[0].sha256 -ne $package.recipe_sha256 -or
        $recipe[0].name -ne ($package.name + '.PKGBUILD')) { throw "Stale runtime recipe mapping: $($package.name)" }
}

function Assert-Recipe([string]$Path, $Record) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing runtime recipe: $Path" }
    if ((Get-Item -LiteralPath $Path).Length -ne $Record.bytes -or (Get-FileHash -LiteralPath $Path).Hash -ne $Record.sha256) {
        throw "Runtime recipe checksum mismatch: $Path"
    }
}

foreach ($recipe in $inventory.recipes) {
    $path = Join-Path $CacheDirectory $recipe.name
    if ($Download -and -not (Test-Path -LiteralPath $path)) {
        New-Item -ItemType Directory -Path $CacheDirectory -Force | Out-Null
        $temporary = Join-Path $CacheDirectory ([IO.Path]::GetRandomFileName())
        try {
            & curl.exe --disable --fail --location --silent --show-error --connect-timeout 20 --max-time 120 --output $temporary $recipe.url
            if ($LASTEXITCODE -ne 0) { throw "Runtime recipe retrieval failed: $($recipe.package)" }
            Assert-Recipe $temporary $recipe
            Move-Item -LiteralPath $temporary -Destination $path
        }
        finally { if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary } }
    }
    Assert-Recipe $path $recipe
}
Write-Output "Verified $($inventory.recipes.Count) package-build-matched recipes: $CacheDirectory"
Write-Output 'No recipe, package build, source retrieval or license approval was performed.'
