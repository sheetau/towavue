[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateSet('chromaprint', 'openal', 'zvbi', 'gcc-libs')][string]$Component,
    [Parameter(Mandatory = $true)][string]$PackageArchive,
    [Parameter(Mandatory = $true)][string]$Recipe,
    [Parameter(Mandatory = $true)][string]$SourceArchive,
    [Parameter(Mandatory = $true)][string[]]$RuntimeDll,
    [string]$LgplLicense,
    [string]$GplLicense,
    [string]$PatchDirectory,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$inventoryPath = Join-Path $repositoryRoot "docs/native-$Component-inputs.json"
$inventory = Get-Content -LiteralPath $inventoryPath -Raw -Encoding UTF8 | ConvertFrom-Json
if ($inventory.schema_version -ne 1) { throw 'Unsupported native package inventory.' }
$packageInventory = if ($Component -eq 'gcc-libs') { 'docs/msys2-toolchain-inputs.json' } else { 'docs/msys2-media-inputs.json' }
$media = Get-Content -LiteralPath (Join-Path $repositoryRoot $packageInventory) -Raw -Encoding UTF8 | ConvertFrom-Json
$package = @($media.packages | Where-Object { $_.name -eq $inventory.package.name })
if ($package.Count -ne 1 -or $package[0].version -ne $inventory.package.version -or
    $package[0].sha256 -ne $inventory.package.sha256 -or $package[0].bytes -ne $inventory.package.bytes) {
    throw 'Native package inventory is stale.'
}

function Assert-Material([string]$Path, $Record) {
    if (-not $Path -or -not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing native package material: $Path" }
    if ((Get-Item -LiteralPath $Path).Length -ne $Record.bytes -or
        (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Record.sha256) {
        throw "Native package material checksum mismatch: $Path"
    }
}

Assert-Material $PackageArchive $inventory.package
Assert-Material $Recipe $inventory.recipe
Assert-Material $SourceArchive $inventory.source
$runtimes = @($inventory.runtime)
if ($RuntimeDll.Count -ne $runtimes.Count) { throw 'Incorrect native runtime DLL count.' }
for ($index = 0; $index -lt $runtimes.Count; $index++) {
    Assert-Material $RuntimeDll[$index] $runtimes[$index]
}
if ($inventory.lgpl) { Assert-Material $LgplLicense $inventory.lgpl }
if ($inventory.gpl) { Assert-Material $GplLicense $inventory.gpl }
foreach ($patch in $inventory.patches) {
    if (-not $PatchDirectory) { throw 'Missing native package material: patch directory' }
    Assert-Material (Join-Path $PatchDirectory $patch.name) $patch
}
$PackageArchive = (Resolve-Path -LiteralPath $PackageArchive).Path
$SourceArchive = (Resolve-Path -LiteralPath $SourceArchive).Path
$OutputDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh materials output directory; existing output is preserved.' }

# Only selected regular entries of the exact hash-verified archives are extracted.
# In particular, the source archive's internal header symlink is not materialized.
$tar = Join-Path $env:SystemRoot 'System32/tar.exe'
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
& $tar -xf $PackageArchive -C $OutputDirectory @($inventory.package_metadata.name)
if ($LASTEXITCODE -ne 0) { throw 'Native package metadata extraction failed.' }
& $tar -xf $SourceArchive -C $OutputDirectory @($inventory.source_notices | ForEach-Object { $inventory.source.prefix + $_.name })
if ($LASTEXITCODE -ne 0) { throw 'Native package source notice extraction failed.' }
foreach ($file in $inventory.package_metadata) { Assert-Material (Join-Path $OutputDirectory $file.name) $file }
foreach ($file in $inventory.source_notices) {
    Assert-Material (Join-Path $OutputDirectory ($inventory.source.prefix + $file.name)) $file
}
$buildInfo = Get-Content -LiteralPath (Join-Path $OutputDirectory '.BUILDINFO') -Raw -Encoding UTF8
if ($buildInfo -notmatch ('(?m)^pkgbuild_sha256sum = ' + $inventory.recipe.sha256 + '\r?$')) {
    throw 'The package build record does not identify the pinned recipe.'
}
$recipeText = Get-Content -LiteralPath $Recipe -Raw -Encoding UTF8
foreach ($required in @($inventory.source.sha256) + @($inventory.recipe.required_options) + @($inventory.patches.sha256)) {
    if ($required -and -not $recipeText.Contains($required)) { throw 'The recipe does not identify the pinned source, patches and options.' }
}
Copy-Item -LiteralPath $SourceArchive -Destination (Join-Path $OutputDirectory $inventory.source.name)
Copy-Item -LiteralPath $Recipe -Destination (Join-Path $OutputDirectory 'PKGBUILD')
if ($inventory.lgpl) {
    Copy-Item -LiteralPath $LgplLicense -Destination (Join-Path $OutputDirectory 'COPYING.LGPLv2.1')
    Assert-Material (Join-Path $OutputDirectory 'COPYING.LGPLv2.1') $inventory.lgpl
}
if ($inventory.gpl) {
    Copy-Item -LiteralPath $GplLicense -Destination (Join-Path $OutputDirectory 'COPYING.GPLv2')
    Assert-Material (Join-Path $OutputDirectory 'COPYING.GPLv2') $inventory.gpl
}
foreach ($patch in $inventory.patches) {
    Copy-Item -LiteralPath (Join-Path $PatchDirectory $patch.name) -Destination (Join-Path $OutputDirectory $patch.name)
    Assert-Material (Join-Path $OutputDirectory $patch.name) $patch
}
Copy-Item -LiteralPath $inventoryPath -Destination (Join-Path $OutputDirectory 'INPUTS.json')
Copy-Item -LiteralPath (Join-Path $repositoryRoot "third-party/$($Component.ToUpperInvariant())-MATERIALS-README.txt") -Destination (Join-Path $OutputDirectory 'README.txt')
Assert-Material (Join-Path $OutputDirectory $inventory.source.name) $inventory.source
Assert-Material (Join-Path $OutputDirectory 'PKGBUILD') $inventory.recipe
Write-Output "Native $Component materials: $OutputDirectory"
Write-Output 'Offline byte checks passed; no build, download, binary copy or publication was performed.'
