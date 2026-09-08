[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$CacheDirectory,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$inventoryPath = Join-Path $repositoryRoot 'docs/native-gcc-static-inputs.json'
$inventory = Get-Content -LiteralPath $inventoryPath -Raw -Encoding UTF8 | ConvertFrom-Json
$audit = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-runtime-package-audit.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($inventory.schema_version -ne 1 -or $inventory.inputs.Count -ne 22 -or $inventory.archives.Count -ne 3) { throw 'Incomplete native GCC static inventory.' }
$CacheDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($CacheDirectory)
$OutputDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh native GCC static output directory.' }
if ($CacheDirectory -eq $OutputDirectory -or
    $CacheDirectory.StartsWith($OutputDirectory.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
    $OutputDirectory.StartsWith($CacheDirectory.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Native GCC static output overlaps its cache.'
}
function Assert-Path([string]$Name) {
    if (-not $Name -or $Name -match '(^/|[\\:"]|(^|/)\.\.(/|$))') { throw 'Invalid native GCC static material path.' }
}
function Assert-Input([string]$Path, $Record) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing native GCC static input: $Path" }
    if ((Get-Item -LiteralPath $Path).Length -ne $Record.bytes -or
        (Get-FileHash -LiteralPath $Path).Hash -ne $Record.sha256) { throw "Native GCC static checksum mismatch: $Path" }
}
$inputs = @{}
foreach ($record in $inventory.inputs) {
    Assert-Path $record.name
    if ($record.output) { Assert-Path $record.output }
    if ($inputs.ContainsKey($record.name)) { throw 'Duplicate native GCC static input.' }
    $inputs[$record.name] = $record
    Assert-Input (Join-Path $CacheDirectory $record.name) $record
}
$consumer = @($audit.packages | Where-Object name -eq $inventory.consumer.package)
$inputRecord = $inputs[$inventory.consumer.input]
if ($consumer.Count -ne 1 -or $consumer[0].archive_sha256 -ne $inputRecord.sha256 -or $consumer[0].archive_bytes -ne $inputRecord.bytes) { throw 'Stale native GCC static consumer mapping.' }
$tar = Join-Path $env:SystemRoot 'System32/tar.exe'
$build = (& $tar -xOf (Join-Path $CacheDirectory $inventory.consumer.input) '.BUILDINFO') -join "`n"
if ($LASTEXITCODE -ne 0) { throw 'Cannot read native GCC static consumer record.' }
$recipe = Get-Content -LiteralPath (Join-Path $CacheDirectory $inventory.recipe) -Raw -Encoding UTF8
foreach ($name in @($inventory.source) + @($inventory.recipe_inputs)) {
    if (-not $inputs.ContainsKey($name) -or -not $recipe.Contains($inputs[$name].sha256)) { throw 'Stale native GCC static recipe mapping.' }
}
foreach ($archive in $inventory.archives) {
    Assert-Path $archive.output
    if (-not $inputs.ContainsKey($archive.input)) { throw 'Stale native GCC static archive mapping.' }
    foreach ($document in $archive.documents) { Assert-Path $document.name }
    if ($archive.package_name) {
        $line = 'installed = ' + $archive.package_name + '-' + $inventory.version + '-any'
        if ($build -notmatch ('(?m)^' + [regex]::Escape($line) + '$')) { throw 'Stale native GCC static dependency mapping.' }
        $package = (& $tar -xOf (Join-Path $CacheDirectory $archive.input) '.BUILDINFO') -join "`n"
        if ($LASTEXITCODE -ne 0) { throw 'Cannot read native GCC static package record.' }
        foreach ($line in @(('pkgname = ' + $archive.package_name), ('pkgver = ' + $inventory.version), ('pkgbuild_sha256sum = ' + $inputs[$inventory.recipe].sha256))) {
            if ($package -notmatch ('(?m)^' + [regex]::Escape($line) + '$')) { throw 'Stale native GCC static package mapping.' }
        }
    }
}
foreach ($mapping in $inventory.matching_headers) {
    $installed = @($inventory.archives[0].documents | Where-Object name -eq $mapping.package_member)
    $original = @($inventory.archives[2].documents | Where-Object name -eq $mapping.source_member)
    if ($installed.Count -ne 1 -or $original.Count -ne 1 -or $installed[0].sha256 -ne $original[0].sha256) { throw 'Stale native GCC static header mapping.' }
}

# Select only previously inventoried regular members from the hash-pinned archives.
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
foreach ($archive in $inventory.archives) {
    $directory = Join-Path $OutputDirectory $archive.output
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
    & $tar -xf (Join-Path $CacheDirectory $archive.input) -C $directory @($archive.documents.name)
    if ($LASTEXITCODE -ne 0) { throw 'Native GCC static extraction failed.' }
    foreach ($document in $archive.documents) { Assert-Input (Join-Path $directory $document.name) $document }
}
foreach ($record in $inventory.inputs | Where-Object output) {
    $path = Join-Path $OutputDirectory $record.output
    Copy-Item -LiteralPath (Join-Path $CacheDirectory $record.name) -Destination $path
    Assert-Input $path $record
}
Copy-Item -LiteralPath (Join-Path $repositoryRoot 'third-party/NATIVE-GCC-STATIC-README.txt') -Destination (Join-Path $OutputDirectory 'README.txt')
# Partial extraction is diagnostic evidence, never a completed kit.
Copy-Item -LiteralPath $inventoryPath -Destination (Join-Path $OutputDirectory 'INPUTS.json')
Write-Output "Native GCC static materials: $OutputDirectory"
Write-Output 'Original source/header materials only; no installation, compiler execution or distribution approval.'
