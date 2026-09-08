[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateSet('uavs3d', 'vvenc')][string]$Codec,
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
if (Test-Path -LiteralPath $BuildDirectory) { throw 'Use a fresh codec build directory; existing output is preserved.' }
$expectedRevision = if ($Codec -eq 'uavs3d') { '0e20d2c291853f196c68922a264bcd8471d75b68' } else { '0f2e874451d6b194615e5dfefdc96796a7da00f4' }
$revision = & git -C $SourceDirectory rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $revision -ne $expectedRevision) { throw 'Codec source revision does not match the pinned input.' }
$changed = @(& git -c core.autocrlf=false -c core.filemode=false -C $SourceDirectory diff --name-only HEAD)
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect codec source changes.' }
if ($Codec -eq 'uavs3d') {
    if ($changed.Count -ne 1 -or $changed[0] -ne 'source/decoder/uavs3d.h' -or
        (Get-FileHash "$SourceDirectory/source/decoder/uavs3d.h").Hash -ne 'C4193DA1A00D43CB41EB9E7F61C5782545E8F76ECE72477EDD52310F30CB6192') {
        throw 'Apply only uavs3d-cdecl-guard.patch to the verified source.'
    }
    $options = @('-DCOMPILE_10BIT=1', '-DCMAKE_POLICY_VERSION_MINIMUM=3.5')
    $package = 'uavs3d'
    $versionGate = 'uavs3d >= 1.1.41'
}
else {
    if ($changed.Count -ne 0) { throw 'Use the unmodified verified VVenC source.' }
    $options = @('-DVVENC_LIBRARY_ONLY=ON', '-DVVENC_ENABLE_WERROR=OFF', '-DVVENC_ENABLE_LINK_TIME_OPT=OFF',
        '-DEXTRALIBS=-lstdc++', '-DVVENC_TOPLEVEL_OUTPUT_DIRS=OFF', '-DCCACHE_FOUND=OFF')
    $package = 'libvvenc'
    $versionGate = 'libvvenc >= 1.6.1'
}
$saved = @{}
foreach ($name in @('PATH', 'PKG_CONFIG_PATH', 'PKG_CONFIG_LIBDIR', 'BASH_ENV', 'ENV',
        'GIT_CONFIG_COUNT', 'GIT_CONFIG_KEY_0', 'GIT_CONFIG_VALUE_0', 'GIT_CONFIG_KEY_1', 'GIT_CONFIG_VALUE_1')) {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
try {
    $env:PATH = "$MsysRoot/mingw64/bin;$MsysRoot/usr/bin;" + (Join-Path $env:SystemRoot 'System32')
    $env:PKG_CONFIG_PATH = $null
    $env:PKG_CONFIG_LIBDIR = "$MsysRoot/mingw64/lib/pkgconfig"
    Remove-Item Env:BASH_ENV,Env:ENV -ErrorAction SilentlyContinue
    $env:GIT_CONFIG_COUNT = '2'
    $env:GIT_CONFIG_KEY_0 = 'core.autocrlf'
    $env:GIT_CONFIG_VALUE_0 = 'false'
    $env:GIT_CONFIG_KEY_1 = 'core.filemode'
    $env:GIT_CONFIG_VALUE_1 = 'false'
    $cmake = "$MsysRoot/mingw64/bin/cmake.exe"
    $prefix = "$BuildDirectory/prefix"
    Push-Location $SourceDirectory
    try {
        & $cmake -S $SourceDirectory -B "$BuildDirectory/build" -G Ninja -DCMAKE_BUILD_TYPE=Release `
            "-DCMAKE_INSTALL_PREFIX=$prefix" "-DCMAKE_C_COMPILER=$MsysRoot/mingw64/bin/gcc.exe" `
            "-DCMAKE_CXX_COMPILER=$MsysRoot/mingw64/bin/g++.exe" "-DCMAKE_MAKE_PROGRAM=$MsysRoot/mingw64/bin/ninja.exe" `
            -DBUILD_SHARED_LIBS=OFF @options
        if ($LASTEXITCODE -ne 0) { throw 'Native codec configuration failed.' }
        if ($Codec -eq 'uavs3d' -and (Get-Content "$SourceDirectory/version.h" -Raw) -notmatch $expectedRevision) {
            throw 'AVS3 version generation selected a different repository.'
        }
        & $cmake --build "$BuildDirectory/build" --parallel 8
        if ($LASTEXITCODE -ne 0) { throw 'Native codec compilation failed.' }
        & $cmake --install "$BuildDirectory/build"
        if ($LASTEXITCODE -ne 0) { throw 'Native codec staged installation failed.' }
    }
    finally { Pop-Location }
    $env:PKG_CONFIG_LIBDIR = "$prefix/lib/pkgconfig;$MsysRoot/mingw64/lib/pkgconfig"
    $pkgConfig = "$MsysRoot/mingw64/bin/pkg-config.exe"
    $selectedPrefix = & $pkgConfig --variable=prefix $package
    if ($LASTEXITCODE -ne 0 -or $selectedPrefix -ne $prefix) { throw 'pkg-config selected a different codec prefix.' }
    & $pkgConfig --exists $versionGate
    if ($LASTEXITCODE -ne 0) { throw 'Codec does not pass the FFmpeg version gate.' }
    $flags = & $pkgConfig --static --cflags --libs $package
    if ($LASTEXITCODE -ne 0) { throw 'Codec pkg-config lookup failed.' }
    & "$MsysRoot/mingw64/bin/gcc.exe" -Wall -Wextra -Werror (Join-Path $PSScriptRoot "fixtures/$Codec-smoke.c") `
        -o "$BuildDirectory/api-smoke.exe" @($flags -split ' ' | Where-Object { $_ })
    if ($LASTEXITCODE -ne 0) { throw 'Codec C API static link failed.' }
    & "$BuildDirectory/api-smoke.exe"
    if ($LASTEXITCODE -ne 0) { throw 'Native codec smoke test failed.' }
    Write-Output "Native static $Codec prefix: $prefix"
    Write-Output 'This is not a full FFmpeg, AVS3 bitstream, VVC decode or distribution verification.'
}
finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
}
