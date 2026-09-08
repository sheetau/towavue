[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateSet('chromaprint', 'openal', 'zvbi')][string]$Component,
    [Parameter(Mandatory = $true)][string]$PackageArchive,
    [Parameter(Mandatory = $true)][string]$Recipe,
    [Parameter(Mandatory = $true)][string]$SourceArchive,
    [Parameter(Mandatory = $true)][string]$RuntimeDll,
    [string]$LgplLicense,
    [string]$GplLicense,
    [string]$PatchDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$generator = Join-Path $PSScriptRoot 'prepare-native-package-materials.ps1'
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot "docs/native-$Component-inputs.json") -Raw -Encoding UTF8 | ConvertFrom-Json
$inputs = @{ Component = $Component }
$fileInputs = @('PackageArchive', 'Recipe', 'SourceArchive', 'RuntimeDll')
if ($inventory.lgpl) { $fileInputs += 'LgplLicense' }
if ($inventory.gpl) { $fileInputs += 'GplLicense' }
foreach ($name in $fileInputs) {
    $inputs[$name] = (Resolve-Path -LiteralPath (Get-Variable -Name $name -ValueOnly)).Path
}
if ($inventory.patches) { $inputs.PatchDirectory = (Resolve-Path -LiteralPath $PatchDirectory).Path }
$testDirectory = Join-Path $repositoryRoot ('target/tmp/' + $Component + '-materials-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory | Out-Null
$first = Join-Path $testDirectory 'first'
& $generator @inputs -OutputDirectory $first
$files = @(Get-ChildItem -LiteralPath $first -Recurse -File)
$expectedCount = @{ chromaprint = 13; openal = 18; zvbi = 17 }[$Component]
if ($files.Count -ne $expectedCount -or @($files | Where-Object { $_.Extension -in @('.dll', '.exe') }).Count) {
    throw "Expected exactly $expectedCount source/provenance/notice files and no binaries."
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
foreach ($inputName in $fileInputs) {
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
            if ($kind -eq 'missing' -and $message -notlike 'Missing native package material:*') { throw }
            if ($kind -eq 'corrupt' -and $message -notlike 'Native package material checksum mismatch:*') { throw }
            $rejected = $true
        }
        if (-not $rejected -or (Test-Path -LiteralPath $destination)) { throw "Invalid input created output: $inputName / $kind" }
        if ($kind -eq 'corrupt' -and (Get-FileHash -LiteralPath $path).Hash -ne $corruptHash) { throw 'Corrupt input was modified.' }
    }
}
foreach ($patch in $inventory.patches) {
    foreach ($kind in @('missing', 'corrupt')) {
        $arguments = $inputs.Clone()
        $directory = Join-Path $testDirectory ($patch.name + '-' + $kind)
        New-Item -ItemType Directory -Path $directory | Out-Null
        foreach ($other in $inventory.patches) {
            if ($other.name -ne $patch.name -or $kind -eq 'corrupt') {
                Copy-Item -LiteralPath (Join-Path $inputs.PatchDirectory $other.name) -Destination $directory
            }
        }
        $path = Join-Path $directory $patch.name
        if ($kind -eq 'corrupt') {
            $bytes = [IO.File]::ReadAllBytes($path)
            $bytes[0] = $bytes[0] -bxor 1
            [IO.File]::WriteAllBytes($path, $bytes)
            $corruptHash = (Get-FileHash -LiteralPath $path).Hash
        }
        $arguments.PatchDirectory = $directory
        $destination = $directory + '-output'
        $rejected = $false
        try { & $generator @arguments -OutputDirectory $destination | Out-Null }
        catch {
            $message = $_.Exception.Message
            if ($kind -eq 'missing' -and $message -notlike 'Missing native package material:*') { throw }
            if ($kind -eq 'corrupt' -and $message -notlike 'Native package material checksum mismatch:*') { throw }
            $rejected = $true
        }
        if (-not $rejected -or (Test-Path -LiteralPath $destination)) { throw "Invalid patch created output: $path" }
        if ($kind -eq 'corrupt' -and (Get-FileHash -LiteralPath $path).Hash -ne $corruptHash) { throw 'Corrupt patch was modified.' }
    }
}
foreach ($name in $expected.Keys) {
    if ((Get-FileHash -LiteralPath (Join-Path $first $name)).Hash -ne $expected[$name]) {
        throw "Rejected invocation changed prior output: $name"
    }
}
Write-Output "$Component materials checks passed: exact output, arbitrary cwd, existing-output protection, and all missing/corrupt input pairs."
