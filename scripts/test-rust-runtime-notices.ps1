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
$appCache = Join-Path $testDirectory 'app-cache'
New-Item -ItemType Directory -Path $appCache | Out-Null
$appArchives = @($inventory.archives | Where-Object used_by -eq 'towavue')
foreach ($archive in $appArchives) {
    $name = ([uri]$archive.url).Segments[-1]
    Copy-Item -LiteralPath (Join-Path $repositoryRoot ('target/tmp/rust-runtime-materials/' + $name)) -Destination (Join-Path $appCache $name)
}
$appPath = Join-Path $testDirectory 'app.zip'
& $generator -Scope towavue -CacheDirectory $appCache -OutputPath $appPath | Out-Null
Push-Location $testDirectory
try { & $generator -Scope towavue -CacheDirectory 'app-cache' -OutputPath 'app-repeat.zip' | Out-Null }
finally { Pop-Location }
$appHash = (Get-FileHash -LiteralPath $appPath).Hash
if ($appHash -ne (Get-FileHash -LiteralPath (Join-Path $testDirectory 'app-repeat.zip')).Hash) { throw 'Repeated towavue runtime notices differ.' }
$zip = [IO.Compression.ZipFile]::OpenRead($appPath)
$hasher = [Security.Cryptography.SHA256]::Create()
try {
    $files = @($appArchives.files)
    if ($files.Count -ne 18 -or $zip.Entries.Count -ne 20) { throw 'Unexpected towavue runtime notice coverage.' }
    foreach ($entry in $zip.Entries) {
        if ($entry.FullName -notin @('README.txt','INPUTS.json') -and -not $entry.FullName.StartsWith('1.98.0-x86_64-pc-windows-msvc/')) { throw 'Foreign runtime material in towavue output.' }
    }
    foreach ($file in $files) {
        $entries = @($zip.Entries | Where-Object FullName -eq $file.output)
        if ($entries.Count -ne 1 -or $entries[0].Length -ne $file.bytes) { throw 'Incomplete towavue runtime notice.' }
        $stream = $entries[0].Open()
        try { $hash = [BitConverter]::ToString($hasher.ComputeHash($stream)).Replace('-','').ToLowerInvariant() }
        finally { $stream.Dispose() }
        if ($hash -ne $file.sha256) { throw 'Modified towavue runtime notice.' }
    }
    $reader = [IO.StreamReader]::new($zip.GetEntry('INPUTS.json').Open(),[Text.UTF8Encoding]::new($false,$true))
    try { $embedded = $reader.ReadToEnd() | ConvertFrom-Json }
    finally { $reader.Dispose() }
    if ($embedded.archives.Count -ne 2 -or @($embedded.archives | Where-Object { $_.used_by -ne 'towavue' -or $_.target -ne 'x86_64-pc-windows-msvc' }).Count) { throw 'Foreign runtime provenance in towavue output.' }
}
finally { $hasher.Dispose(); $zip.Dispose() }
foreach ($archive in $appArchives) {
    $path = Join-Path $appCache ([uri]$archive.url).Segments[-1]
    Move-Item -LiteralPath $path -Destination ($path + '.held')
    $rejected = $false
    try {
        try { & $generator -Scope towavue -CacheDirectory $appCache -OutputPath $appPath | Out-Null }
        catch { if ($_.Exception.Message -notlike 'Runtime archive is missing:*') { throw }; $rejected = $true }
    }
    finally { Move-Item -LiteralPath ($path + '.held') -Destination $path }
    if (-not $rejected) { throw 'Missing towavue runtime input accepted.' }
    $stream = [IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite)
    try { $original = $stream.ReadByte(); $stream.Position = 0; $stream.WriteByte($original -bxor 1) }
    finally { $stream.Dispose() }
    $corruptHash = (Get-FileHash -LiteralPath $path).Hash
    $rejected = $false
    try {
        try { & $generator -Scope towavue -Download -CacheDirectory $appCache -OutputPath $appPath | Out-Null }
        catch { if ($_.Exception.Message -notlike 'Runtime archive checksum mismatch:*') { throw }; $rejected = $true }
        if (-not $rejected -or (Get-FileHash -LiteralPath $path).Hash -ne $corruptHash) { throw 'Corrupt towavue runtime input accepted or replaced.' }
    }
    finally {
        $stream = [IO.File]::OpenWrite($path)
        try { $stream.WriteByte($original) }
        finally { $stream.Dispose() }
    }
    if ((Get-FileHash -LiteralPath $appPath).Hash -ne $appHash) { throw 'Rejected towavue input changed existing output.' }
}
Write-Output 'Rust runtime notices passed: 36 original legacy files and 18 app-only originals; deterministic/arbitrary-cwd outputs; no excluded GNU inputs needed for app scope; missing/corrupt inputs and existing outputs preserved.'
Write-Output "Artifacts: $testDirectory"
