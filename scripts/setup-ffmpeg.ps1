[CmdletBinding()]
param(
    [switch]$ExportGitHubEnvironment
)

$ErrorActionPreference = 'Stop'
$release = 'autobuild-2026-09-03-13-17'
$asset = 'ffmpeg-n9.0.1-11-ge47273f4d9-win64-lgpl-shared-9.0.zip'
$expectedSha256 = 'ad26fca80435853043bd75a989be38261fa28b54bb623459b88786177b22fa86'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$vendorDirectory = Join-Path $repositoryRoot 'vendor\ffmpeg'
$archivePath = Join-Path $vendorDirectory $asset
$distributionName = [IO.Path]::GetFileNameWithoutExtension($asset)
$distributionRoot = Join-Path $vendorDirectory $distributionName
$headerPath = Join-Path $distributionRoot 'include\libavcodec\avcodec.h'
$downloadUri = "https://github.com/BtbN/FFmpeg-Builds/releases/download/$release/$asset"

New-Item -ItemType Directory -Path $vendorDirectory -Force | Out-Null

if (-not (Test-Path -LiteralPath $archivePath)) {
    Invoke-WebRequest -Uri $downloadUri -OutFile $archivePath
}

$actualSha256 = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actualSha256 -ne $expectedSha256) {
    throw "FFmpeg archive checksum mismatch. Expected $expectedSha256, got $actualSha256."
}

if (-not (Test-Path -LiteralPath $headerPath)) {
    Expand-Archive -LiteralPath $archivePath -DestinationPath $vendorDirectory
}

if (-not (Test-Path -LiteralPath $headerPath)) {
    throw "FFmpeg archive did not contain the expected development files at $distributionRoot."
}

$binDirectory = Join-Path $distributionRoot 'bin'
$libclangDirectory = Join-Path $env:ProgramFiles 'LLVM\bin'
$libclangPath = Join-Path $libclangDirectory 'libclang.dll'
if (-not (Test-Path -LiteralPath $libclangPath)) {
    throw 'libclang.dll was not found. Install LLVM.LLVM before building towavue.'
}

$env:FFMPEG_DIR = $distributionRoot
$env:LIBCLANG_PATH = $libclangDirectory
$env:PATH = "$binDirectory;$env:PATH"

if ($ExportGitHubEnvironment) {
    if ([string]::IsNullOrWhiteSpace($env:GITHUB_ENV)) {
        throw 'GITHUB_ENV is not available.'
    }

    "FFMPEG_DIR=$distributionRoot" | Out-File -LiteralPath $env:GITHUB_ENV -Encoding utf8 -Append
    "LIBCLANG_PATH=$libclangDirectory" | Out-File -LiteralPath $env:GITHUB_ENV -Encoding utf8 -Append
    $binDirectory | Out-File -LiteralPath $env:GITHUB_PATH -Encoding utf8 -Append
}

Write-Output $distributionRoot
