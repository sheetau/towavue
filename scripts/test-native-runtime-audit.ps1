[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$RuntimeDirectory,
    [Parameter(Mandatory = $true)][string]$MsysRoot
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$RuntimeDirectory = (Resolve-Path -LiteralPath $RuntimeDirectory).Path
$MsysRoot = (Resolve-Path -LiteralPath $MsysRoot).Path
$generator = Join-Path $PSScriptRoot 'prepare-native-runtime-audit.ps1'
$baseline = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-runtime-package-audit.json') -Raw -Encoding UTF8
$inventory = $baseline | ConvertFrom-Json
if ($inventory.runtime.Count -ne 94 -or $inventory.packages.Count -ne 72) { throw 'Incomplete native runtime baseline.' }
$testDirectory = Join-Path $repositoryRoot ('target/tmp/runtime-audit-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory | Out-Null
$savedPath = $env:PATH
$arguments = @{ RuntimeDirectory = $RuntimeDirectory; MsysRoot = $MsysRoot }
Push-Location $testDirectory
try { & $generator @arguments -OutputDirectory 'valid' }
finally { Pop-Location }
$valid = Join-Path $testDirectory 'valid'
$actual = Get-Content -LiteralPath "$valid/INDEX.json" -Raw -Encoding UTF8
if ($actual.Replace("`r`n", "`n").TrimEnd() -cne $baseline.Replace("`r`n", "`n").TrimEnd()) {
    throw 'Runtime/package graph differs from the pinned audit baseline.'
}
$outputHash = (Get-FileHash -LiteralPath "$valid/INDEX.json").Hash
foreach ($package in $inventory.packages) {
    foreach ($notice in $package.package_notices) {
        $path = Join-Path "$valid/$($package.name)" $notice.name
        if ((Get-FileHash -LiteralPath $path).Hash -ne $notice.sha256) { throw "Package notice hash mismatch: $path" }
    }
}
if ($env:PATH -ne $savedPath) { throw 'Audit did not restore PATH.' }

foreach ($case in @('existing', 'missing-cache', 'corrupt-package', 'mismatched-dll')) {
    $options = $arguments.Clone()
    $destination = Join-Path $testDirectory $case
    if ($case -eq 'existing') { $destination = $valid }
    elseif ($case -eq 'missing-cache') { $options.CacheDirectory = Join-Path $testDirectory 'absent-cache' }
    elseif ($case -eq 'corrupt-package') {
        $cache = Join-Path $testDirectory 'corrupt-cache'
        New-Item -ItemType Directory -Path $cache | Out-Null
        foreach ($package in $inventory.packages) {
            $name = ([uri]$package.archive_url).Segments[-1]
            Copy-Item -LiteralPath (Join-Path "$repositoryRoot/vendor/msys2/packages-20260908" $name) -Destination $cache
        }
        $damaged = Join-Path $cache 'mingw-w64-x86_64-chromaprint-1.6.1-1-any.pkg.tar.zst'
        $bytes = [IO.File]::ReadAllBytes($damaged)
        $bytes[0] = $bytes[0] -bxor 1
        [IO.File]::WriteAllBytes($damaged, $bytes)
        $damagedHash = (Get-FileHash -LiteralPath $damaged).Hash
        $options.CacheDirectory = $cache
    }
    else {
        $runtime = Join-Path $testDirectory 'changed-runtime'
        New-Item -ItemType Directory -Path $runtime | Out-Null
        foreach ($file in $inventory.runtime) { Copy-Item -LiteralPath (Join-Path $RuntimeDirectory $file.name) -Destination $runtime }
        $damaged = Join-Path $runtime 'libchromaprint.dll'
        $bytes = [IO.File]::ReadAllBytes($damaged)
        # Change only a COFF timestamp byte, preserving a parseable import graph.
        $timestamp = [BitConverter]::ToInt32($bytes, 0x3c) + 8
        $bytes[$timestamp] = $bytes[$timestamp] -bxor 1
        [IO.File]::WriteAllBytes($damaged, $bytes)
        $damagedHash = (Get-FileHash -LiteralPath $damaged).Hash
        $options.RuntimeDirectory = $runtime
    }
    $expected = switch ($case) {
        'existing' { 'Use a fresh runtime audit directory*' }
        'missing-cache' { 'Missing runtime package archive:*' }
        'corrupt-package' { 'Runtime package archive checksum mismatch:*' }
        'mismatched-dll' { 'Runtime DLL does not match its pinned package: libchromaprint.dll' }
    }
    $rejected = $false
    try { & $generator @options -OutputDirectory $destination | Out-Null }
    catch {
        if ($_.Exception.Message -notlike $expected) { throw }
        $rejected = $true
    }
    if (-not $rejected -or $env:PATH -ne $savedPath) { throw "Invalid audit input was accepted or PATH changed: $case" }
    if ($case -in @('missing-cache', 'corrupt-package') -and (Test-Path -LiteralPath $destination)) { throw 'Archive preflight failure created output.' }
    if ($case -eq 'mismatched-dll' -and (Test-Path -LiteralPath "$destination/INDEX.json")) { throw 'Mismatched DLL produced an audit index.' }
    if ($case -in @('corrupt-package', 'mismatched-dll') -and (Get-FileHash -LiteralPath $damaged).Hash -ne $damagedHash) { throw 'Rejected input was modified.' }
    if ((Get-FileHash -LiteralPath "$valid/INDEX.json").Hash -ne $outputHash) { throw 'Previous audit index was changed.' }
    Write-Output "PASS runtime audit rejection: $case"
}
Write-Output 'Pinned runtime/package graph, package notice hashes, arbitrary cwd, PATH restoration and rejection tests passed.'
