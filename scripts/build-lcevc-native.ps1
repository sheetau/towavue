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
if (Test-Path -LiteralPath $BuildDirectory) { throw 'Use a fresh LCEVC build directory; existing output is preserved.' }
$revision = & git -C $SourceDirectory rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $revision -ne 'a254bd474649e5dcd8182689ac414420bfe8d8c3') {
    throw 'LCEVC source revision does not match the pinned input.'
}
$changed = @(& git -c core.autocrlf=false -c core.filemode=false -C $SourceDirectory diff --name-only HEAD)
if ($LASTEXITCODE -ne 0 -or $changed.Count -ne 1 -or $changed[0] -ne 'cmake/modules/CMakeInstall.cmake' -or
    (Get-FileHash -LiteralPath "$SourceDirectory/cmake/modules/CMakeInstall.cmake").Hash -ne 'B93E16FA93BC816C59CF7BABB5108EE7B7EFDF7C5492A3AD15357790949C3354') {
    throw 'Apply only the pinned lcevc-static-link-order.patch to the verified source.'
}
$savedPath = $env:PATH
$savedPkgPath = $env:PKG_CONFIG_PATH
$savedPkgLibdir = $env:PKG_CONFIG_LIBDIR
try {
    $env:PATH = "$MsysRoot/mingw64/bin;$MsysRoot/usr/bin;" + (Join-Path $env:SystemRoot 'System32')
    $cmake = "$MsysRoot/mingw64/bin/cmake.exe"
    $prefix = "$BuildDirectory/prefix"
    & $cmake -S $SourceDirectory -B "$BuildDirectory/build" -G Ninja -DCMAKE_BUILD_TYPE=Release `
        "-DCMAKE_INSTALL_PREFIX=$prefix" "-DCMAKE_C_COMPILER=$MsysRoot/mingw64/bin/gcc.exe" `
        "-DCMAKE_CXX_COMPILER=$MsysRoot/mingw64/bin/g++.exe" "-DCMAKE_MAKE_PROGRAM=$MsysRoot/mingw64/bin/ninja.exe" `
        "-DPython3_EXECUTABLE=$MsysRoot/mingw64/bin/python.exe" "-DGIT_EXECUTABLE=$MsysRoot/usr/bin/git.exe" `
        -DBUILD_SHARED_LIBS=OFF -DVN_SDK_EXECUTABLES=OFF -DVN_SDK_SAMPLE_SOURCE=OFF `
        -DVN_SDK_TRACING=OFF -DVN_SDK_METRICS=OFF -DVN_SDK_SYSTEM_INSTALL=ON -DVN_SDK_PIPELINE_VULKAN=OFF `
        -DCMAKE_EXPORT_COMPILE_COMMANDS=ON
    if ($LASTEXITCODE -ne 0) { throw 'LCEVC configuration failed.' }
    & $cmake --build "$BuildDirectory/build" --parallel 8
    if ($LASTEXITCODE -ne 0) { throw 'LCEVC compilation failed.' }
    & $cmake --install "$BuildDirectory/build"
    if ($LASTEXITCODE -ne 0) { throw 'LCEVC staged installation failed.' }
    $env:PKG_CONFIG_PATH = $null
    $env:PKG_CONFIG_LIBDIR = "$prefix/lib/pkgconfig"
    $flags = & "$MsysRoot/mingw64/bin/pkg-config.exe" --static --cflags --libs lcevc_dec
    if ($LASTEXITCODE -ne 0) { throw 'LCEVC pkg-config lookup failed.' }
    & "$MsysRoot/mingw64/bin/gcc.exe" -Wall -Wextra -Werror (Join-Path $PSScriptRoot 'fixtures/lcevc-smoke.c') `
        -o "$BuildDirectory/api-smoke.exe" @($flags -split ' ' | Where-Object { $_ })
    if ($LASTEXITCODE -ne 0) { throw 'LCEVC C API static link failed.' }
    & "$BuildDirectory/api-smoke.exe"
    if ($LASTEXITCODE -ne 0) { throw 'LCEVC initialization/destruction test failed.' }
    Write-Output "Native static LCEVC prefix: $prefix"
    Write-Output 'This is not a full FFmpeg build, enhanced-frame decode test or distribution approval.'
}
finally {
    $env:PATH = $savedPath
    $env:PKG_CONFIG_PATH = $savedPkgPath
    $env:PKG_CONFIG_LIBDIR = $savedPkgLibdir
}
