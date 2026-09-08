[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$getter = Join-Path $PSScriptRoot 'get-msys2-bootstrap.ps1'
$cache = Join-Path $repositoryRoot 'vendor/msys2/bootstrap-20260611'
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/msys2-bootstrap-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$testDirectory = Join-Path $repositoryRoot ('target/tmp/msys2-bootstrap-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory | Out-Null
& $getter | Out-Null
$timestamps = @($inventory.files | ForEach-Object { (Get-Item -LiteralPath (Join-Path $cache $_.name)).LastWriteTimeUtc.Ticks })
Push-Location -LiteralPath $testDirectory
try { & $getter -Download | Out-Null }
finally { Pop-Location }
for ($index = 0; $index -lt $inventory.files.Count; $index++) {
    if ((Get-Item -LiteralPath (Join-Path $cache $inventory.files[$index].name)).LastWriteTimeUtc.Ticks -ne $timestamps[$index]) {
        throw 'A verified bootstrap cache file was rewritten.'
    }
}
$archive = $inventory.files | Where-Object { $_.name.EndsWith('.sfx.exe') }
$upstreamChecksum = (Get-Content -LiteralPath (Join-Path $cache ($archive.name + '.sha256')) -Raw -Encoding UTF8).Trim() -split '\s+'
if ($upstreamChecksum.Count -ne 2 -or $upstreamChecksum[0] -ne $archive.sha256 -or $upstreamChecksum[1] -ne $archive.name) {
    throw 'Upstream bootstrap checksum disagrees with the pinned archive.'
}

$missing = Join-Path $testDirectory 'missing'
$rejected = $false
try { & $getter -CacheDirectory $missing | Out-Null }
catch {
    if ($_.Exception.Message -notlike 'MSYS2 bootstrap file is missing:*') { throw }
    $rejected = $true
}
if (-not $rejected -or (Test-Path -LiteralPath $missing)) { throw 'Missing offline bootstrap cache was not rejected without writes.' }

$modified = Join-Path $testDirectory 'modified'
New-Item -ItemType Directory -Path $modified | Out-Null
$modifiedPath = Join-Path $modified $archive.name
Copy-Item -LiteralPath (Join-Path $cache $archive.name) -Destination $modifiedPath
$stream = [IO.File]::Open($modifiedPath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite)
try {
    $firstByte = $stream.ReadByte()
    $stream.Position = 0
    $stream.WriteByte($firstByte -bxor 1)
}
finally { $stream.Dispose() }
$modifiedHash = (Get-FileHash -LiteralPath $modifiedPath -Algorithm SHA256).Hash
$rejected = $false
try { & $getter -Download -CacheDirectory $modified | Out-Null }
catch {
    if ($_.Exception.Message -notlike 'MSYS2 bootstrap checksum mismatch:*') { throw }
    $rejected = $true
}
if (-not $rejected -or (Get-FileHash -LiteralPath $modifiedPath -Algorithm SHA256).Hash -ne $modifiedHash) {
    throw 'Corrupt bootstrap cache was accepted or overwritten.'
}
if (@(Get-ChildItem -LiteralPath $modified).Count -ne 1) { throw 'Rejected bootstrap cache gained extra files.' }
Write-Output 'MSYS2 bootstrap checks passed: pinned files, upstream checksum, unchanged cache, arbitrary cwd, missing offline input and same-size corrupt input rejection.'
Write-Output "Artifacts: $testDirectory"
