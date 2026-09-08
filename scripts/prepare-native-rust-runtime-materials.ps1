[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$CacheDirectory,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$inventoryPath = Join-Path $repositoryRoot 'docs/native-rust-runtime-inputs.json'
$inventory = Get-Content -LiteralPath $inventoryPath -Raw -Encoding UTF8 | ConvertFrom-Json
$native = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-rust-dependencies.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($inventory.schema_version -ne 1 -or $inventory.toolchains.Count -ne 2 -or
    $inventory.inputs.Count -ne 65 -or $inventory.archives.Count -ne 46) { throw 'Incomplete native Rust runtime inventory.' }
$CacheDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($CacheDirectory)
$OutputDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh native Rust runtime output directory.' }
if ($CacheDirectory -eq $OutputDirectory -or
    $CacheDirectory.StartsWith($OutputDirectory.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
    $OutputDirectory.StartsWith($CacheDirectory.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Native Rust runtime output overlaps its cache.'
}

function Assert-Path([string]$Name) {
    if (-not $Name -or $Name -match '(^/|[\\:"]|(^|/)\.\.(/|$))') { throw 'Invalid native Rust runtime material path.' }
}
function Assert-Input([string]$Path, $Record) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing native Rust runtime input: $Path" }
    if ((Get-Item -LiteralPath $Path).Length -ne $Record.bytes -or
        (Get-FileHash -LiteralPath $Path).Hash -ne $Record.sha256) { throw "Native Rust runtime checksum mismatch: $Path" }
}

$inputs = @{}
foreach ($record in $inventory.inputs) {
    Assert-Path $record.name
    if ($record.output) { Assert-Path $record.output }
    if ($inputs.ContainsKey($record.name)) { throw 'Duplicate native Rust runtime input.' }
    $inputs[$record.name] = $record
    Assert-Input (Join-Path $CacheDirectory $record.name) $record
}
$tar = Join-Path $env:SystemRoot 'System32/tar.exe'
foreach ($toolchain in $inventory.toolchains) {
    $component = @($native.components | Where-Object name -eq $toolchain.used_by)
    if ($component.Count -ne 1 -or $component[0].build_rust_package -ne $toolchain.build_rust_package -or
        $toolchain.build_rust_package -ne ('mingw-w64-x86_64-rust-' + $toolchain.version + '-any')) {
        throw 'Stale native Rust runtime component mapping.'
    }
    $recipe = Get-Content -LiteralPath (Join-Path $CacheDirectory $toolchain.recipe) -Raw -Encoding UTF8
    foreach ($hash in @($toolchain.source_release.sha256) + @($toolchain.recipe_inputs | ForEach-Object { $inputs[$_].sha256 })) {
        if (-not $hash -or -not $recipe.Contains($hash)) { throw 'Stale native Rust runtime recipe mapping.' }
    }
}
foreach ($archive in $inventory.archives) {
    Assert-Path $archive.output
    if (-not $inputs.ContainsKey($archive.input)) { throw 'Stale native Rust runtime archive mapping.' }
    foreach ($document in $archive.documents) { Assert-Path $document.name }
    if ($archive.package_name) {
        $buildInfo = (& $tar -xOf (Join-Path $CacheDirectory $archive.input) '.BUILDINFO') -join "`n"
        if ($LASTEXITCODE -ne 0) { throw 'Cannot read native Rust runtime package record.' }
        foreach ($line in @(('pkgname = ' + $archive.package_name), ('pkgver = ' + $archive.version),
            ('pkgbuild_sha256sum = ' + $inputs[$archive.recipe].sha256))) {
            if ($buildInfo -notmatch ('(?m)^' + [regex]::Escape($line) + '$')) { throw 'Stale native Rust runtime package mapping.' }
        }
    }
}
$lockText = (& $tar -xOf (Join-Path $CacheDirectory 'mingw-w64-x86_64-rust-src-1.87.0-2-any.pkg.tar.zst') 'mingw64/lib/rustlib/src/rust/library/Cargo.lock') -join "`n"
if ($LASTEXITCODE -ne 0) { throw 'Cannot read native Rust standard-library lock.' }
$locked = @{}
foreach ($block in ($lockText -split '(?m)^\[\[package\]\]')) {
    $name = [regex]::Match($block, '(?m)^name = "([^"]+)"').Groups[1].Value
    $version = [regex]::Match($block, '(?m)^version = "([^"]+)"').Groups[1].Value
    $checksum = [regex]::Match($block, '(?m)^checksum = "([^"]+)"').Groups[1].Value
    if ($checksum) { $locked['std-1.87-crates/' + $name + '-' + $version + '.crate'] = $checksum }
}
if ($locked.Count -ne 42) { throw 'Stale native Rust standard-library lock.' }
foreach ($name in $locked.Keys) {
    if ($inputs[$name].sha256 -ne $locked[$name]) { throw 'Stale native Rust standard-library lock mapping.' }
}

# Only fixed, previously inventoried regular members are selected from pinned inputs.
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
foreach ($archive in $inventory.archives) {
    $directory = Join-Path $OutputDirectory $archive.output
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
    & $tar -xf (Join-Path $CacheDirectory $archive.input) -C $directory @($archive.documents.name)
    if ($LASTEXITCODE -ne 0) { throw 'Native Rust runtime extraction failed.' }
    foreach ($document in $archive.documents) { Assert-Input (Join-Path $directory $document.name) $document }
}
foreach ($record in $inventory.inputs | Where-Object output) {
    $path = Join-Path $OutputDirectory $record.output
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $CacheDirectory $record.name) -Destination $path
    Assert-Input $path $record
}
Copy-Item -LiteralPath (Join-Path $repositoryRoot 'third-party/NATIVE-RUST-RUNTIME-README.txt') -Destination (Join-Path $OutputDirectory 'README.txt')
# An extraction failure leaves diagnostic files, never a completed inventory.
Copy-Item -LiteralPath $inventoryPath -Destination (Join-Path $OutputDirectory 'INPUTS.json')
Write-Output "Native Rust runtime materials: $OutputDirectory"
Write-Output 'Original source/notice materials only; no compiler binary package, installation or distribution approval.'
