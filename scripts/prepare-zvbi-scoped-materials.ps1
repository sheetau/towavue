[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$SourceArchive,
    [Parameter(Mandatory = $true)][string]$ZvbiBuildDirectory,
    [Parameter(Mandatory = $true)][string]$FfmpegBuildDirectory,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $repositoryRoot 'docs/native-zvbi-scoped-inputs.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$SourceArchive = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($SourceArchive)
$ZvbiBuildDirectory = (Resolve-Path -LiteralPath $ZvbiBuildDirectory).Path
$FfmpegBuildDirectory = (Resolve-Path -LiteralPath $FfmpegBuildDirectory).Path
$OutputDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh scoped ZVBI materials directory; existing output is preserved.' }
function Assert-Bytes([string]$Path, $Record) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing scoped ZVBI input: $Path" }
    if ((Get-Item -LiteralPath $Path).Length -ne $Record.bytes -or (Get-FileHash -LiteralPath $Path).Hash -ne $Record.sha256) {
        throw "Scoped ZVBI checksum mismatch: $Path"
    }
}
foreach ($file in $manifest.repository_materials) { Assert-Bytes (Join-Path $repositoryRoot $file.name) $file }
$original = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-zvbi-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
Assert-Bytes $SourceArchive $original.source
foreach ($file in $manifest.prefix_files) { Assert-Bytes (Join-Path "$ZvbiBuildDirectory/prefix" $file.name) $file }
Assert-Bytes "$FfmpegBuildDirectory/prefix/bin/libzvbi-0.dll" $manifest.prefix_files[0]
if (-not (Test-Path -LiteralPath "$FfmpegBuildDirectory/build-inputs.json" -PathType Leaf)) {
    throw 'Missing scoped ZVBI input: FFmpeg build-inputs.json'
}
$buildInputs = Get-Content -LiteralPath "$FfmpegBuildDirectory/build-inputs.json" -Raw -Encoding UTF8 | ConvertFrom-Json
$recorded = @($buildInputs.source_prefixes | Where-Object { $_.package -eq 'zvbi-0.2' })
if ($recorded.Count -ne 1 -or $recorded[0].prefix -ne "$ZvbiBuildDirectory/prefix".Replace('\', '/') -or
    $recorded[0].files.Count -ne $manifest.prefix_files.Count) { throw 'FFmpeg does not identify this scoped ZVBI prefix.' }
foreach ($file in $manifest.prefix_files) {
    $match = @($recorded[0].files | Where-Object { $_.name -eq $file.name -and $_.bytes -eq $file.bytes -and $_.sha256 -eq $file.sha256 })
    if ($match.Count -ne 1) { throw 'FFmpeg recorded different scoped ZVBI input bytes.' }
}

# Replay only the hash-pinned patch in a separate repository, never in the actual build source.
$scratch = Join-Path $repositoryRoot ('target/tmp/zvbi-source-replay-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
& (Join-Path $env:SystemRoot 'System32/tar.exe') -xf $SourceArchive -C $scratch
if ($LASTEXITCODE -ne 0) { throw 'Scoped ZVBI source extraction failed.' }
$sourceFiles = @(Get-ChildItem -LiteralPath $scratch -Recurse -File -Force | Sort-Object FullName)
if ($sourceFiles.Count -ne $manifest.original_source_files) { throw 'Unexpected original ZVBI source file count.' }
$before = @{}
foreach ($file in $sourceFiles) { $before[$file.FullName] = (Get-FileHash -LiteralPath $file.FullName).Hash }
& git -c core.autocrlf=false -C $scratch init --quiet
if ($LASTEXITCODE -ne 0) { throw 'Cannot isolate ZVBI patch replay.' }
& git -c core.autocrlf=false -C $scratch apply (Join-Path $repositoryRoot 'third-party/patches/zvbi-no-program-id.patch')
if ($LASTEXITCODE -ne 0) { throw 'Scoped ZVBI patch replay failed.' }
$changed = @()
$sourceEvidence = @(foreach ($file in $sourceFiles) {
    $name = $file.FullName.Substring($scratch.Length + 1).Replace('\', '/')
    $hash = (Get-FileHash -LiteralPath $file.FullName).Hash
    if ($hash -ne $before[$file.FullName]) { $changed += $name }
    $record = [ordered]@{ name = $name; bytes = (Get-Item -LiteralPath $file.FullName).Length; sha256 = $hash.ToLowerInvariant() }
    Assert-Bytes (Join-Path "$ZvbiBuildDirectory/source" $name) $record
    $record
})
if (@(Compare-Object @($manifest.changed_sources) $changed).Count) { throw 'Unexpected scoped ZVBI patch targets.' }

New-Item -ItemType Directory -Path "$OutputDirectory/sources", "$OutputDirectory/generated" | Out-Null
Copy-Item -LiteralPath $SourceArchive -Destination "$OutputDirectory/sources/$($original.source.name)"
foreach ($file in $manifest.repository_materials) {
    $target = Join-Path $OutputDirectory $file.name
    New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repositoryRoot $file.name) -Destination $target
    Assert-Bytes $target $file
}
Copy-Item -LiteralPath "$ZvbiBuildDirectory/prefix/include/libzvbi.h" -Destination "$OutputDirectory/generated/libzvbi.h"
foreach ($name in @('COPYING.md', 'NEWS', 'README.md')) { Copy-Item -LiteralPath (Join-Path $scratch $name) -Destination "$OutputDirectory/sources/$name" }
Copy-Item -LiteralPath $manifestPath -Destination "$OutputDirectory/SCOPED-INPUTS.json"
Copy-Item -LiteralPath (Join-Path $repositoryRoot 'third-party/ZVBI-SCOPED-MATERIALS-README.txt') -Destination "$OutputDirectory/README.txt"
$evidence = [ordered]@{
    schema_version = 1; original_source_sha256 = $original.source.sha256
    original_source_files_after_patch = $sourceEvidence
    source_check_scope = 'All original tracked inputs, including uncompiled files; generated Autotools/system-header closure is not proved.'
    ffmpeg_zvbi_prefix_inputs = @($manifest.prefix_files)
    distribution_approved = $false
}
Assert-Bytes "$OutputDirectory/sources/$($original.source.name)" $original.source
Assert-Bytes "$OutputDirectory/generated/libzvbi.h" $manifest.prefix_files[1]
[IO.File]::WriteAllText("$OutputDirectory/EVIDENCE.json", ($evidence | ConvertTo-Json -Depth 8) + "`n", [Text.UTF8Encoding]::new($false))
Write-Output "Verified scoped ZVBI source materials: $OutputDirectory"
Write-Output '224 original inputs match patch replay; six prefix inputs match FFmpeg records and the staged DLL. No binaries copied or distribution approval.'
