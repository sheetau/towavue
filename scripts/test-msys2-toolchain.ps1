[CmdletBinding()]
param([switch]$IncludeMediaDependencies)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$getter = Join-Path $PSScriptRoot 'get-msys2-toolchain.ps1'
$cache = Join-Path $repositoryRoot 'vendor/msys2/packages-20260908'
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/msys2-toolchain-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$expectedCount = 235
if ($IncludeMediaDependencies) {
    $media = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/msys2-media-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $inventory.packages = @($inventory.packages) + @($media.packages)
    $expectedCount = 330
}
$testDirectory = Join-Path $repositoryRoot ('target/tmp/msys2-toolchain-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory | Out-Null
if ($inventory.packages.Count -ne $expectedCount -or @($inventory.packages.name | Sort-Object -Unique).Count -ne $expectedCount) {
    throw 'Unexpected or duplicate MSYS2 toolchain packages.'
}
& $getter -IncludeMediaDependencies:$IncludeMediaDependencies | Out-Null
$first = $inventory.packages[0]
$archiveName = ([uri]$first.url).Segments[-1]
$archivePath = Join-Path $cache $archiveName
$cacheInputs = @($inventory.packages | ForEach-Object {
    $path = Join-Path $cache ([uri]$_.url).Segments[-1]
    $path
    $path + '.sig'
})
$timestamps = @($cacheInputs | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTimeUtc.Ticks })
Push-Location -LiteralPath $testDirectory
try { & $getter -Download -IncludeMediaDependencies:$IncludeMediaDependencies | Out-Null }
finally { Pop-Location }
for ($index = 0; $index -lt $cacheInputs.Count; $index++) {
    $path = $cacheInputs[$index]
    if ((Get-Item -LiteralPath $path).LastWriteTimeUtc.Ticks -ne $timestamps[$index]) {
        throw 'Verified package cache was rewritten.'
    }
}

$missing = Join-Path $testDirectory 'missing'
$rejected = $false
try { & $getter -CacheDirectory $missing -IncludeMediaDependencies:$IncludeMediaDependencies | Out-Null }
catch {
    if ($_.Exception.Message -notlike 'MSYS2 package is missing:*') { throw }
    $rejected = $true
}
if (-not $rejected -or (Test-Path -LiteralPath $missing)) { throw 'Offline missing package cache was not rejected without writes.' }

$modified = Join-Path $testDirectory 'modified'
New-Item -ItemType Directory -Path $modified | Out-Null
$modifiedPath = Join-Path $modified $archiveName
Copy-Item -LiteralPath $archivePath -Destination $modifiedPath
$stream = [IO.File]::Open($modifiedPath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite)
try {
    $firstByte = $stream.ReadByte()
    $stream.Position = 0
    $stream.WriteByte($firstByte -bxor 1)
}
finally { $stream.Dispose() }
$modifiedHash = (Get-FileHash -LiteralPath $modifiedPath -Algorithm SHA256).Hash
$rejected = $false
try { & $getter -Download -CacheDirectory $modified -IncludeMediaDependencies:$IncludeMediaDependencies | Out-Null }
catch {
    if ($_.Exception.Message -notlike 'MSYS2 package checksum mismatch:*') { throw }
    $rejected = $true
}
if (-not $rejected -or (Get-FileHash -LiteralPath $modifiedPath -Algorithm SHA256).Hash -ne $modifiedHash -or
    @(Get-ChildItem -LiteralPath $modified).Count -ne 1) {
    throw 'Corrupt package was accepted, overwritten or followed by extra downloads.'
}
$signatureModified = Join-Path $testDirectory 'signature-modified'
New-Item -ItemType Directory -Path $signatureModified | Out-Null
Copy-Item -LiteralPath $archivePath -Destination (Join-Path $signatureModified $archiveName)
$signaturePath = Join-Path $signatureModified ($archiveName + '.sig')
Copy-Item -LiteralPath ($archivePath + '.sig') -Destination $signaturePath
$stream = [IO.File]::Open($signaturePath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite)
try {
    $firstByte = $stream.ReadByte()
    $stream.Position = 0
    $stream.WriteByte($firstByte -bxor 1)
}
finally { $stream.Dispose() }
$signatureHash = (Get-FileHash -LiteralPath $signaturePath -Algorithm SHA256).Hash
$rejected = $false
try { & $getter -Download -CacheDirectory $signatureModified -IncludeMediaDependencies:$IncludeMediaDependencies | Out-Null }
catch {
    if ($_.Exception.Message -notlike 'MSYS2 package checksum mismatch:*.sig') { throw }
    $rejected = $true
}
if (-not $rejected -or (Get-FileHash -LiteralPath $signaturePath -Algorithm SHA256).Hash -ne $signatureHash -or
    @(Get-ChildItem -LiteralPath $signatureModified).Count -ne 2) {
    throw 'Corrupt package signature was accepted, overwritten or followed by extra downloads.'
}
Write-Output "MSYS2 toolchain input checks passed: $expectedCount unique pinned archives and signatures, arbitrary cwd, cached reuse, missing offline input and same-size corrupt archive/signature rejection."
Write-Output "Artifacts: $testDirectory"
