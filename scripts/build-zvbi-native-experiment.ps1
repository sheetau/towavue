[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$MsysRoot,
    [Parameter(Mandatory = $true)][string]$SourceArchive,
    [Parameter(Mandatory = $true)][string]$BuildDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-zvbi-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$MsysRoot = (Resolve-Path -LiteralPath $MsysRoot).Path.Replace('\', '/')
$SourceArchive = (Resolve-Path -LiteralPath $SourceArchive).Path
$BuildDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($BuildDirectory).Replace('\', '/')
foreach ($path in @($MsysRoot, $BuildDirectory)) {
    if ($path -notmatch '^[A-Za-z]:/[A-Za-z0-9_./-]+$') { throw 'Use ASCII paths without spaces for this native experiment.' }
}
if (Test-Path -LiteralPath $BuildDirectory) { throw 'Use a fresh ZVBI experiment directory; existing output is preserved.' }
if ((Get-Item -LiteralPath $SourceArchive).Length -ne $inventory.source.bytes -or
    (Get-FileHash -LiteralPath $SourceArchive).Hash -ne $inventory.source.sha256) {
    throw 'ZVBI source archive does not match the pinned VCS archive.'
}
$patch = Join-Path $repositoryRoot 'third-party/patches/zvbi-no-program-id.patch'
if ((Get-FileHash -LiteralPath $patch).Hash -ne '9ef31d7e15655f7ceecabb1516a663ecd01ceec0dc104b4d7aec656923a83874') {
    throw 'ZVBI experiment patch differs from the reviewed input.'
}
New-Item -ItemType Directory -Path "$BuildDirectory/source", "$BuildDirectory/build" | Out-Null
& (Join-Path $env:SystemRoot 'System32/tar.exe') -xf $SourceArchive -C "$BuildDirectory/source"
if ($LASTEXITCODE -ne 0) { throw 'ZVBI source extraction failed.' }
& git -c core.autocrlf=false -C "$BuildDirectory/source" apply --check $patch
if ($LASTEXITCODE -ne 0) { throw 'ZVBI experiment patch check failed.' }
& git -c core.autocrlf=false -C "$BuildDirectory/source" apply $patch
if ($LASTEXITCODE -ne 0) { throw 'ZVBI experiment patch application failed.' }

$saved = @{}
foreach ($name in @('PATH', 'PKG_CONFIG_PATH', 'PKG_CONFIG_LIBDIR', 'BASH_ENV', 'ENV', 'CFLAGS', 'CXXFLAGS')) {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
try {
    $env:PATH = "$MsysRoot/mingw64/bin;$MsysRoot/usr/bin;" + (Join-Path $env:SystemRoot 'System32')
    $env:PKG_CONFIG_PATH = $null
    $env:PKG_CONFIG_LIBDIR = "$MsysRoot/mingw64/lib/pkgconfig"
    $env:BASH_ENV = $null
    $env:ENV = $null
    $env:CFLAGS = '-O2 -g'
    $env:CXXFLAGS = '-O2 -g'
    $buildUnix = '/' + $BuildDirectory.Substring(0, 1).ToLowerInvariant() + $BuildDirectory.Substring(2)
    $command = "set -e; exec > $buildUnix/build.log 2>&1; export ACLOCAL_PATH=/mingw64/share/aclocal; cd $buildUnix/source; NOCONFIGURE=1 ./autogen.sh; cd ../build; ../source/configure --prefix=$buildUnix/prefix --build=x86_64-w64-mingw32 --host=x86_64-w64-mingw32 --target=x86_64-w64-mingw32 --disable-examples --disable-tests --enable-static --enable-shared; make -C src -j8; make -C src install; make install-pkgconfigDATA"
    & "$MsysRoot/usr/bin/bash.exe" --noprofile --norc -c $command
    if ($LASTEXITCODE -ne 0) { throw "ZVBI experimental build failed; inspect $BuildDirectory/build.log." }
    $prefix = "$BuildDirectory/prefix"
    $dependencies = @(Get-ChildItem -LiteralPath "$BuildDirectory/build/src/.deps" -Filter '*.Plo')
    if ($dependencies.Count -eq 0) { throw 'Missing actual compiler dependency records.' }
    foreach ($file in $dependencies) {
        if ((Get-Content -LiteralPath $file.FullName -Raw) -match '(pdc|packet-830)\.[ch]|exp-vtx\.c|dvb/(frontend|dmx)\.h|videodev2k\.h') {
            throw "Excluded source/header occurs in compiler dependencies: $($file.Name)"
        }
    }
    $header = & "$MsysRoot/mingw64/bin/gcc.exe" -E -P -x c "$prefix/include/libzvbi.h"
    if ($LASTEXITCODE -ne 0 -or ($header -join "`n") -match '\b(vbi_program_id|vbi_pil|vbi_pid_channel|vbi_decode_teletext_830\w*)\b') {
        throw 'Regenerated public header retains excluded declarations or does not preprocess.'
    }
    $exports = & "$MsysRoot/mingw64/bin/objdump.exe" -p "$prefix/bin/libzvbi-0.dll"
    if ($LASTEXITCODE -ne 0 -or ($exports -join "`n") -match '\b(vbi_pil_\w*|vbi_pty_validity_window|vbi_\w*pdc\w*|vbi_\w*teletext_830\w*)\b') {
        throw 'Experimental DLL retains excluded exports or cannot be inspected.'
    }
    $env:PKG_CONFIG_LIBDIR = "$prefix/lib/pkgconfig;$MsysRoot/mingw64/lib/pkgconfig"
    $selected = & "$MsysRoot/mingw64/bin/pkg-config.exe" --variable=prefix zvbi-0.2
    if ($LASTEXITCODE -ne 0 -or $selected -ne $prefix) { throw 'pkg-config selected another ZVBI prefix.' }
    & "$MsysRoot/mingw64/bin/pkg-config.exe" --exists 'zvbi-0.2 >= 0.2.28'
    if ($LASTEXITCODE -ne 0) { throw 'ZVBI does not satisfy the FFmpeg version gate.' }
    Write-Output "Experimental ZVBI prefix: $prefix"
    Write-Output "Checked $($dependencies.Count) compiler dependency records and the preprocessed public header."
    Write-Output 'Not a full-API ZVBI replacement or distribution approval; teletext comparison is still required.'
}
finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
}
