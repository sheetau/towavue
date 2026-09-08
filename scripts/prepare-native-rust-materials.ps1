[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$SourceSupplementDirectory,
    [Parameter(Mandatory = $true)][string]$CrateCacheDirectory,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$inventoryPath = Join-Path $repositoryRoot 'docs/native-rust-dependencies.json'
$inventory = Get-Content -LiteralPath $inventoryPath -Raw -Encoding UTF8 | ConvertFrom-Json
$audit = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-runtime-package-audit.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$supplements = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-source-supplements.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($inventory.schema_version -ne 1 -or $inventory.components.Count -ne 2 -or
    $inventory.crates.Count -ne 157 -or $inventory.additional_notices.Count -ne 4) { throw 'Incomplete native Rust material inventory.' }
$SourceSupplementDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($SourceSupplementDirectory)
$CrateCacheDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($CrateCacheDirectory)
$OutputDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh native Rust material output directory.' }
foreach ($directory in @($SourceSupplementDirectory, $CrateCacheDirectory, (Join-Path $repositoryRoot 'third-party/native-rust'))) {
    if ($directory -eq $OutputDirectory -or
        $directory.StartsWith($OutputDirectory.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
        $OutputDirectory.StartsWith($directory.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Native Rust output overlaps an input directory.'
    }
}

function Assert-Input([string]$Path, $Record) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing native Rust input: $Path" }
    if ((Get-Item -LiteralPath $Path).Length -ne $Record.bytes -or
        (Get-FileHash -LiteralPath $Path).Hash -ne $Record.sha256) { throw "Native Rust input checksum mismatch: $Path" }
}

$locked = @{}
foreach ($component in $inventory.components) {
    $owner = @($audit.packages | Where-Object name -eq $component.package)
    $runtimeRecord = @($audit.runtime | Where-Object name -eq $component.runtime.name)
    $source = @($supplements.packages | Where-Object package -eq $component.package)
    if ($owner.Count -ne 1 -or $owner[0].recipe_sha256 -ne $component.recipe_sha256 -or
        $runtimeRecord.Count -ne 1 -or $runtimeRecord[0].sha256 -ne $component.runtime.sha256 -or
        $runtimeRecord[0].bytes -ne $component.runtime.bytes -or
        @($inventory.crates | Where-Object component -eq $component.name).Count -ne $component.crate_count) {
        throw 'Stale native Rust component mapping.'
    }
    if ($source.Count -ne 1 -or $source[0].recipe_sha256 -ne $component.recipe_sha256) {
        throw 'Stale native Rust source mapping.'
    }
    $archive = @($source[0].files | Where-Object name -eq $component.source_archive.name)
    $lock = @($source[0].selected_documents | Where-Object name -eq $component.cargo_lock.name)
    if ($archive.Count -ne 1 -or $archive[0].sha256 -ne $component.source_archive.sha256 -or
        $archive[0].bytes -ne $component.source_archive.bytes -or $lock.Count -ne 1 -or
        $lock[0].sha256 -ne $component.cargo_lock.sha256 -or $lock[0].bytes -ne $component.cargo_lock.bytes) {
        throw 'Stale native Rust source mapping.'
    }
    $directory = Join-Path $SourceSupplementDirectory $component.package
    Assert-Input (Join-Path $directory $component.source_archive.name) $component.source_archive
    $lockPath = Join-Path $directory $component.cargo_lock.name
    Assert-Input $lockPath $component.cargo_lock
    $lockText = Get-Content -LiteralPath $lockPath -Raw -Encoding UTF8
    foreach ($block in ($lockText -split '(?m)^\[\[package\]\]')) {
        $name = [regex]::Match($block, '(?m)^name = "([^"]+)"').Groups[1].Value
        $version = [regex]::Match($block, '(?m)^version = "([^"]+)"').Groups[1].Value
        $checksum = [regex]::Match($block, '(?m)^checksum = "([^"]+)"').Groups[1].Value
        if ($checksum) { $locked[$component.name + '/' + $name + '-' + $version] = $checksum }
    }
}
$seen = [Collections.Generic.HashSet[string]]::new()
foreach ($crate in $inventory.crates) {
    $stem = $crate.name + '-' + $crate.version
    $key = $crate.component + '/' + $stem
    if (-not $seen.Add($key) -or $crate.archive.name -ne ($stem + '.crate') -or
        $locked[$key] -ne $crate.archive.sha256) { throw 'Stale native Rust crate/lock mapping.' }
    Assert-Input (Join-Path $CrateCacheDirectory $crate.archive.name) $crate.archive
    foreach ($document in $crate.selected_documents) {
        if (-not $document.name.StartsWith($stem + '/', [StringComparison]::Ordinal) -or
            $document.name -match '(^/|[\\:"]|(^|/)\.\.(/|$))') { throw 'Invalid native Rust document path.' }
    }
}
foreach ($notice in $inventory.additional_notices) { Assert-Input (Join-Path $repositoryRoot $notice.repository_path) $notice }

# All source locks, crate archives and external originals pass before output is created.
$tar = Join-Path $env:SystemRoot 'System32/tar.exe'
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
foreach ($component in $inventory.components) {
    $directory = Join-Path $OutputDirectory $component.name
    New-Item -ItemType Directory -Path $directory | Out-Null
    Copy-Item -LiteralPath (Join-Path (Join-Path $SourceSupplementDirectory $component.package) $component.cargo_lock.name) -Destination (Join-Path $directory 'Cargo.lock')
    Assert-Input (Join-Path $directory 'Cargo.lock') $component.cargo_lock
}
foreach ($crate in $inventory.crates) {
    $directory = Join-Path $OutputDirectory $crate.component
    $archive = Join-Path $directory $crate.archive.name
    Copy-Item -LiteralPath (Join-Path $CrateCacheDirectory $crate.archive.name) -Destination $archive
    Assert-Input $archive $crate.archive
    # These fixed regular members belong to the pinned archive; original bytes are retained.
    & $tar -xf $archive -C $directory @($crate.selected_documents.name)
    if ($LASTEXITCODE -ne 0) { throw 'Native Rust document extraction failed.' }
    foreach ($document in $crate.selected_documents) { Assert-Input (Join-Path $directory $document.name) $document }
}
$noticeDirectory = Join-Path $OutputDirectory 'upstream-notices'
New-Item -ItemType Directory -Path $noticeDirectory | Out-Null
foreach ($notice in $inventory.additional_notices) {
    $path = Join-Path $noticeDirectory $notice.name
    Copy-Item -LiteralPath (Join-Path $repositoryRoot $notice.repository_path) -Destination $path
    Assert-Input $path $notice
}
Copy-Item -LiteralPath (Join-Path $repositoryRoot 'third-party/NATIVE-RUST-MATERIALS-README.txt') -Destination (Join-Path $OutputDirectory 'README.txt')
# Failed extraction leaves diagnostics, never a completed material inventory.
Copy-Item -LiteralPath $inventoryPath -Destination (Join-Path $OutputDirectory 'INPUTS.json')
Write-Output "Native Rust materials: $OutputDirectory"
Write-Output '157 pinned crate archives, selected original documents and two source locks; not linked-code or compiler-runtime closure.'
