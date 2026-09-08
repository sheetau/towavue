[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$MsysRoot,
    [Parameter(Mandatory = $true)][string]$SourceDirectory,
    [Parameter(Mandatory = $true)][string]$BuildDirectory
)

$ErrorActionPreference = 'Stop'
$MsysRoot = (Resolve-Path -LiteralPath $MsysRoot).Path.Replace('\', '/')
$SourceDirectory = (Resolve-Path -LiteralPath $SourceDirectory).Path.Replace('\', '/')
$BuildDirectory = [IO.Path]::GetFullPath($BuildDirectory).Replace('\', '/')
foreach ($path in @($MsysRoot, $SourceDirectory, $BuildDirectory)) {
    if ($path -notmatch '^[A-Za-z]:/[A-Za-z0-9_./-]+$') { throw 'Use ASCII paths without spaces for this native build.' }
}
if (Test-Path -LiteralPath $BuildDirectory) { throw 'Use a fresh RIST build directory; existing output is preserved.' }
$revision = & git -C $SourceDirectory rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $revision -ne '4f45ef8f78983892d52ccd52d9f675435b23738f') {
    throw 'RIST source revision does not match the pinned input.'
}
$changed = @(& git -c core.autocrlf=false -c core.filemode=false -C $SourceDirectory diff --name-only HEAD)
if ($LASTEXITCODE -ne 0 -or $changed.Count -ne 0) { throw 'Use the unmodified verified RIST source.' }
$saved = @{}
foreach ($name in @('PATH', 'CC', 'CXX', 'PKG_CONFIG_PATH', 'PKG_CONFIG_LIBDIR', 'CMAKE_PREFIX_PATH',
        'GIT_CONFIG_COUNT', 'GIT_CONFIG_KEY_0', 'GIT_CONFIG_VALUE_0', 'GIT_CONFIG_KEY_1', 'GIT_CONFIG_VALUE_1')) {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
try {
    $env:PATH = "$MsysRoot/mingw64/bin;$MsysRoot/usr/bin;" + (Join-Path $env:SystemRoot 'System32')
    $env:CC = "$MsysRoot/mingw64/bin/gcc.exe"
    $env:CXX = "$MsysRoot/mingw64/bin/g++.exe"
    $env:PKG_CONFIG_PATH = $null
    $env:PKG_CONFIG_LIBDIR = "$MsysRoot/mingw64/lib/pkgconfig"
    $env:CMAKE_PREFIX_PATH = $null
    $env:GIT_CONFIG_COUNT = '2'
    $env:GIT_CONFIG_KEY_0 = 'core.autocrlf'
    $env:GIT_CONFIG_VALUE_0 = 'false'
    $env:GIT_CONFIG_KEY_1 = 'core.filemode'
    $env:GIT_CONFIG_VALUE_1 = 'false'
    $meson = "$MsysRoot/mingw64/bin/meson.exe"
    $prefix = "$BuildDirectory/prefix"
    & $meson setup "$BuildDirectory/build" $SourceDirectory "--prefix=$prefix" --buildtype=release `
        --default-library=static --wrap-mode=nodownload -Duse_mbedtls=true -Dbuiltin_mbedtls=true `
        -Dbuiltin_cjson=true -Dfallback_builtin=false -Dbuilt_tools=false -Dtest=false -Dhave_mingw_pthreads=true
    if ($LASTEXITCODE -ne 0) { throw 'RIST native configuration failed.' }
    & $meson compile -C "$BuildDirectory/build" -j 8
    if ($LASTEXITCODE -ne 0) { throw 'RIST compilation failed.' }
    & $meson install -C "$BuildDirectory/build"
    if ($LASTEXITCODE -ne 0) { throw 'RIST staged installation failed.' }
    $env:PKG_CONFIG_LIBDIR = "$prefix/lib/pkgconfig;$MsysRoot/mingw64/lib/pkgconfig"
    $pkgConfig = "$MsysRoot/mingw64/bin/pkg-config.exe"
    $selectedPrefix = & $pkgConfig --variable=prefix librist
    if ($LASTEXITCODE -ne 0 -or $selectedPrefix -ne $prefix) { throw 'pkg-config selected a different RIST prefix.' }
    & $pkgConfig --exists 'librist >= 0.2.7'
    if ($LASTEXITCODE -ne 0) { throw 'RIST does not pass the FFmpeg version gate.' }
    $flags = & $pkgConfig --static --cflags --libs librist
    if ($LASTEXITCODE -ne 0) { throw 'RIST pkg-config lookup failed.' }
    & "$MsysRoot/mingw64/bin/gcc.exe" -Wall -Wextra -Werror (Join-Path $PSScriptRoot 'fixtures/librist-smoke.c') `
        -o "$BuildDirectory/api-smoke.exe" @($flags -split ' ' | Where-Object { $_ })
    if ($LASTEXITCODE -ne 0) { throw 'RIST C API static link failed.' }
    & "$BuildDirectory/api-smoke.exe"
    if ($LASTEXITCODE -ne 0) { throw 'RIST receiver initialization/destruction failed.' }
    Write-Output "Native static RIST prefix: $prefix"
    Write-Output 'Uses the fixed source bundled Mbed TLS 3.6.6 and cJSON, not external Mbed TLS 4.2.'
    Write-Output 'This is not an encrypted-stream, full FFmpeg or distribution verification.'
}
finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
}
