[CmdletBinding()]
param(
    [switch]$Download,
    [switch]$IncludeMediaDependencies,
    [string]$CacheDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/msys2-toolchain-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($inventory.schema_version -ne 1) { throw 'Unsupported MSYS2 toolchain inventory.' }
$signatureInventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/msys2-toolchain-signatures.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($signatureInventory.schema_version -ne 1 -or $signatureInventory.packages.Count -ne $inventory.packages.Count) {
    throw 'MSYS2 toolchain signature inventory is stale.'
}
$signatures = @{}
foreach ($signature in $signatureInventory.packages) { $signatures.Add($signature.name, $signature) }
if ($IncludeMediaDependencies) {
    $media = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/msys2-media-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($media.schema_version -ne 1) { throw 'Unsupported MSYS2 media inventory.' }
    foreach ($package in $media.packages) { $signatures.Add($package.name, $package.signature) }
    $inventory.packages = @($inventory.packages) + @($media.packages)
}
if (-not $CacheDirectory) { $CacheDirectory = Join-Path $repositoryRoot 'vendor/msys2/packages-20260908' }
$CacheDirectory = [IO.Path]::GetFullPath($CacheDirectory)

function Assert-Package([string]$Path, $Record) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "MSYS2 package is missing: $Path" }
    if ((Get-Item -LiteralPath $Path).Length -ne $Record.bytes -or
        (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Record.sha256) {
        throw "MSYS2 package checksum mismatch: $Path"
    }
}

$files = @(foreach ($package in $inventory.packages) {
    if (-not $signatures.ContainsKey($package.name)) { throw "Missing pinned signature: $($package.name)" }
    $package
    [pscustomobject]@{
        name = $package.name + '.sig'
        version = $package.version
        url = $package.url + '.sig'
        bytes = $signatures[$package.name].bytes
        sha256 = $signatures[$package.name].sha256
    }
})
foreach ($package in $files) {
    $path = Join-Path $CacheDirectory ([uri]$package.url).Segments[-1]
    if ($Download -and -not (Test-Path -LiteralPath $path)) {
        New-Item -ItemType Directory -Path $CacheDirectory -Force | Out-Null
        $temporary = Join-Path $CacheDirectory ([IO.Path]::GetRandomFileName())
        try {
            Write-Output "Downloading MSYS2 package: $($package.name) $($package.version)"
            & curl.exe --disable --fail --location --silent --show-error --connect-timeout 20 --max-time 180 --output $temporary $package.url
            if ($LASTEXITCODE -ne 0) { throw "MSYS2 package download failed: $($package.name)" }
            Assert-Package $temporary $package
            Move-Item -LiteralPath $temporary -Destination $path
        }
        finally {
            if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
        }
    }
    Assert-Package $path $package
}
Write-Output "Verified $($inventory.packages.Count) pinned MSYS2 package archives and detached signature files: $CacheDirectory"
Write-Output 'No package installation, hook execution or live dependency resolution was performed.'
