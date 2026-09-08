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
if (Test-Path -LiteralPath $BuildDirectory) { throw 'Use a fresh ARIB build directory; existing output is preserved.' }
$revision = & git -C $SourceDirectory rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $revision -ne '5e9be272f96e00f15a2f3c5f8ba7e124862aec38') {
    throw 'ARIB source revision does not match the pinned input.'
}
$expected = @{
    'configure.ac' = '2eeaae1d6c8c9474b30ddec6145438a545dd77bce384a44ad99da6c9d2173225'
    'src/decoder.c' = 'f2a766ab92628456c6614f509bb943c5f66e0d85c8454c2f1df9b5e6adfbb78d'
    'src/decoder_macro.h' = 'b38e6ca6f899c880a194acebdc2c925a1bd51e352ef8e279840d0a4557c37088'
    'src/drcs.c' = 'd55d6bdbe3f543d8788be1bdd7efa80055804bc48f64e9e29808a91232d92464'
    'src/drcs.h' = '9350b47cf2eaac402985ed0a338cd9e4ec7936040a99c8e7d28a59282ce0607b'
    'src/parser.c' = 'c35899763ea3162a9caa7b6be7e877efc7262ab06ede6efc66ad8d18d70d383f'
}
$changed = @(& git -c core.autocrlf=false -c core.filemode=false -C $SourceDirectory diff --name-only HEAD)
if ($LASTEXITCODE -ne 0 -or $changed.Count -ne $expected.Count) { throw 'Unexpected ARIB source changes.' }
foreach ($path in $changed) {
    if (-not $expected.ContainsKey($path) -or (Get-FileHash -LiteralPath "$SourceDirectory/$path").Hash -ne $expected[$path]) {
        throw 'Apply only the three pinned recipe patches and aribb24-version.patch to the verified source.'
    }
}
$saved = @{}
foreach ($name in @('PATH', 'PKG_CONFIG_PATH', 'PKG_CONFIG_LIBDIR', 'BASH_ENV', 'ENV')) {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
try {
    $env:PATH = "$MsysRoot/mingw64/bin;$MsysRoot/usr/bin;" + (Join-Path $env:SystemRoot 'System32')
    $env:PKG_CONFIG_PATH = $null
    $env:PKG_CONFIG_LIBDIR = "$MsysRoot/mingw64/lib/pkgconfig"
    Remove-Item Env:BASH_ENV,Env:ENV -ErrorAction SilentlyContinue
    $sourceUnix = '/' + $SourceDirectory.Substring(0, 1).ToLowerInvariant() + $SourceDirectory.Substring(2)
    $buildUnix = '/' + $BuildDirectory.Substring(0, 1).ToLowerInvariant() + $BuildDirectory.Substring(2)
    $command = "set -e; export ACLOCAL_PATH=/mingw64/share/aclocal; cd $sourceUnix; autoreconf -fi; mkdir -p $buildUnix/build; cd $buildUnix/build; $sourceUnix/configure --host=x86_64-w64-mingw32 --prefix=$buildUnix/prefix --disable-shared --enable-static --with-pic; make -j8; make install"
    & "$MsysRoot/usr/bin/bash.exe" --noprofile --norc -c $command
    if ($LASTEXITCODE -ne 0) { throw 'ARIB native configuration, build or staged installation failed.' }
    $prefix = "$BuildDirectory/prefix"
    $env:PKG_CONFIG_LIBDIR = "$prefix/lib/pkgconfig;$MsysRoot/mingw64/lib/pkgconfig"
    $pkgConfig = "$MsysRoot/mingw64/bin/pkg-config.exe"
    $selectedPrefix = & $pkgConfig --variable=prefix aribb24
    if ($LASTEXITCODE -ne 0 -or $selectedPrefix -ne $prefix) { throw 'pkg-config selected a different ARIB prefix.' }
    & $pkgConfig --exists 'aribb24 > 1.0.3'
    if ($LASTEXITCODE -ne 0) { throw 'ARIB does not pass the FFmpeg version gate.' }
    $flags = & $pkgConfig --static --cflags --libs aribb24
    if ($LASTEXITCODE -ne 0) { throw 'ARIB pkg-config lookup failed.' }
    & "$MsysRoot/mingw64/bin/gcc.exe" -Wall -Wextra -Werror (Join-Path $PSScriptRoot 'fixtures/aribb24-smoke.c') `
        -o "$BuildDirectory/api-smoke.exe" @($flags -split ' ' | Where-Object { $_ })
    if ($LASTEXITCODE -ne 0) { throw 'ARIB C API static link failed.' }
    & "$BuildDirectory/api-smoke.exe"
    if ($LASTEXITCODE -ne 0) { throw 'ARIB character decode failed.' }
    Write-Output "Native static ARIB prefix: $prefix"
    Write-Output 'This is not a full subtitle rendering test, FFmpeg build or distribution approval.'
}
finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
}
