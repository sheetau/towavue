[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$MsysRoot,
    [Parameter(Mandatory = $true)][string]$BuildDirectory,
    [Parameter(Mandatory = $true)][string]$Aribb24Prefix,
    [Parameter(Mandatory = $true)][string]$LcevcPrefix,
    [Parameter(Mandatory = $true)][string]$LibristPrefix,
    [Parameter(Mandatory = $true)][string]$Uavs3dPrefix,
    [Parameter(Mandatory = $true)][string]$VvencPrefix,
    [string]$ZvbiPrefix,
    [string]$SourceArchive
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$features = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/ffmpeg-native-features.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$baseline = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-runtime-package-audit.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$toolchain = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/msys2-toolchain-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$media = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/msys2-media-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if (-not $SourceArchive) { $SourceArchive = Join-Path $repositoryRoot 'vendor/ffmpeg/source-e47273f4d9227152dcbf543cebaf9e2430ddbcc4.tar.gz' }
if (-not (Test-Path -LiteralPath $SourceArchive -PathType Leaf)) { throw 'Missing pinned FFmpeg source archive.' }
if ((Get-Item -LiteralPath $SourceArchive).Length -ne 17323649 -or
    (Get-FileHash -LiteralPath $SourceArchive).Hash -ne '6491dae95e3cf3cdbac02933b55860e782b0c4f0a6bd8f37cef30fded259283c') {
    throw 'FFmpeg source archive checksum mismatch.'
}
$SourceArchive = (Resolve-Path -LiteralPath $SourceArchive).Path
$MsysRoot = (Resolve-Path -LiteralPath $MsysRoot).Path.Replace('\', '/')
$BuildDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($BuildDirectory).Replace('\', '/')
$prefixes = @($Aribb24Prefix, $LcevcPrefix, $LibristPrefix, $Uavs3dPrefix, $VvencPrefix) |
    ForEach-Object { (Resolve-Path -LiteralPath $_).Path.Replace('\', '/') }
if ($ZvbiPrefix) {
    $ZvbiPrefix = (Resolve-Path -LiteralPath $ZvbiPrefix).Path.Replace('\', '/')
    $prefixes += $ZvbiPrefix
    if ((Get-FileHash -LiteralPath "$ZvbiPrefix/include/libzvbi.h").Hash -ne '8e2a467f9ac02022a147470c868553eb563636b56b67b8f16da4aec8a8a14acc' -or
        (Get-FileHash -LiteralPath "$ZvbiPrefix/bin/libzvbi-0.dll").Hash -eq 'fa1b722aa22739a9b147f7c918a981c51fc48ebd1269b192e412b6f346f816cc') {
        throw 'Use the verified limited ZVBI experiment header and a rebuilt DLL.'
    }
}
foreach ($path in @($MsysRoot, $BuildDirectory) + $prefixes) {
    if ($path -notmatch '^[A-Za-z]:/[A-Za-z0-9_./-]+$') { throw 'Use ASCII paths without spaces for this native build.' }
}
if (Test-Path -LiteralPath $BuildDirectory) { throw 'Use a fresh FFmpeg build directory; existing output is preserved.' }
foreach ($path in @($MsysRoot, $SourceArchive.Replace('\', '/')) + $prefixes) {
    if ($path.StartsWith($BuildDirectory.TrimEnd('/') + '/', [StringComparison]::OrdinalIgnoreCase)) { throw 'FFmpeg build output overlaps an input.' }
}
& (Join-Path $PSScriptRoot 'test-ffmpeg-native-features.ps1')
$packageNames = @('aribb24', 'lcevc_dec', 'librist', 'uavs3d', 'libvvenc')
$linkLibraries = @('libaribb24.a', 'liblcevc_dec_api.a', 'librist.a', 'libuavs3d.a', 'libvvenc.a')
if ($ZvbiPrefix) {
    $packageNames += 'zvbi-0.2'
    $linkLibraries += 'libzvbi.dll.a'
}
$prefixInputs = @(for ($i = 0; $i -lt $prefixes.Count; $i++) {
    if (-not (Test-Path -LiteralPath ($prefixes[$i] + '/lib/' + $linkLibraries[$i]) -PathType Leaf)) { throw "Missing native link library: $($packageNames[$i])" }
    [ordered]@{
        package = $packageNames[$i]
        prefix = $prefixes[$i]
        files = @(Get-ChildItem -LiteralPath $prefixes[$i] -Recurse -File | Sort-Object FullName | ForEach-Object {
            [ordered]@{ name = $_.FullName.Replace('\', '/').Substring($prefixes[$i].Length + 1); bytes = $_.Length; sha256 = (Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant() }
        })
    }
})
$saved = @{}
foreach ($name in @('PATH', 'PKG_CONFIG_PATH', 'PKG_CONFIG_LIBDIR', 'PKG_CONFIG', 'BASH_ENV', 'ENV', 'CC', 'CXX', 'AR', 'LD', 'NM',
        'CFLAGS', 'CXXFLAGS', 'CPPFLAGS', 'LDFLAGS', 'LIBRARY_PATH', 'CPATH', 'C_INCLUDE_PATH', 'CPLUS_INCLUDE_PATH', 'GCC_EXEC_PREFIX',
        'COMPILER_PATH', 'MAKEFLAGS', 'MFLAGS', 'INCLUDE', 'LIB')) {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
function Invoke-BuildCommand([string]$Command, [string]$Log) {
    & "$MsysRoot/usr/bin/bash.exe" --noprofile --norc -c "set -e; $Command >$buildUnix/$Log 2>&1"
    if ($LASTEXITCODE -ne 0) { throw "Native FFmpeg step failed; see $BuildDirectory/$Log" }
}
function Write-Evidence([string]$Name, $Value) {
    [IO.File]::WriteAllText("$BuildDirectory/$Name", ($Value | ConvertTo-Json -Depth 12) + "`n", [Text.UTF8Encoding]::new($false))
}
try {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $null, 'Process') }
    $env:PATH = "$MsysRoot/mingw64/bin;$MsysRoot/usr/bin;" + (Join-Path $env:SystemRoot 'System32')
    $env:PKG_CONFIG_LIBDIR = (@($prefixes | ForEach-Object { "$_/lib/pkgconfig" }) + "$MsysRoot/mingw64/lib/pkgconfig") -join ';'
    $installed = @(& "$MsysRoot/usr/bin/pacman.exe" --config (Join-Path $PSScriptRoot 'fixtures/msys2-smoke/pacman.conf') -Q)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot query the native build environment.' }
    $expected = @(@($toolchain.packages) + @($media.packages) | ForEach-Object { $_.name + ' ' + $_.version })
    if (@(Compare-Object ($expected | Sort-Object) ($installed | Sort-Object)).Count) { throw 'Native build packages differ from the pinned snapshot.' }
    for ($i = 0; $i -lt $prefixes.Count; $i++) {
        $selected = & "$MsysRoot/mingw64/bin/pkg-config.exe" --variable=prefix $packageNames[$i]
        if ($LASTEXITCODE -ne 0 -or $selected.Replace('\', '/') -ne $prefixes[$i]) { throw "Wrong native prefix selected: $($packageNames[$i])" }
    }
    New-Item -ItemType Directory -Path "$BuildDirectory/source", "$BuildDirectory/build" | Out-Null
    # This exact archive was audited: 10548 regular/directory entries, one root, no links or traversal.
    & (Join-Path $env:SystemRoot 'System32/tar.exe') -xf $SourceArchive -C "$BuildDirectory/source"
    if ($LASTEXITCODE -ne 0) { throw 'Pinned FFmpeg source extraction failed.' }
    $buildUnix = '/' + $BuildDirectory.Substring(0, 1).ToLowerInvariant() + $BuildDirectory.Substring(2)
    $sourceUnix = "$buildUnix/source/FFmpeg-$($features.ffmpeg_commit)"
    $prefix = "$BuildDirectory/prefix"
    $cflags = ($prefixes | ForEach-Object { "-I$_/include" }) -join ' '
    $ldflags = ($prefixes | ForEach-Object { "-L$_/lib" }) -join ' '
    $flags = @($features.configure_flags) + @('--target-os=mingw32', '--arch=x86_64', '--pkg-config-flags=--static',
        "--prefix=$buildUnix/prefix", "--extra-cflags='$cflags'", "--extra-ldflags='$ldflags'")
    Write-Evidence 'build-inputs.json' ([ordered]@{ source_sha256 = (Get-FileHash -LiteralPath $SourceArchive).Hash.ToLowerInvariant(); configure_flags = $flags; packages = $installed; source_prefixes = $prefixInputs })
    Write-Output "Configuring all 81 retained flags: $BuildDirectory/configure.log"
    Invoke-BuildCommand "cd $buildUnix/build; $sourceUnix/configure $($flags -join ' ')" 'configure.log'
    $config = Get-Content -LiteralPath "$BuildDirectory/build/config.h" -Raw -Encoding UTF8
    foreach ($flag in $features.configure_flags | Where-Object { $_ -like '--enable-*' }) {
        $macro = $flag.Substring(9).Replace('-', '_').ToUpperInvariant()
        if ($config -notmatch "(?m)^#define (CONFIG|HAVE)_$macro 1\r?$" ) { throw "Native feature was not enabled: $flag" }
    }
    foreach ($macro in @('GPL', 'NONFREE', 'GPLV3')) {
        if ($config -notmatch "(?m)^#define CONFIG_$macro 0\r?$") { throw "Unexpected native license configuration: $macro" }
    }
    if ($config -notmatch '(?m)^#define HAVE_SPIRV_UNIFIED1_SPIRV_H 1\r?$') { throw 'Required SPIR-V headers were not found.' }
    $configMake = Get-Content -LiteralPath "$BuildDirectory/build/ffbuild/config.mak" -Raw -Encoding UTF8
    foreach ($entry in @(@{ key = 'CFLAGS'; first = $cflags }, @{ key = 'LDFLAGS'; first = $ldflags })) {
        if ($configMake -notmatch ('(?m)^' + $entry.key + '=[ \t]*' + [regex]::Escape($entry.first))) { throw "Native prefix order is wrong in $($entry.key)." }
    }
    Write-Output "Building native FFmpeg (8 jobs): $BuildDirectory/build.log"
    Invoke-BuildCommand "cd $buildUnix/build; make -j8" 'build.log'
    if ($ZvbiPrefix) {
        $dependencies = Get-Content -LiteralPath "$BuildDirectory/build/libavcodec/libzvbi-teletextdec.d" -Raw
        if (-not $dependencies.Contains("$ZvbiPrefix/include/libzvbi.h") -or
            $dependencies -match 'mingw64/include/libzvbi\.h') {
            throw 'FFmpeg did not compile against the verified experimental ZVBI header.'
        }
    }
    $symbols = @(& "$MsysRoot/mingw64/bin/nm.exe" --defined-only "$BuildDirectory/build/libavcodec/avcodec-63.dll")
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect native ARIB definitions.' }
    foreach ($symbol in @('arib_instance_new', 'arib_decode_buffer')) {
        if (-not ($symbols | Where-Object { $_ -match ('\sT\s' + $symbol + '$') })) { throw "Missing source-built ARIB definition: $symbol" }
    }
    Write-Output "Installing into isolated developer prefix: $BuildDirectory/install.log"
    Invoke-BuildCommand "cd $buildUnix/build; make install" 'install.log'
    $libraries = @('avcodec', 'avdevice', 'avfilter', 'avformat', 'avutil', 'swresample', 'swscale')
    foreach ($library in $libraries) {
        Copy-Item -LiteralPath "$prefix/bin/$library.lib" -Destination "$prefix/lib/$library.lib"
        if ((Get-FileHash -LiteralPath "$prefix/bin/$library.lib").Hash -ne (Get-FileHash -LiteralPath "$prefix/lib/$library.lib").Hash) { throw 'MSVC import library copy mismatch.' }
    }
    $inspector = Join-Path $PSScriptRoot 'get-native-runtime-dependencies.ps1'
    # Stage the exact package list before inspection, so no duplicate ZVBI location is allowed.
    foreach ($name in @($baseline.packages.runtime_dlls)) {
        $original = $baseline.runtime | Where-Object { $_.name -eq $name }
        $runtimeInput = "$MsysRoot/mingw64/bin/$name"
        if ($ZvbiPrefix -and $name -eq 'libzvbi-0.dll') {
            $runtimeInput = "$ZvbiPrefix/bin/$name"
            $original = $prefixInputs[-1].files | Where-Object { $_.name -eq 'bin/libzvbi-0.dll' }
        }
        if ((Get-FileHash -LiteralPath $runtimeInput).Hash -ne $original.sha256 -or (Get-Item -LiteralPath $runtimeInput).Length -ne $original.bytes) {
            throw "Native runtime input differs from recorded bytes: $name"
        }
        Copy-Item -LiteralPath $runtimeInput -Destination "$prefix/bin/$name"
        if ((Get-FileHash -LiteralPath "$prefix/bin/$name").Hash -ne $original.sha256) { throw 'Runtime DLL copy mismatch.' }
    }
    $graph = @(& $inspector -EntryPoints "$prefix/bin/ffmpeg.exe", "$prefix/bin/ffprobe.exe" -SearchDirectories "$prefix/bin" -ObjdumpExecutable "$MsysRoot/mingw64/bin/objdump.exe")
    if (@(Compare-Object ($baseline.runtime.name | Sort-Object) ($graph.name | Sort-Object)).Count) { throw 'Native runtime closure differs from the audited candidate.' }
    foreach ($file in $graph) {
        $original = $baseline.runtime | Where-Object { $_.name -eq $file.name }
        $actualImports = @($file.imports | ForEach-Object { $_.kind + ':' + $_.name } | Sort-Object)
        $expectedImports = @($original.imports | ForEach-Object { $_.kind + ':' + $_.name } | Sort-Object)
        if (@(Compare-Object $expectedImports $actualImports).Count) { throw "Native runtime import edges changed: $($file.name)" }
    }
    $staged = @(& $inspector -EntryPoints "$prefix/bin/ffmpeg.exe", "$prefix/bin/ffprobe.exe" -SearchDirectories "$prefix/bin" -ObjdumpExecutable "$MsysRoot/mingw64/bin/objdump.exe")
    Write-Evidence 'runtime.json' $staged
    Write-Evidence 'COMPLETE.json' ([ordered]@{ status = 'native-build-and-developer-staging-verified'; runtime_files = $staged.Count; prefix = $prefix; distribution_approved = $false })
    Write-Output "Native FFmpeg developer prefix: $prefix"
    Write-Output 'No package update, WSL, product replacement, publication or complete distribution approval. Separate runtime/media/performance tests remain required.'
}
finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
}
