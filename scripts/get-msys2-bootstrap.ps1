[CmdletBinding()]
param(
    [switch]$Download,
    [string]$CacheDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/msys2-bootstrap-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($inventory.schema_version -ne 1) { throw 'Unsupported MSYS2 bootstrap inventory.' }
if (-not $CacheDirectory) { $CacheDirectory = Join-Path $repositoryRoot 'vendor/msys2/bootstrap-20260611' }
$CacheDirectory = [IO.Path]::GetFullPath($CacheDirectory)

function Assert-BootstrapFile([string]$Path, $Record) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "MSYS2 bootstrap file is missing: $Path" }
    if ((Get-Item -LiteralPath $Path).Length -ne $Record.bytes -or
        (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Record.sha256) {
        throw "MSYS2 bootstrap checksum mismatch: $Path"
    }
}

foreach ($file in $inventory.files) {
    $path = Join-Path $CacheDirectory $file.name
    if ($Download -and -not (Test-Path -LiteralPath $path)) {
        New-Item -ItemType Directory -Path $CacheDirectory -Force | Out-Null
        $temporary = Join-Path $CacheDirectory ([IO.Path]::GetRandomFileName())
        try {
            $url = 'https://github.com/msys2/msys2-installer/releases/download/' + $inventory.release + '/' + $file.name
            Write-Output "Downloading MSYS2 bootstrap input: $($file.name)"
            & curl.exe --disable --fail --location --silent --show-error --connect-timeout 20 --max-time 180 --output $temporary $url
            if ($LASTEXITCODE -ne 0) { throw "MSYS2 bootstrap download failed: $($file.name)" }
            Assert-BootstrapFile $temporary $file
            Move-Item -LiteralPath $temporary -Destination $path
        }
        finally {
            if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
        }
    }
    Assert-BootstrapFile $path $file
    Write-Output "Verified MSYS2 bootstrap input: $($file.name)"
}

Write-Output "Bootstrap cache: $CacheDirectory"
Write-Output 'Pinned byte checks passed. No archive execution, extraction, environment initialization or package update was performed.'
