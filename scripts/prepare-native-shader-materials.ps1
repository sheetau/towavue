[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$CacheDirectory,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$PackageDirectory = (Join-Path (Split-Path -Parent $PSScriptRoot) 'vendor/msys2/packages-20260908')
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$inventoryPath = Join-Path $repositoryRoot 'docs/native-shader-inputs.json'
$inventory = Get-Content -LiteralPath $inventoryPath -Raw -Encoding UTF8 | ConvertFrom-Json
$audit = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-runtime-package-audit.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($inventory.schema_version -ne 1 -or $inventory.components.Count -ne 4) { throw 'Incomplete native shader inventory.' }
$CacheDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($CacheDirectory)
$PackageDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($PackageDirectory)
$OutputDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh native shader output directory.' }
foreach ($root in @($CacheDirectory, $PackageDirectory)) {
    if ($root -eq $OutputDirectory -or
        $root.StartsWith($OutputDirectory.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
        $OutputDirectory.StartsWith($root.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Native shader output overlaps an input directory.'
    }
}
function Assert-Path([string]$Name) {
    if (-not $Name -or $Name -match '(^/|[\\:"]|(^|/)\.\.(/|$))') { throw 'Invalid native shader material path.' }
}
function Assert-Input([string]$Path, $Record) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing native shader input: $Path" }
    if ((Get-Item -LiteralPath $Path).Length -ne $Record.bytes -or
        (Get-FileHash -LiteralPath $Path).Hash -ne $Record.sha256) { throw "Native shader checksum mismatch: $Path" }
}
$tar = Join-Path $env:SystemRoot 'System32/tar.exe'
$names = @{}
foreach ($component in $inventory.components) {
    Assert-Path $component.name
    if ($names.ContainsKey($component.name)) { throw 'Duplicate native shader component.' }
    $names[$component.name] = $true
    foreach ($record in @($component.package_archive, $component.package_signature)) {
        Assert-Path $record.name
        Assert-Input (Join-Path $PackageDirectory $record.name) $record
    }
    foreach ($record in @($component.recipe, $component.source) + @($component.patches)) {
        Assert-Path $record.name
        Assert-Input (Join-Path $CacheDirectory $record.name) $record
    }
    foreach ($record in @($component.package_documents) + @($component.selected_documents)) { Assert-Path $record.name }
    $consumer = @($audit.packages | Where-Object name -eq $component.consumer)
    if ($consumer.Count -ne 1) { throw 'Stale native shader consumer mapping.' }
    $parent = $consumer[0]
    $parentPath = Join-Path $PackageDirectory ($parent.archive_url.Split('/')[-1])
    Assert-Input $parentPath @{bytes=$parent.archive_bytes;sha256=$parent.archive_sha256}
    $parentBuild = (& $tar -xOf $parentPath '.BUILDINFO') -join "`n"
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read native shader consumer record.' }
    $expected = 'installed = ' + $component.package + '-' + $component.version + '-any'
    if ($parentBuild -notmatch ('(?m)^' + [regex]::Escape($expected) + '$')) { throw 'Stale native shader dependency mapping.' }
    $build = (& $tar -xOf (Join-Path $PackageDirectory $component.package_archive.name) '.BUILDINFO') -join "`n"
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read native shader package record.' }
    foreach ($line in @(('pkgname = ' + $component.package), ('pkgver = ' + $component.version),
        ('pkgbuild_sha256sum = ' + $component.recipe.sha256))) {
        if ($build -notmatch ('(?m)^' + [regex]::Escape($line) + '$')) { throw 'Stale native shader package mapping.' }
    }
    $recipe = Get-Content -LiteralPath (Join-Path $CacheDirectory $component.recipe.name) -Raw -Encoding UTF8
    foreach ($record in @($component.source) + @($component.patches)) {
        if (-not $recipe.Contains($record.sha256)) { throw 'Stale native shader recipe mapping.' }
    }
}

# Hash-pinned archives were inventoried as safe regular files/directories.
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
foreach ($component in $inventory.components) {
    $directory = Join-Path $OutputDirectory $component.name
    foreach ($selection in @(
        @{Root=$PackageDirectory; Archive=$component.package_archive; Folder='package'; Documents=$component.package_documents},
        @{Root=$CacheDirectory; Archive=$component.source; Folder='source'; Documents=$component.selected_documents}
    )) {
        $destination = Join-Path $directory $selection.Folder
        New-Item -ItemType Directory -Path $destination -Force | Out-Null
        & $tar -xf (Join-Path $selection.Root $selection.Archive.name) -C $destination @($selection.Documents.name)
        if ($LASTEXITCODE -ne 0) { throw 'Native shader extraction failed.' }
        foreach ($record in $selection.Documents) { Assert-Input (Join-Path $destination $record.name) $record }
    }
    foreach ($record in @($component.recipe, $component.source) + @($component.patches)) {
        $path = Join-Path $directory $record.name
        Copy-Item -LiteralPath (Join-Path $CacheDirectory $record.name) -Destination $path
        Assert-Input $path $record
    }
}
Copy-Item -LiteralPath (Join-Path $repositoryRoot 'third-party/NATIVE-SHADER-MATERIALS-README.txt') -Destination (Join-Path $OutputDirectory 'README.txt')
# A failed extraction never receives the completion inventory.
Copy-Item -LiteralPath $inventoryPath -Destination (Join-Path $OutputDirectory 'INPUTS.json')
Write-Output "Native shader source materials: $OutputDirectory"
Write-Output 'Four static/header inputs only; no installation, runtime change or distribution approval.'
