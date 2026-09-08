[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$generator = Join-Path $PSScriptRoot 'prepare-rust-runtime-notices.ps1'
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/rust-runtime-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$testDirectory = Join-Path $repositoryRoot ('target/tmp/rust-runtime-notice-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory | Out-Null
$firstPath = Join-Path $testDirectory 'first.zip'
$secondPath = Join-Path $testDirectory 'second.zip'
& $generator -OutputPath $firstPath | Out-Null
Push-Location -LiteralPath $testDirectory
try { & $generator -OutputPath $secondPath | Out-Null }
finally { Pop-Location }
$expectedHash = (Get-FileHash -LiteralPath $firstPath -Algorithm SHA256).Hash
if ($expectedHash -ne (Get-FileHash -LiteralPath $secondPath -Algorithm SHA256).Hash) {
    throw 'Repeated runtime notice generation produced different bytes.'
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::OpenRead($firstPath)
$hasher = [Security.Cryptography.SHA256]::Create()
try {
    $files = @($inventory.archives | ForEach-Object { $_.files })
    if ($files.Count -ne 36 -or $zip.Entries.Count -ne $files.Count + 2) { throw 'Unexpected runtime notice coverage.' }
    foreach ($file in $files) {
        $entries = @($zip.Entries | Where-Object FullName -eq $file.output)
        if ($entries.Count -ne 1 -or $entries[0].Length -ne $file.bytes) { throw "Wrong runtime notice entry: $($file.output)" }
        $stream = $entries[0].Open()
        try { $actual = [BitConverter]::ToString($hasher.ComputeHash($stream)).Replace('-', '').ToLowerInvariant() }
        finally { $stream.Dispose() }
        if ($actual -ne $file.sha256) { throw "Modified runtime notice bytes: $($file.output)" }
    }
    foreach ($name in @('README.txt', 'INPUTS.json')) {
        $entry = $zip.GetEntry($name)
        if (-not $entry) { throw "Missing runtime provenance: $name" }
        $stream = $entry.Open()
        $reader = [IO.StreamReader]::new($stream, [Text.UTF8Encoding]::new($false, $true))
        try { $text = $reader.ReadToEnd() }
        finally { $reader.Dispose() }
        if ($text.Contains("`r")) { throw "Expected LF runtime provenance: $name" }
        if ($name -eq 'INPUTS.json') {
            $embedded = $text | ConvertFrom-Json
            if ($embedded.archives.Count -ne 4) { throw 'Incomplete archive provenance.' }
        }
    }
}
finally {
    $hasher.Dispose()
    $zip.Dispose()
}

$missing = Join-Path $testDirectory 'missing'
$rejected = $false
try { & $generator -CacheDirectory $missing -OutputPath $firstPath | Out-Null }
catch {
    if ($_.Exception.Message -notlike 'Runtime archive is missing:*') { throw }
    $rejected = $true
}
if (-not $rejected) { throw 'Missing runtime archive was accepted.' }
if ((Get-FileHash -LiteralPath $firstPath -Algorithm SHA256).Hash -ne $expectedHash) {
    throw 'Missing input changed the existing runtime bundle.'
}

$modified = Join-Path $testDirectory 'modified'
New-Item -ItemType Directory -Path $modified | Out-Null
$archiveName = ([uri]$inventory.archives[0].url).Segments[-1]
$modifiedPath = Join-Path $modified $archiveName
Copy-Item -LiteralPath (Join-Path $repositoryRoot ('target/tmp/rust-runtime-materials/' + $archiveName)) -Destination $modifiedPath
$stream = [IO.File]::Open($modifiedPath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite)
try {
    $firstByte = $stream.ReadByte()
    $stream.Position = 0
    $stream.WriteByte($firstByte -bxor 1)
}
finally { $stream.Dispose() }
$modifiedHash = (Get-FileHash -LiteralPath $modifiedPath -Algorithm SHA256).Hash
$rejected = $false
try { & $generator -Download -CacheDirectory $modified -OutputPath $firstPath | Out-Null }
catch {
    if ($_.Exception.Message -notlike 'Runtime archive checksum mismatch:*') { throw }
    $rejected = $true
}
if (-not $rejected) { throw 'Modified runtime archive was accepted.' }
if ((Get-FileHash -LiteralPath $firstPath -Algorithm SHA256).Hash -ne $expectedHash -or
    (Get-FileHash -LiteralPath $modifiedPath -Algorithm SHA256).Hash -ne $modifiedHash) {
    throw 'Rejected input changed the existing bundle or corrupt cache.'
}
$inputPath = Join-Path $repositoryRoot 'docs/rust-runtime-inputs.json'
$inputHash = (Get-FileHash -LiteralPath $inputPath -Algorithm SHA256).Hash
$rejected = $false
try { & $generator -OutputPath $inputPath | Out-Null }
catch {
    if ($_.Exception.Message -ne 'The notice output must not replace an input.') { throw }
    $rejected = $true
}
if (-not $rejected -or (Get-FileHash -LiteralPath $inputPath -Algorithm SHA256).Hash -ne $inputHash) {
    throw 'Runtime input/output collision was not safely rejected.'
}
Write-Output 'Rust runtime notices passed: 36 exact original files, deterministic bytes, arbitrary cwd, missing/modified input rejection, existing output and corrupt cache preserved.'
Write-Output "Artifacts: $testDirectory"
