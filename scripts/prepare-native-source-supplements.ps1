[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$CacheDirectory,
    [string]$RecipeDirectory,
    [switch]$Download
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$inventoryPath = Join-Path $repositoryRoot 'docs/native-source-supplements.json'
$recipePath = Join-Path $repositoryRoot 'docs/native-runtime-recipes.json'
$auditPath = Join-Path $repositoryRoot 'docs/native-runtime-package-audit.json'
$inventory = Get-Content -LiteralPath $inventoryPath -Raw -Encoding UTF8 | ConvertFrom-Json
$recipes = Get-Content -LiteralPath $recipePath -Raw -Encoding UTF8 | ConvertFrom-Json
$audit = Get-Content -LiteralPath $auditPath -Raw -Encoding UTF8 | ConvertFrom-Json
if ($inventory.schema_version -ne 1 -or $inventory.packages.Count -ne 37) { throw 'Incomplete source supplement inventory.' }
if ($inventory.additional_notices.Count -ne 2) { throw 'Incomplete additional notice inventory.' }
if (-not $CacheDirectory) { $CacheDirectory = Join-Path $repositoryRoot 'vendor/msys2/source-supplements-20260908' }
if (-not $RecipeDirectory) { $RecipeDirectory = Join-Path $repositoryRoot 'vendor/msys2/runtime-recipes-20260908' }
$CacheDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($CacheDirectory)
$RecipeDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($RecipeDirectory)
$OutputDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh source supplement output directory; existing output is preserved.' }
foreach ($inputDirectory in @($CacheDirectory, $RecipeDirectory)) {
    if ($inputDirectory.StartsWith($OutputDirectory.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
        $OutputDirectory.StartsWith($inputDirectory.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
        $inputDirectory -eq $OutputDirectory) { throw 'Source supplement output overlaps an input directory.' }
}

function Assert-Input([string]$Path, $Record) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing source supplement input: $Path" }
    if ((Get-Item -LiteralPath $Path).Length -ne $Record.bytes -or (Get-FileHash -LiteralPath $Path).Hash -ne $Record.sha256) {
        throw "Source supplement checksum mismatch: $Path"
    }
}

# Check all existing inputs before retrieving anything or creating output.
foreach ($notice in $inventory.additional_notices) {
    Assert-Input (Join-Path $repositoryRoot $notice.repository_path) $notice
}
foreach ($package in $inventory.packages) {
    $owner = @($audit.packages | Where-Object { $_.name -eq $package.package })
    $recipe = @($recipes.recipes | Where-Object { $_.package -eq $package.package })
    if ($owner.Count -ne 1 -or $recipe.Count -ne 1 -or
        $owner[0].recipe_sha256 -ne $package.recipe_sha256 -or $recipe[0].sha256 -ne $package.recipe_sha256) {
        throw "Stale source supplement mapping: $($package.package)"
    }
    foreach ($notice in $owner[0].package_notices) {
        $noticeInputs = @($package.selected_documents) + @($package.files | Where-Object { $_.package_notice -and $_.kind -eq 'build-input' })
        $matching = @($noticeInputs | Where-Object {
            $_.bytes -eq $notice.bytes -and $_.sha256 -eq $notice.sha256 -and
            (-not $_.package_notice -or $_.package_notice -eq $notice.name)
        })
        if ($matching.Count -ne 1) { throw "Source supplement does not retain audited package notice: $($notice.name)" }
    }
    $path = Join-Path $RecipeDirectory $recipe[0].name
    Assert-Input $path $recipe[0]
    $recipeText = Get-Content -LiteralPath $path -Raw -Encoding UTF8
    foreach ($file in $package.files) {
        $path = Join-Path (Join-Path $CacheDirectory $package.package) $file.name
        if ($file.vcs_commit) {
            if ($file.kind -ne 'source' -or $file.vcs_commit -notmatch '^[a-f0-9]{40}$' -or
                $file.url -notmatch ('^git\+https://[^#]+#commit=' + $file.vcs_commit + '$') -or
                -not $recipeText.Contains('"' + $file.url + '"')) { throw 'Recipe does not identify the pinned VCS source.' }
            # VCS archives are prepared locally with the recorded Git command, never fetched as HTTP files.
            Assert-Input $path $file
        }
        else {
            if (-not $recipeText.Contains($file.sha256)) { throw "Recipe does not identify supplement checksum: $($file.name)" }
            if (-not $Download -or (Test-Path -LiteralPath $path)) { Assert-Input $path $file }
        }
    }
}
foreach ($package in $inventory.packages) {
    $directory = Join-Path $CacheDirectory $package.package
    foreach ($file in $package.files) {
        $path = Join-Path $directory $file.name
        if (-not (Test-Path -LiteralPath $path)) {
            if (-not $Download) { throw "Missing source supplement input: $path" }
            New-Item -ItemType Directory -Path $directory -Force | Out-Null
            $temporary = Join-Path $directory ([IO.Path]::GetRandomFileName())
            try {
                & curl.exe --disable --fail --location --silent --show-error --connect-timeout 20 --max-time 180 --output $temporary $file.url
                if ($LASTEXITCODE -ne 0) { throw "Source supplement retrieval failed: $($file.name)" }
                Assert-Input $temporary $file
                Move-Item -LiteralPath $temporary -Destination $path
            }
            finally { if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary } }
        }
    }
}

$tar = Join-Path $env:SystemRoot 'System32/tar.exe'
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
foreach ($package in $inventory.packages) {
    $directory = Join-Path $OutputDirectory $package.package
    New-Item -ItemType Directory -Path $directory | Out-Null
    $recipe = $recipes.recipes | Where-Object { $_.package -eq $package.package }
    Copy-Item -LiteralPath (Join-Path $RecipeDirectory $recipe.name) -Destination (Join-Path $directory 'PKGBUILD')
    Assert-Input (Join-Path $directory 'PKGBUILD') $recipe
    foreach ($file in $package.files) {
        Copy-Item -LiteralPath (Join-Path (Join-Path $CacheDirectory $package.package) $file.name) -Destination (Join-Path $directory $file.name)
        Assert-Input (Join-Path $directory $file.name) $file
    }
    $source = @($package.files | Where-Object { $_.kind -eq 'source' })
    if ($source.Count -ne 1) { throw 'Expected one pinned source archive per supplement.' }
    # Only fixed regular members of the hash-verified original archive are selected.
    # Internal source symlinks stay inside the preserved archives and are not extracted.
    & $tar -xf (Join-Path $directory $source[0].name) -C $directory @($package.selected_documents.name)
    if ($LASTEXITCODE -ne 0) { throw "Source supplement document extraction failed: $($package.package)" }
    foreach ($document in $package.selected_documents) { Assert-Input (Join-Path $directory $document.name) $document }
    Write-Output "Verified source supplement: $($package.package)"
}
Copy-Item -LiteralPath $recipePath, $auditPath -Destination $OutputDirectory
foreach ($notice in $inventory.additional_notices) {
    $path = Join-Path $OutputDirectory $notice.name
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repositoryRoot $notice.repository_path) -Destination $path
    Assert-Input $path $notice
}
Copy-Item -LiteralPath (Join-Path $repositoryRoot 'third-party/NATIVE-SOURCE-SUPPLEMENTS-README.txt') -Destination (Join-Path $OutputDirectory 'README.txt')
# INPUTS is the completion marker; failed extractions leave diagnostic output only.
Copy-Item -LiteralPath $inventoryPath -Destination (Join-Path $OutputDirectory 'INPUTS.json')
Write-Output "Native source supplements: $OutputDirectory"
Write-Output 'No recipe execution, patch application, build, binary copy, signature verification or distribution approval was performed.'
