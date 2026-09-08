[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$SourceDirectory,
    [Parameter(Mandatory = $true)][string]$FfmpegBuildDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$SourceDirectory = (Resolve-Path -LiteralPath $SourceDirectory).Path
$FfmpegBuildDirectory = (Resolve-Path -LiteralPath $FfmpegBuildDirectory).Path
$manifestPath = Join-Path $repositoryRoot 'docs/native-ffmpeg-material-inputs.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$generator = Join-Path $PSScriptRoot 'prepare-native-ffmpeg-materials.ps1'
$testRoot = Join-Path $repositoryRoot ('target/tmp/native-ffmpeg-material-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot | Out-Null
$utf8 = [Text.UTF8Encoding]::new($false)
$arguments = @{SourceDirectory=$SourceDirectory;FfmpegBuildDirectory=$FfmpegBuildDirectory}
function Get-Hashes([string]$Root) {
    $records = @{}
    foreach ($file in Get-ChildItem -LiteralPath $Root -Recurse -File -Force) {
        $records[$file.FullName.Substring($Root.Length + 1)] = (Get-FileHash -LiteralPath $file.FullName).Hash
    }
    return $records
}
function Assert-Rejected([string]$Message) {
    $rejected = $false
    try { & $generator @arguments | Out-Null }
    catch { if ($_.Exception.Message -notlike $Message) { throw }; $rejected = $true }
    if (-not $rejected) { throw "Invalid FFmpeg material accepted: $Message" }
}
$originalInputs = @{}
foreach ($source in $manifest.sources) {
    foreach ($record in @($source.archive) + @($source.patches)) { $originalInputs[(Join-Path $SourceDirectory $record.name)] = $record.sha256 }
}
foreach ($record in $manifest.repository_materials) { $originalInputs[(Join-Path $repositoryRoot $record.name)] = $record.sha256 }
foreach ($record in $manifest.observed_records) { $originalInputs[(Join-Path $FfmpegBuildDirectory $record.name)] = $record.sha256 }
$oldPath = $env:PATH
$oldCwd = (Get-Location).Path
Push-Location $testRoot
try {
    foreach ($name in @('first','repeat')) {
        $arguments.OutputDirectory = $name
        & $generator @arguments
    }
}
finally { Pop-Location }
$first = Join-Path $testRoot 'first'
$hashes = Get-Hashes $first
$repeated = Get-Hashes (Join-Path $testRoot 'repeat')
if ($hashes.Count -ne $repeated.Count) { throw 'Repeated FFmpeg material count differs.' }
foreach ($name in $hashes.Keys) { if ($hashes[$name] -ne $repeated[$name]) { throw "Repeated FFmpeg materials differ: $name" } }
$build = Get-Content -LiteralPath "$first/BUILD.json" -Raw | ConvertFrom-Json
$runtime = Get-Content -LiteralPath "$first/RUNTIME.json" -Raw | ConvertFrom-Json
$evidence = Get-Content -LiteralPath "$first/EVIDENCE.json" -Raw | ConvertFrom-Json
if ($build.configure_flags.Count -ne 87 -or $build.packages.Count -ne 330 -or @($build.source_prefixes.files).Count -ne 130 -or
    $runtime.Count -ne 94 -or $evidence.distribution_approved -ne $false -or ($evidence.sources | Measure-Object regular_files -Sum).Sum -ne 12509) { throw 'Incomplete portable FFmpeg evidence.' }
foreach ($name in @('BUILD.json','RUNTIME.json','INPUTS.json','EVIDENCE.json')) {
    if ((Get-Content -LiteralPath (Join-Path $first $name) -Raw) -match '(?<![A-Za-z])[A-Za-z]:[/\\]') { throw 'Machine path leaked into material evidence.' }
}
if (@(Get-ChildItem -LiteralPath $first -Recurse -File | Where-Object Extension -in @('.dll','.exe','.a','.lib')).Count) { throw 'Native binary copied into FFmpeg materials.' }
$arguments.OutputDirectory = $first
Assert-Rejected 'Use a fresh FFmpeg materials output directory.'
$arguments.OutputDirectory = Join-Path $SourceDirectory 'overlap-test-output'
Assert-Rejected 'FFmpeg material output overlaps an input.'

$fixture = Join-Path $testRoot 'fixture'
$fixtureSource = Join-Path $fixture 'inputs'
$fixtureBuild = Join-Path $fixture 'build'
$fixtureManifest = Join-Path $fixture 'docs/native-ffmpeg-material-inputs.json'
$fixtureGenerator = Join-Path $fixture 'scripts/prepare-native-ffmpeg-materials.ps1'
New-Item -ItemType Directory -Path "$fixture/docs", "$fixture/scripts", $fixtureSource, $fixtureBuild | Out-Null
Copy-Item -LiteralPath $manifestPath -Destination $fixtureManifest
Copy-Item -LiteralPath $generator -Destination $fixtureGenerator
$fixtureInputs = @()
foreach ($source in $manifest.sources) {
    foreach ($record in @($source.archive) + @($source.patches)) {
        $target = Join-Path $fixtureSource $record.name
        Copy-Item -LiteralPath (Join-Path $SourceDirectory $record.name) -Destination $target
        $fixtureInputs += $target
    }
}
foreach ($record in $manifest.repository_materials) {
    $target = Join-Path $fixture $record.name
    New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repositoryRoot $record.name) -Destination $target
    $fixtureInputs += $target
}
foreach ($record in $manifest.observed_records) {
    $target = Join-Path $fixtureBuild $record.name
    Copy-Item -LiteralPath (Join-Path $FfmpegBuildDirectory $record.name) -Destination $target
    $fixtureInputs += $target
}
$generator = $fixtureGenerator
$arguments = @{SourceDirectory=$fixtureSource;FfmpegBuildDirectory=$fixtureBuild;OutputDirectory=(Join-Path $testRoot 'rejected')}
foreach ($path in $fixtureInputs) {
    Move-Item -LiteralPath $path -Destination ($path + '.held')
    try { Assert-Rejected 'Missing FFmpeg material:*' }
    finally { Move-Item -LiteralPath ($path + '.held') -Destination $path }
    $stream = [IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite)
    try { $original = $stream.ReadByte(); $stream.Position = 0; $stream.WriteByte($original -bxor 1) }
    finally { $stream.Dispose() }
    $corruptHash = (Get-FileHash -LiteralPath $path).Hash
    try {
        Assert-Rejected 'FFmpeg material checksum mismatch:*'
        if ((Get-FileHash -LiteralPath $path).Hash -ne $corruptHash) { throw 'Collector repaired invalid input.' }
    }
    finally {
        $stream = [IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Write)
        try { $stream.WriteByte($original) }
        finally { $stream.Dispose() }
    }
    if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Invalid input created material output.' }
}
# Semantic manifest failures use the unchanged observed candidate, not modified binaries.
$arguments.FfmpegBuildDirectory = $FfmpegBuildDirectory
$originalManifest = [IO.File]::ReadAllText($fixtureManifest,$utf8)
foreach ($case in @('path','duplicate','count','tree','notice')) {
    $changed = $originalManifest | ConvertFrom-Json
    $message = switch ($case) {
        'path' { $changed.sources[0].root = '../outside'; 'Invalid FFmpeg material path.' }
        'duplicate' { $changed.sources[1].archive = $changed.sources[0].archive; 'Duplicate FFmpeg material destination.' }
        'count' { $changed.expected_counts.runtime_files = 93; 'FFmpeg observed input coverage mismatch.' }
        'tree' { $changed.sources[0].patched_tree_sha256 = '0' * 64; 'FFmpeg patched source tree mismatch.' }
        'notice' { $changed.sources[0].notices[0].sha256 = '0' * 64; 'FFmpeg material checksum mismatch:*' }
    }
    [IO.File]::WriteAllText($fixtureManifest,($changed | ConvertTo-Json -Depth 12),$utf8)
    try { Assert-Rejected $message }
    finally { [IO.File]::WriteAllText($fixtureManifest,$originalManifest,$utf8) }
    if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Failed source replay created output.' }
}
foreach ($path in $originalInputs.Keys) { if ((Get-FileHash -LiteralPath $path).Hash -ne $originalInputs[$path]) { throw 'Original FFmpeg input changed.' } }
$after = Get-Hashes $first
foreach ($name in $hashes.Keys) { if ($hashes[$name] -ne $after[$name]) { throw 'Existing FFmpeg materials changed.' } }
if ($env:PATH -ne $oldPath -or (Get-Location).Path -ne $oldCwd) { throw 'Collector leaked process environment.' }
Write-Output "PASS: exact/arbitrary-cwd/repeated output; $($fixtureInputs.Count) missing/corrupt pairs; five manifest failures; input/output preservation."
Write-Output "Evidence: $testRoot"
