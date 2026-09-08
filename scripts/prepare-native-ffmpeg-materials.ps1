[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$SourceDirectory,
    [Parameter(Mandatory = $true)][string]$FfmpegBuildDirectory,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $repositoryRoot 'docs/native-ffmpeg-material-inputs.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$SourceDirectory = (Resolve-Path -LiteralPath $SourceDirectory).Path
$FfmpegBuildDirectory = (Resolve-Path -LiteralPath $FfmpegBuildDirectory).Path
$OutputDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh FFmpeg materials output directory.' }
$utf8 = [Text.UTF8Encoding]::new($false)
function Assert-Path([string]$Name) {
    if (-not $Name -or $Name -match '(^/|[\\:"]|(^|/)\.\.?(/|$))') { throw 'Invalid FFmpeg material path.' }
}
function Assert-File([string]$Path, $Record) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing FFmpeg material: $Path" }
    $item = Get-Item -LiteralPath $Path
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'FFmpeg material links are not allowed.' }
    if ($item.Length -ne $Record.bytes -or (Get-FileHash -LiteralPath $Path).Hash -ne $Record.sha256) { throw "FFmpeg material checksum mismatch: $Path" }
}
function Assert-Separate([string]$Root) {
    $rootPath = [IO.Path]::GetFullPath($Root).TrimEnd('\','/')
    $outputPath = $OutputDirectory.TrimEnd('\','/')
    if ($rootPath -eq $outputPath -or
        $rootPath.StartsWith($outputPath + '\',[StringComparison]::OrdinalIgnoreCase) -or
        $outputPath.StartsWith($rootPath + '\',[StringComparison]::OrdinalIgnoreCase)) { throw 'FFmpeg material output overlaps an input.' }
}
function Get-TreeDigest($Files) {
    $lookup = @{}
    foreach ($file in $Files) {
        if ($lookup.ContainsKey($file.name)) { throw 'Duplicate FFmpeg source member.' }
        $lookup[$file.name] = $file
    }
    $names = [string[]]@($lookup.Keys)
    [Array]::Sort($names,[StringComparer]::Ordinal)
    $lines = @($names | ForEach-Object { "$_ $($lookup[$_].bytes) $($lookup[$_].sha256)" })
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($sha.ComputeHash($utf8.GetBytes(($lines -join "`n") + "`n"))).Replace('-','').ToLowerInvariant() }
    finally { $sha.Dispose() }
}
if ($manifest.schema_version -ne 1 -or $manifest.sources.Count -ne 6 -or $manifest.observed_records.Count -ne 2) { throw 'Incomplete FFmpeg materials manifest.' }
Assert-Separate $SourceDirectory
Assert-Separate $FfmpegBuildDirectory
$copies = @{}
foreach ($source in $manifest.sources) {
    foreach ($name in @($source.name,$source.root,$source.archive.name) + @($source.notices.name)) { Assert-Path $name }
    foreach ($record in @($source.archive) + @($source.patches)) {
        Assert-Path $record.name
        Assert-File (Join-Path $SourceDirectory $record.name) $record
        $destination = if ($record.name -eq $source.archive.name) { 'sources/' + $record.name } else { 'patches/' + $record.name }
        if ($copies.ContainsKey($destination)) { throw 'Duplicate FFmpeg material destination.' }
        $copies[$destination] = @{path=(Join-Path $SourceDirectory $record.name);record=$record}
    }
}
foreach ($record in $manifest.repository_materials) {
    Assert-Path $record.name
    Assert-File (Join-Path $repositoryRoot $record.name) $record
    $copies[$record.name] = @{path=(Join-Path $repositoryRoot $record.name);record=$record}
}
foreach ($record in $manifest.observed_records) {
    Assert-Path $record.name
    Assert-File (Join-Path $FfmpegBuildDirectory $record.name) $record
}
$build = Get-Content -LiteralPath "$FfmpegBuildDirectory/build-inputs.json" -Raw -Encoding UTF8 | ConvertFrom-Json
$runtime = Get-Content -LiteralPath "$FfmpegBuildDirectory/runtime.json" -Raw -Encoding UTF8 | ConvertFrom-Json
$counts = $manifest.expected_counts
if ($build.source_sha256 -ne $manifest.sources[0].archive.sha256 -or
    $build.configure_flags.Count -ne $counts.configure_flags -or $build.packages.Count -ne $counts.packages -or
    $build.source_prefixes.Count -ne $counts.prefixes -or @($build.source_prefixes.files).Count -ne $counts.prefix_files -or
    $runtime.Count -ne $counts.runtime_files) { throw 'FFmpeg observed input coverage mismatch.' }
$flags = @($build.configure_flags)
$prefixes = @(foreach ($prefix in $build.source_prefixes) {
    Assert-Separate $prefix.prefix
    foreach ($file in $prefix.files) { Assert-Path $file.name; Assert-File (Join-Path $prefix.prefix $file.name) $file }
    $token = '@PREFIX:' + $prefix.package + '@'
    $flags = @($flags | ForEach-Object { $_.Replace($prefix.prefix,$token) })
    [ordered]@{package=$prefix.package;prefix=$token;files=$prefix.files}
})
$flags = @($flags | ForEach-Object { if ($_ -like '--prefix=*') { '--prefix=@FFMPEG_PREFIX@' } else { $_ } })
$portableBuild = [ordered]@{schema_version=1;normalization='Only six prefix paths and the FFmpeg output prefix are replaced by explicit placeholders.';source_sha256=$build.source_sha256;configure_flags=$flags;packages=$build.packages;source_prefixes=$prefixes}
$portableRuntime = @(foreach ($file in $runtime) {
    Assert-Path $file.name
    Assert-File (Join-Path "$FfmpegBuildDirectory/prefix/bin" $file.name) $file
    [ordered]@{name=$file.name;bytes=$file.bytes;sha256=$file.sha256;imports=@($file.imports | Select-Object name,kind)}
})
$buildJson = ConvertTo-Json -InputObject $portableBuild -Depth 10
$runtimeJson = ConvertTo-Json -InputObject $portableRuntime -Depth 8
if ($buildJson -match '[A-Za-z]:[/\\]' -or $runtimeJson -match '[A-Za-z]:[/\\]') { throw 'Unnormalized machine path in FFmpeg materials.' }

# Replay exact archives/patches in isolated repositories; never modify build sources.
$scratch = Join-Path $repositoryRoot ('target/tmp/ffmpeg-material-replay-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$tar = Join-Path $env:SystemRoot 'System32/tar.exe'
$sourceEvidence = @(foreach ($source in $manifest.sources) {
    $archive = Join-Path $SourceDirectory $source.archive.name
    $names = @(& $tar -tf $archive)
    if ($LASTEXITCODE -ne 0) { throw 'FFmpeg source inventory failed.' }
    $types = @(& $tar -tvf $archive)
    if ($LASTEXITCODE -ne 0 -or $names.Count -ne $source.entries -or $types.Count -ne $names.Count) { throw 'FFmpeg source entry count mismatch.' }
    $regular = @()
    for ($index = 0; $index -lt $names.Count; $index++) {
        Assert-Path $names[$index]
        if (-not $names[$index].StartsWith($source.root + '/', [StringComparison]::Ordinal) -or $types[$index][0] -notin @('-','d')) { throw 'Unexpected FFmpeg source archive member.' }
        if ($types[$index][0] -eq '-') { $regular += $names[$index].Substring($source.root.Length + 1) }
    }
    if ($regular.Count -ne $source.regular_files) { throw 'FFmpeg regular source count mismatch.' }
    $replay = Join-Path $scratch $source.name
    New-Item -ItemType Directory -Path $replay | Out-Null
    & $tar -xf $archive -C $replay
    if ($LASTEXITCODE -ne 0) { throw 'FFmpeg source extraction failed.' }
    $root = Join-Path $replay $source.root
    foreach ($notice in $source.notices) {
        $original = Join-Path $root $notice.name
        Assert-File $original $notice
        $destination = 'notices/' + $source.name + '/' + $notice.name
        $preserved = Join-Path $scratch $destination
        New-Item -ItemType Directory -Path (Split-Path -Parent $preserved) -Force | Out-Null
        Copy-Item -LiteralPath $original -Destination $preserved
        $copies[$destination] = @{path=$preserved;record=$notice}
    }
    if ($source.patches.Count) {
        & git -C $root -c core.autocrlf=false init --quiet
        if ($LASTEXITCODE -ne 0) { throw 'Cannot isolate FFmpeg dependency patch replay.' }
        foreach ($patch in $source.patches) {
            & git -C $root -c core.autocrlf=false apply --unidiff-zero (Join-Path $SourceDirectory $patch.name)
            if ($LASTEXITCODE -ne 0) { throw 'FFmpeg dependency patch replay failed.' }
        }
    }
    $files = @(foreach ($name in $regular) {
        $path = Join-Path $root $name
        [pscustomobject]@{name=$name;bytes=(Get-Item -LiteralPath $path).Length;sha256=(Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant()}
    })
    $digest = Get-TreeDigest $files
    if ($digest -ne $source.patched_tree_sha256) { throw 'FFmpeg patched source tree mismatch.' }
    [ordered]@{name=$source.name;regular_files=$files.Count;patched_tree_sha256=$digest}
})

New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
foreach ($name in $copies.Keys) {
    $target = Join-Path $OutputDirectory $name
    New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
    Copy-Item -LiteralPath $copies[$name].path -Destination $target
    Assert-File $target $copies[$name].record
}
Copy-Item -LiteralPath (Join-Path $repositoryRoot 'third-party/FFMPEG-NATIVE-MATERIALS-README.txt') -Destination "$OutputDirectory/README.txt"
Copy-Item -LiteralPath $manifestPath -Destination "$OutputDirectory/INPUTS.json"
[IO.File]::WriteAllText("$OutputDirectory/BUILD.json", $buildJson + "`n", $utf8)
[IO.File]::WriteAllText("$OutputDirectory/RUNTIME.json", $runtimeJson + "`n", $utf8)
$evidence = [ordered]@{schema_version=1;sources=$sourceEvidence;source_check_scope=$manifest.source_check_scope;observed_record_hashes=$manifest.observed_records;verified_counts=$counts;distribution_approved=$false}
# Last file: no partial source/notice copy is marked complete.
[IO.File]::WriteAllText("$OutputDirectory/EVIDENCE.json", ($evidence | ConvertTo-Json -Depth 8) + "`n", $utf8)
Write-Output "Verified FFmpeg source materials: $OutputDirectory"
Write-Output 'Six archives, six patches, 12509 original source files; 130 prefix inputs and 94 runtime hashes checked. No binaries copied or distribution approved.'
