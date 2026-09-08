[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$PackageArchive,
    [Parameter(Mandatory = $true)][string]$Recipe,
    [Parameter(Mandatory = $true)][string]$SourceArchive,
    [Parameter(Mandatory = $true)][string]$RuntimeDll,
    [Parameter(Mandatory = $true)][string]$LgplLicense
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$generator = Join-Path $PSScriptRoot 'prepare-chromaprint-materials.ps1'
$inputs = @{}
foreach ($name in @('PackageArchive', 'Recipe', 'SourceArchive', 'RuntimeDll', 'LgplLicense')) {
    $inputs[$name] = (Resolve-Path -LiteralPath (Get-Variable -Name $name -ValueOnly)).Path
}
$testDirectory = Join-Path $repositoryRoot ('target/tmp/chromaprint-materials-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory | Out-Null
$first = Join-Path $testDirectory 'first'
& $generator @inputs -OutputDirectory $first
$files = @(Get-ChildItem -LiteralPath $first -Recurse -File)
if ($files.Count -ne 13 -or @($files | Where-Object { $_.Extension -in @('.dll', '.exe') }).Count) {
    throw 'Expected exactly 13 source/provenance/notice files and no binaries.'
}
$expected = @{}
foreach ($file in $files) {
    $expected[$file.FullName.Substring($first.Length + 1)] = (Get-FileHash -LiteralPath $file.FullName).Hash
}
Push-Location $testDirectory
try { & $generator @inputs -OutputDirectory 'second' }
finally { Pop-Location }
foreach ($name in $expected.Keys) {
    if ((Get-FileHash -LiteralPath (Join-Path "$testDirectory/second" $name)).Hash -ne $expected[$name]) {
        throw "Arbitrary-cwd output mismatch: $name"
    }
}
$rejected = $false
try { & $generator @inputs -OutputDirectory $first | Out-Null }
catch {
    if ($_.Exception.Message -notlike 'Use a fresh materials output directory*') { throw }
    $rejected = $true
}
if (-not $rejected) { throw 'Existing output was accepted.' }
foreach ($inputName in $inputs.Keys) {
    foreach ($kind in @('missing', 'corrupt')) {
        $arguments = $inputs.Clone()
        $path = Join-Path $testDirectory "$inputName-$kind"
        if ($kind -eq 'corrupt') {
            Copy-Item -LiteralPath $inputs[$inputName] -Destination $path
            $bytes = [IO.File]::ReadAllBytes($path)
            $bytes[0] = $bytes[0] -bxor 1
            [IO.File]::WriteAllBytes($path, $bytes)
            $corruptHash = (Get-FileHash -LiteralPath $path).Hash
        }
        $arguments[$inputName] = $path
        $destination = Join-Path $testDirectory "$inputName-$kind-output"
        $rejected = $false
        try { & $generator @arguments -OutputDirectory $destination | Out-Null }
        catch {
            $message = $_.Exception.Message
            if ($kind -eq 'missing' -and $message -notlike 'Missing Chromaprint material:*') { throw }
            if ($kind -eq 'corrupt' -and $message -notlike 'Chromaprint material checksum mismatch:*') { throw }
            $rejected = $true
        }
        if (-not $rejected -or (Test-Path -LiteralPath $destination)) { throw "Invalid input created output: $inputName / $kind" }
        if ($kind -eq 'corrupt' -and (Get-FileHash -LiteralPath $path).Hash -ne $corruptHash) { throw 'Corrupt input was modified.' }
    }
}
foreach ($name in $expected.Keys) {
    if ((Get-FileHash -LiteralPath (Join-Path $first $name)).Hash -ne $expected[$name]) {
        throw "Rejected invocation changed prior output: $name"
    }
}
Write-Output 'Chromaprint materials checks passed: exact output, arbitrary cwd, existing-output protection, and five missing/corrupt input pairs.'
