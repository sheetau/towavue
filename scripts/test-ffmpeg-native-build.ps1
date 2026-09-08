[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$BuildDirectory,
    [Parameter(Mandatory = $true)][string]$MsysRoot,
    [Parameter(Mandatory = $true)][string]$ReferenceExecutable
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$BuildDirectory = (Resolve-Path -LiteralPath $BuildDirectory).Path
$MsysRoot = (Resolve-Path -LiteralPath $MsysRoot).Path
$ReferenceExecutable = (Resolve-Path -LiteralPath $ReferenceExecutable).Path
$prefix = Join-Path $BuildDirectory 'prefix'
$candidate = Join-Path $prefix 'bin/ffmpeg.exe'
if ($candidate -eq $ReferenceExecutable) { throw 'Use a distinct reference executable.' }
$inputs = Get-Content -LiteralPath (Join-Path $BuildDirectory 'build-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$runtime = Get-Content -LiteralPath (Join-Path $BuildDirectory 'runtime.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$complete = Get-Content -LiteralPath (Join-Path $BuildDirectory 'COMPLETE.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($complete.runtime_files -ne 94 -or $complete.distribution_approved -ne $false) { throw 'Unexpected native build completion record.' }
$testDirectory = Join-Path $repositoryRoot ('target/tmp/native-ffmpeg-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory | Out-Null
$savedPath = $env:PATH
$savedFfmpeg = $env:FFMPEG_DIR
try {
    $env:PATH = Join-Path $env:SystemRoot 'System32'
    $env:FFMPEG_DIR = $null
    $actual = @(& (Join-Path $PSScriptRoot 'get-native-runtime-dependencies.ps1') -EntryPoints $candidate, (Join-Path $prefix 'bin/ffprobe.exe') `
        -SearchDirectories (Join-Path $prefix 'bin') -ObjdumpExecutable (Join-Path $MsysRoot 'mingw64/bin/objdump.exe'))
    if (($actual | ConvertTo-Json -Depth 8) -cne ($runtime | ConvertTo-Json -Depth 8)) { throw 'Staged native runtime differs from its build evidence.' }
    foreach ($inputPrefix in $inputs.source_prefixes) {
        foreach ($file in $inputPrefix.files) {
            if ((Get-FileHash -LiteralPath (Join-Path $inputPrefix.prefix $file.name)).Hash -ne $file.sha256) { throw 'Native source-prefix input changed during or after build.' }
        }
    }
    foreach ($library in @('avcodec', 'avdevice', 'avfilter', 'avformat', 'avutil', 'swresample', 'swscale')) {
        if ((Get-FileHash -LiteralPath "$prefix/bin/$library.lib").Hash -ne (Get-FileHash -LiteralPath "$prefix/lib/$library.lib").Hash) { throw 'Staged MSVC import library differs.' }
    }
    Push-Location $testDirectory
    try {
        foreach ($mode in @('decoders', 'encoders', 'filters', 'demuxers', 'muxers', 'protocols', 'hwaccels')) {
            $sets = @{}
            foreach ($variant in @('reference', 'candidate')) {
                $executable = if ($variant -eq 'reference') { $ReferenceExecutable } else { $candidate }
                $lines = @(& $executable -hide_banner "-$mode")
                if ($LASTEXITCODE -ne 0) { throw "Cannot enumerate $variant $mode." }
                $pattern = switch ($mode) {
                    'protocols' { '^\s{2}([a-zA-Z0-9_]+)$' }
                    'hwaccels' { '^([a-zA-Z0-9_]+)$' }
                    default { '^\s+[A-Z.]{1,6}\s+([^\s=]+)\s' }
                }
                $sets[$variant] = @($lines | ForEach-Object { if ($_ -match $pattern) { $Matches[1] } } | Sort-Object -Unique)
                if ($sets[$variant].Count -eq 0) { throw "Empty feature enumeration: $mode" }
            }
            if (@(Compare-Object $sets.reference $sets.candidate).Count) { throw "Native $mode differ from the reference." }
            Write-Output "Matched ${mode}: $($sets.candidate.Count)"
        }
        & $candidate -nostdin -v error -n -f lavfi -i 'testsrc2=size=128x128:rate=1' -frames:v 3 -c:v ffv1 'three-frames.mkv'
        if ($LASTEXITCODE -ne 0) { throw 'Isolated native helper encode failed.' }
        $frames = & (Join-Path $prefix 'bin/ffprobe.exe') -v error -count_frames -select_streams v:0 -show_entries stream=nb_read_frames -of default=nw=1:nk=1 'three-frames.mkv'
        if ($LASTEXITCODE -ne 0 -or $frames -ne '3') { throw 'Isolated native helper did not decode three frames.' }
        & (Join-Path $PSScriptRoot 'test-ffmpeg-chromaprint.ps1') -ReferenceExecutable $ReferenceExecutable -CandidateExecutable $candidate
    }
    finally { Pop-Location }
}
finally {
    $env:PATH = $savedPath
    $env:FFMPEG_DIR = $savedFfmpeg
}

$builder = Join-Path $PSScriptRoot 'build-ffmpeg-native.ps1'
$arguments = @{
    MsysRoot = $MsysRoot; BuildDirectory = $BuildDirectory
    Aribb24Prefix = $inputs.source_prefixes[0].prefix; LcevcPrefix = $inputs.source_prefixes[1].prefix
    LibristPrefix = $inputs.source_prefixes[2].prefix; Uavs3dPrefix = $inputs.source_prefixes[3].prefix; VvencPrefix = $inputs.source_prefixes[4].prefix
}
$zvbiInput = @($inputs.source_prefixes | Where-Object { $_.package -eq 'zvbi-0.2' })
if ($zvbiInput.Count) { $arguments.ZvbiPrefix = $zvbiInput[0].prefix }
function Assert-Rejected([string]$Message) {
    $rejected = $false
    try { & $builder @arguments | Out-Null }
    catch {
        if ($_.Exception.Message -notlike $Message) { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw "Invalid native build input accepted: $Message" }
}
$completionHash = (Get-FileHash -LiteralPath (Join-Path $BuildDirectory 'COMPLETE.json')).Hash
Assert-Rejected 'Use a fresh FFmpeg build directory*'
$arguments.BuildDirectory = Join-Path $testDirectory 'rejected'
$arguments.SourceArchive = Join-Path $testDirectory 'missing.tar.gz'
Assert-Rejected 'Missing pinned FFmpeg source archive.'
$originalArchive = Join-Path $repositoryRoot 'vendor/ffmpeg/source-e47273f4d9227152dcbf543cebaf9e2430ddbcc4.tar.gz'
$arguments.SourceArchive = Join-Path $testDirectory 'corrupt.tar.gz'
$bytes = [IO.File]::ReadAllBytes($originalArchive)
$bytes[0] = $bytes[0] -bxor 1
[IO.File]::WriteAllBytes($arguments.SourceArchive, $bytes)
$corruptHash = (Get-FileHash -LiteralPath $arguments.SourceArchive).Hash
Assert-Rejected 'FFmpeg source archive checksum mismatch.'
if ((Get-FileHash -LiteralPath $arguments.SourceArchive).Hash -ne $corruptHash) { throw 'Corrupt source was overwritten.' }
$arguments.SourceArchive = $originalArchive
$wrongPrefix = Join-Path $testDirectory 'wrong-prefix'
New-Item -ItemType Directory -Path (Join-Path $wrongPrefix 'lib/pkgconfig') | Out-Null
Copy-Item -LiteralPath (Join-Path $arguments.Aribb24Prefix 'lib/libaribb24.a') -Destination (Join-Path $wrongPrefix 'lib')
# Leave the package descriptor absent so lookup resolves the rejected MSYS2 fallback.
# Copying a descriptor with an old prefix is not sufficient: pkgconf relocates it.
$arguments.Aribb24Prefix = $wrongPrefix
$before = @{}
foreach ($name in @('PATH', 'PKG_CONFIG_PATH', 'PKG_CONFIG_LIBDIR', 'BASH_ENV', 'CFLAGS')) { $before[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
try {
    $env:PKG_CONFIG_PATH = 'native-test-sentinel'
    $env:BASH_ENV = 'native-test-do-not-source'
    $env:CFLAGS = '-Dnative_test_do_not_inherit'
    Assert-Rejected 'Wrong native prefix selected: aribb24'
    if ($env:PATH -ne $before.PATH -or $env:PKG_CONFIG_PATH -ne 'native-test-sentinel' -or $env:BASH_ENV -ne 'native-test-do-not-source' -or
        $env:CFLAGS -ne '-Dnative_test_do_not_inherit' -or $env:PKG_CONFIG_LIBDIR -ne $before.PKG_CONFIG_LIBDIR) { throw 'Build failure changed caller environment.' }
}
finally { foreach ($name in $before.Keys) { [Environment]::SetEnvironmentVariable($name, $before[$name], 'Process') } }
if (Test-Path -LiteralPath $arguments.BuildDirectory) { throw 'Rejected native preflight created build output.' }
if ((Get-FileHash -LiteralPath (Join-Path $BuildDirectory 'COMPLETE.json')).Hash -ne $completionHash) { throw 'Existing native build was changed.' }
Write-Output 'Native staged identities, prefix inputs, feature sets, isolated helpers, fingerprints, input/output protection and failure environment restoration passed.'
Write-Output 'Hardware execution, release app performance, bit-identical rebuilds and distribution approval are not established by this test.'
