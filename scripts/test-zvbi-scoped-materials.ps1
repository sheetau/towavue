[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$SourceArchive,
    [Parameter(Mandatory = $true)][string]$ZvbiBuildDirectory,
    [Parameter(Mandatory = $true)][string]$FfmpegBuildDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$generator = Join-Path $PSScriptRoot 'prepare-zvbi-scoped-materials.ps1'
$manifest = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-zvbi-scoped-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$arguments = @{
    SourceArchive = (Resolve-Path -LiteralPath $SourceArchive).Path
    ZvbiBuildDirectory = (Resolve-Path -LiteralPath $ZvbiBuildDirectory).Path
    FfmpegBuildDirectory = (Resolve-Path -LiteralPath $FfmpegBuildDirectory).Path
}
$testDirectory = Join-Path $repositoryRoot ('target/tmp/zvbi-scoped-materials-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory | Out-Null
function Get-Hashes([string]$Directory) {
    @(Get-ChildItem -LiteralPath $Directory -Recurse -File | Sort-Object FullName | ForEach-Object {
        $_.FullName.Substring($Directory.Length + 1) + ':' + (Get-FileHash -LiteralPath $_.FullName).Hash
    })
}
$first = Join-Path $testDirectory 'first'
& $generator @arguments -OutputDirectory $first
$expected = Get-Hashes $first
if ($expected.Count -ne 13 -or @(Get-ChildItem -LiteralPath $first -Recurse -File | Where-Object { $_.Extension -in @('.dll', '.exe', '.a', '.ts') }).Count) {
    throw 'Expected thirteen source/provenance files without runtime binaries or recorded samples.'
}
$evidence = Get-Content -LiteralPath "$first/EVIDENCE.json" -Raw -Encoding UTF8 | ConvertFrom-Json
if ($evidence.distribution_approved -ne $false -or $evidence.original_source_files_after_patch.Count -ne 224 -or
    $evidence.ffmpeg_zvbi_prefix_inputs.Count -ne 6) { throw 'Unexpected scoped materials evidence.' }
$savedPath = $env:PATH
$savedLocation = (Get-Location).Path
Push-Location -LiteralPath $testDirectory
try { & $generator @arguments -OutputDirectory 'second' }
finally { Pop-Location }
if (@(Compare-Object $expected (Get-Hashes "$testDirectory/second")).Count) { throw 'Arbitrary-cwd material hashes differ.' }
$rejected = $false
try { & $generator @arguments -OutputDirectory $first | Out-Null }
catch {
    if ($_.Exception.Message -notlike 'Use a fresh scoped ZVBI materials directory*') { throw }
    $rejected = $true
}
if (-not $rejected) { throw 'Existing material output was accepted.' }

# All negative cases operate on copies, including a self-contained copy of the collector's repository inputs.
$fixture = Join-Path $testDirectory 'fixture'
$pairs = @(
    @{ Source = $arguments.SourceArchive; Target = "$fixture/source.tar" }
    @{ Source = "$($arguments.FfmpegBuildDirectory)/prefix/bin/libzvbi-0.dll"; Target = "$fixture/ffmpeg/prefix/bin/libzvbi-0.dll" }
)
foreach ($file in $manifest.prefix_files) {
    $pairs += @{ Source = "$($arguments.ZvbiBuildDirectory)/prefix/$($file.name)"; Target = "$fixture/zvbi/prefix/$($file.name)" }
}
foreach ($file in $evidence.original_source_files_after_patch) {
    $pairs += @{ Source = "$($arguments.ZvbiBuildDirectory)/source/$($file.name)"; Target = "$fixture/zvbi/source/$($file.name)" }
}
foreach ($name in @($manifest.repository_materials.name) + @('scripts/prepare-zvbi-scoped-materials.ps1', 'docs/native-zvbi-scoped-inputs.json', 'third-party/ZVBI-SCOPED-MATERIALS-README.txt')) {
    $pairs += @{ Source = (Join-Path $repositoryRoot $name); Target = "$fixture/repository/$name" }
}
$originalHashes = @{}
foreach ($pair in $pairs) {
    $originalHashes[$pair.Source] = (Get-FileHash -LiteralPath $pair.Source).Hash
    New-Item -ItemType Directory -Path (Split-Path -Parent $pair.Target) -Force | Out-Null
    Copy-Item -LiteralPath $pair.Source -Destination $pair.Target
}
$recordPath = "$fixture/ffmpeg/build-inputs.json"
$originalRecord = "$($arguments.FfmpegBuildDirectory)/build-inputs.json"
$originalHashes[$originalRecord] = (Get-FileHash -LiteralPath $originalRecord).Hash
$record = Get-Content -LiteralPath $originalRecord -Raw -Encoding UTF8 | ConvertFrom-Json
($record.source_prefixes | Where-Object { $_.package -eq 'zvbi-0.2' }).prefix = "$fixture/zvbi/prefix".Replace('\', '/')
$encoding = [Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllText($recordPath, ($record | ConvertTo-Json -Depth 12), $encoding)
$generator = "$fixture/repository/scripts/prepare-zvbi-scoped-materials.ps1"
$arguments = @{ SourceArchive = "$fixture/source.tar"; ZvbiBuildDirectory = "$fixture/zvbi"; FfmpegBuildDirectory = "$fixture/ffmpeg" }
& $generator @arguments -OutputDirectory "$testDirectory/copied-inputs" | Out-Null
if (@(Compare-Object $expected (Get-Hashes "$testDirectory/copied-inputs")).Count) { throw 'Copied-input evidence differs.' }
$rejectionCount = 0
function Assert-Rejected([string]$MessagePattern) {
    $destination = Join-Path $testDirectory ('rejected-' + $script:rejectionCount)
    $caught = $false
    try { & $generator @arguments -OutputDirectory $destination | Out-Null }
    catch {
        if ($_.Exception.Message -notlike $MessagePattern) { throw }
        $caught = $true
    }
    if (-not $caught -or (Test-Path -LiteralPath $destination)) { throw 'Invalid input was accepted or created material output.' }
    $script:rejectionCount++
}
$checkedPaths = @("$fixture/source.tar", "$fixture/ffmpeg/prefix/bin/libzvbi-0.dll", $recordPath)
$checkedPaths += @($manifest.repository_materials | ForEach-Object { "$fixture/repository/$($_.name)" })
$checkedPaths += @($manifest.prefix_files | ForEach-Object { "$fixture/zvbi/prefix/$($_.name)" })
$checkedPaths += @(@($manifest.changed_sources) + 'COPYING.md' | ForEach-Object { "$fixture/zvbi/source/$_" })
foreach ($path in $checkedPaths) {
    $bytes = [IO.File]::ReadAllBytes($path)
    $hash = (Get-FileHash -LiteralPath $path).Hash
    Move-Item -LiteralPath $path -Destination "$path.saved"
    try {
        Assert-Rejected 'Missing scoped ZVBI input:*'
        if (Test-Path -LiteralPath $path) { throw 'Missing input was recreated.' }
    }
    finally { Move-Item -LiteralPath "$path.saved" -Destination $path }
    $corrupt = $bytes.Clone()
    $corrupt[0] = $corrupt[0] -bxor 1
    [IO.File]::WriteAllBytes($path, $corrupt)
    $corruptHash = (Get-FileHash -LiteralPath $path).Hash
    try {
        if ($path -eq $recordPath) { Assert-Rejected '*Invalid JSON*' }
        else { Assert-Rejected 'Scoped ZVBI checksum mismatch:*' }
        if ((Get-FileHash -LiteralPath $path).Hash -ne $corruptHash) { throw 'Corrupt input was modified.' }
    }
    finally { [IO.File]::WriteAllBytes($path, $bytes) }
    if ((Get-FileHash -LiteralPath $path).Hash -ne $hash) { throw 'Copied input restoration failed.' }
}
$recordBytes = [IO.File]::ReadAllBytes($recordPath)
foreach ($kind in @('prefix', 'package', 'duplicate', 'name', 'bytes', 'sha256')) {
    $record = $encoding.GetString($recordBytes) | ConvertFrom-Json
    $zvbi = $record.source_prefixes | Where-Object { $_.package -eq 'zvbi-0.2' }
    switch ($kind) {
        'prefix' { $zvbi.prefix += '-other' }
        'package' { $zvbi.package = 'other' }
        'duplicate' { $zvbi.files = @($zvbi.files) + $zvbi.files[0] }
        'name' { $zvbi.files[0].name = 'other.dll' }
        'bytes' { $zvbi.files[0].bytes++ }
        'sha256' { $zvbi.files[0].sha256 = '0' * 64 }
    }
    [IO.File]::WriteAllText($recordPath, ($record | ConvertTo-Json -Depth 12), $encoding)
    $corruptHash = (Get-FileHash -LiteralPath $recordPath).Hash
    try {
        Assert-Rejected 'FFmpeg * scoped ZVBI *'
        if ((Get-FileHash -LiteralPath $recordPath).Hash -ne $corruptHash) { throw 'Rejected FFmpeg record was modified.' }
    }
    finally { [IO.File]::WriteAllBytes($recordPath, $recordBytes) }
}
foreach ($path in $originalHashes.Keys) {
    if ((Get-FileHash -LiteralPath $path).Hash -ne $originalHashes[$path]) { throw "Original input changed: $path" }
}
if (@(Compare-Object $expected (Get-Hashes $first)).Count -or $env:PATH -ne $savedPath -or (Get-Location).Path -ne $savedLocation) {
    throw 'Prior output, PATH or working directory changed.'
}
Write-Output "Scoped ZVBI material checks passed: 13 deterministic files, $rejectionCount invalid-input cases, existing-output and original-input preservation."
Write-Output "Evidence: $testDirectory"
