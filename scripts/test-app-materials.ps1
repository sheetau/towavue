[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$RustNotices,
    [Parameter(Mandatory = $true)][string]$RuntimeNotices,
    [Parameter(Mandatory = $true)][string]$Executable
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $repositoryRoot 'docs/app-material-inputs.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$generator = Join-Path $PSScriptRoot 'prepare-app-materials.ps1'
$arguments = @{RustNotices=(Resolve-Path -LiteralPath $RustNotices).Path;RuntimeNotices=(Resolve-Path -LiteralPath $RuntimeNotices).Path;Executable=(Resolve-Path -LiteralPath $Executable).Path}
$testRoot = Join-Path $repositoryRoot ('target/tmp/app-material-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot | Out-Null
$originalInputs = @{}
foreach ($key in $arguments.Keys) { $originalInputs[$arguments[$key]] = (Get-FileHash -LiteralPath $arguments[$key]).Hash }
foreach ($file in @($manifest.runtime_inventory) + @($manifest.repository_materials)) { $originalInputs[(Join-Path $repositoryRoot $file.name)] = $file.sha256 }
function Assert-Rejected([string]$Message) {
    $rejected = $false
    try { & $generator @arguments | Out-Null }
    catch { if ($_.Exception.Message -notlike $Message) { throw }; $rejected = $true }
    if (-not $rejected) { throw "Invalid application materials accepted: $Message" }
}
$oldPath = $env:PATH
$oldCwd = (Get-Location).Path
Push-Location $testRoot
try {
    foreach ($name in @('first','repeat')) { $arguments.OutputDirectory = $name; & $generator @arguments }
}
finally { Pop-Location }
$first = Join-Path $testRoot 'first'
$files = @(Get-ChildItem -LiteralPath $first -Recurse -File -Force)
if ($files.Count -ne 11) { throw 'Unexpected application material file count.' }
$hashes = @{}
foreach ($file in $files) {
    $name = $file.FullName.Substring($first.Length + 1)
    $hashes[$name] = (Get-FileHash -LiteralPath $file.FullName).Hash
    if ($hashes[$name] -ne (Get-FileHash -LiteralPath (Join-Path "$testRoot/repeat" $name)).Hash) { throw 'Repeated application materials differ.' }
    if ($file.Extension -in @('.dll','.exe','.a','.lib','.ttf','.otf')) { throw 'Unexpected binary in application materials.' }
}
$evidence = Get-Content -LiteralPath "$first/EVIDENCE.json" -Raw | ConvertFrom-Json
if ($evidence.distribution_approved -ne $false -or $evidence.runtime_notice_count -ne 18 -or $evidence.dependency_count -ne 146 -or
    $evidence.candidate.sha256 -ne $manifest.candidate.sha256) { throw 'Incorrect application material evidence.' }
$arguments.OutputDirectory = $first
Assert-Rejected 'Use a fresh application materials output directory.'
$fixture = Join-Path $testRoot 'fixture'
New-Item -ItemType Directory -Path "$fixture/docs", "$fixture/scripts", "$fixture/inputs" | Out-Null
$fixtureInputs = @()
foreach ($key in @('RustNotices','RuntimeNotices','Executable')) {
    $target = Join-Path "$fixture/inputs" (Split-Path -Leaf $arguments[$key])
    Copy-Item -LiteralPath $arguments[$key] -Destination $target
    $arguments[$key] = $target
    $fixtureInputs += $target
}
foreach ($file in @($manifest.runtime_inventory) + @($manifest.repository_materials)) {
    $target = Join-Path $fixture $file.name
    New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repositoryRoot $file.name) -Destination $target
    $fixtureInputs += $target
}
$fixtureManifest = Join-Path $fixture 'docs/app-material-inputs.json'
Copy-Item -LiteralPath $manifestPath -Destination $fixtureManifest
Copy-Item -LiteralPath $generator -Destination "$fixture/scripts/prepare-app-materials.ps1"
$generator = Join-Path $fixture 'scripts/prepare-app-materials.ps1'
$arguments.OutputDirectory = Join-Path $testRoot 'rejected'
foreach ($path in $fixtureInputs) {
    Move-Item -LiteralPath $path -Destination ($path + '.held')
    try { Assert-Rejected 'Missing application material:*' }
    finally { Move-Item -LiteralPath ($path + '.held') -Destination $path }
    $stream = [IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite)
    try { $original = $stream.ReadByte(); $stream.Position = 0; $stream.WriteByte($original -bxor 1) }
    finally { $stream.Dispose() }
    $corrupt = (Get-FileHash -LiteralPath $path).Hash
    try {
        Assert-Rejected 'Application material checksum mismatch:*'
        if ((Get-FileHash -LiteralPath $path).Hash -ne $corrupt) { throw 'Collector modified invalid application input.' }
    }
    finally {
        $stream = [IO.File]::OpenWrite($path)
        try { $stream.WriteByte($original) }
        finally { $stream.Dispose() }
    }
    if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Rejected application input created output.' }
}
$utf8 = [Text.UTF8Encoding]::new($false)
$manifestText = [IO.File]::ReadAllText($fixtureManifest,$utf8)
foreach ($case in @('version','target','dependencies','runtime_count','path','duplicate')) {
    $changed = $manifestText | ConvertFrom-Json
    $message = switch ($case) {
        'version' { $changed.version = '1.97.1'; 'Application dependency/toolchain mapping is stale.' }
        'target' { $changed.target = 'x86_64-pc-windows-gnu'; 'Application runtime selection is stale.' }
        'dependencies' { $changed.dependency_count = 145; 'Application dependency/toolchain mapping is stale.' }
        'runtime_count' { $changed.runtime_notice_count = 36; 'Application runtime selection is stale.' }
        'path' { $changed.repository_materials[0].name = '../outside'; 'Invalid application material path.' }
        'duplicate' { $changed.repository_materials[1] = $changed.repository_materials[0]; 'Duplicate application material.' }
    }
    [IO.File]::WriteAllText($fixtureManifest,($changed | ConvertTo-Json -Depth 6),$utf8)
    try { Assert-Rejected $message }
    finally { [IO.File]::WriteAllText($fixtureManifest,$manifestText,$utf8) }
    if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Invalid application mapping created output.' }
}
foreach ($path in $originalInputs.Keys) { if ((Get-FileHash -LiteralPath $path).Hash -ne $originalInputs[$path]) { throw 'Original application input changed.' } }
foreach ($name in $hashes.Keys) { if ((Get-FileHash -LiteralPath (Join-Path $first $name)).Hash -ne $hashes[$name]) { throw 'Existing application output changed.' } }
if ($env:PATH -ne $oldPath -or (Get-Location).Path -ne $oldCwd) { throw 'Application collector changed the process environment.' }
Write-Output "PASS: exact/arbitrary-cwd/repeated output; $($fixtureInputs.Count) missing/corrupt pairs; six mapping failures; original inputs/output/environment preserved."
Write-Output "Evidence: $testRoot"
