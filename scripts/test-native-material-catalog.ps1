[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$MaterialsDirectory,
    [string]$PackageDirectory,
    [string]$RecipeDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
if (-not $PackageDirectory) { $PackageDirectory = Join-Path $repositoryRoot 'vendor/msys2/packages-20260908' }
if (-not $RecipeDirectory) { $RecipeDirectory = Join-Path $repositoryRoot 'vendor/msys2/runtime-recipes-20260908' }
$MaterialsDirectory = (Resolve-Path -LiteralPath $MaterialsDirectory).Path
$PackageDirectory = (Resolve-Path -LiteralPath $PackageDirectory).Path
$RecipeDirectory = (Resolve-Path -LiteralPath $RecipeDirectory).Path
$testRoot = Join-Path $repositoryRoot ('target/tmp/native-catalog-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot | Out-Null
$generator = Join-Path $PSScriptRoot 'prepare-native-material-catalog.ps1'
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-material-catalog.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$audit = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-runtime-package-audit.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$recipes = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-runtime-recipes.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$packages = @($audit.packages | Where-Object name -ne $inventory.excluded_package)
$arguments = @{MaterialsDirectory=$MaterialsDirectory;PackageDirectory=$PackageDirectory;RecipeDirectory=$RecipeDirectory}
function Assert-Output([string]$Directory) {
    $records = Get-Content -LiteralPath (Join-Path $Directory 'FILES.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    if (@(Get-ChildItem -LiteralPath $Directory -Recurse -File -Force).Count -ne $records.Count + 2) { throw 'Unexpected catalog output count.' }
    foreach ($record in $records) {
        $path = Join-Path $Directory $record.name
        if ((Get-Item -LiteralPath $path).Length -ne $record.bytes -or (Get-FileHash -LiteralPath $path).Hash -ne $record.sha256) { throw "Catalog output differs: $($record.name)" }
    }
    foreach ($name in @('README.md','PACKAGES.md')) {
        $text = Get-Content -LiteralPath (Join-Path $Directory $name) -Raw -Encoding UTF8
        foreach ($match in [regex]::Matches($text, '\]\(([^)]+)\)')) {
            $link = $match.Groups[1].Value
            if ($link.StartsWith('https://')) { continue }
            if (-not (Test-Path -LiteralPath (Join-Path $Directory ([uri]::UnescapeDataString($link))) -PathType Leaf)) { throw "Broken catalog link: $link" }
        }
    }
    if (Test-Path -LiteralPath (Join-Path $Directory 'packages/mingw-w64-x86_64-zvbi')) { throw 'Excluded ZVBI package copied.' }
    foreach ($file in Get-ChildItem -LiteralPath $Directory -Recurse -File -Force | Where-Object Extension -in @('.dll','.exe','.a','.lib')) {
        # COPYING.LIB is an original license text, not a native library.
        $stream = [IO.File]::OpenRead($file.FullName)
        $header = [byte[]]::new(8)
        try { $length = $stream.Read($header,0,$header.Length) }
        finally { $stream.Dispose() }
        $magic = [Text.Encoding]::ASCII.GetString($header,0,$length)
        if ($magic.StartsWith('MZ') -or $magic.StartsWith('!<arch>')) { throw 'Runtime binary copied into catalog.' }
    }
}
function Assert-Rejected([string]$Message) {
    $rejected = $false
    try { & $generator @arguments | Out-Null }
    catch { if ($_.Exception.Message -notlike $Message) { throw }; $rejected = $true }
    if (-not $rejected) { throw "Invalid catalog accepted: $Message" }
}
Push-Location $testRoot
try {
    foreach ($name in @('first','repeat')) {
        $arguments.OutputDirectory = $name
        & $generator @arguments
        Assert-Output (Join-Path $testRoot $name)
    }
}
finally { Pop-Location }
$first = Join-Path $testRoot 'first'
foreach ($file in Get-ChildItem -LiteralPath $first -Recurse -File -Force) {
    $relative = $file.FullName.Substring($first.Length + 1)
    if ((Get-FileHash -LiteralPath $file.FullName).Hash -ne (Get-FileHash -LiteralPath (Join-Path (Join-Path $testRoot 'repeat') $relative)).Hash) { throw 'Repeated catalog differs.' }
}
$arguments.OutputDirectory = $first
Assert-Rejected 'Use a fresh native catalog output directory.'
$arguments.OutputDirectory = Join-Path $MaterialsDirectory 'rejected-catalog'
Assert-Rejected 'Native catalog output overlaps an input.'
if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Overlapping output created.' }

$fixture = Join-Path $testRoot 'fixture'
foreach ($name in @('scripts/prepare-native-material-catalog.ps1','docs/native-material-catalog.json','docs/native-runtime-package-audit.json','docs/native-runtime-recipes.json')) {
    $path = Join-Path $fixture $name
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repositoryRoot $name) -Destination $path
}
$fixtureInputs = @{}
foreach ($package in $packages) {
    $name = [uri]::UnescapeDataString(([uri]$package.archive_url).Segments[-1])
    $recipe = $recipes.recipes | Where-Object package -eq $package.name
    foreach ($pair in @(@($PackageDirectory,$name,'packages',$package.archive_sha256),@($RecipeDirectory,$recipe.name,'recipes',$recipe.sha256))) {
        $path = Join-Path $fixture ($pair[2] + '/' + $pair[1])
        New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $pair[0] $pair[1]) -Destination $path
        $fixtureInputs[$path] = @{original=(Join-Path $pair[0] $pair[1]);hash=$pair[3]}
    }
}
foreach ($kit in $inventory.kits) {
    $path = Join-Path $fixture ('materials/' + $kit.name)
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $MaterialsDirectory $kit.name) -Destination $path -Recurse
}
$generator = Join-Path $fixture 'scripts/prepare-native-material-catalog.ps1'
$arguments = @{MaterialsDirectory=(Join-Path $fixture 'materials');PackageDirectory=(Join-Path $fixture 'packages');RecipeDirectory=(Join-Path $fixture 'recipes');OutputDirectory=(Join-Path $testRoot 'rejected')}
foreach ($path in $fixtureInputs.Keys) {
    Move-Item -LiteralPath $path -Destination ($path + '.saved')
    try { Assert-Rejected 'Missing native catalog input:*' }
    finally { Move-Item -LiteralPath ($path + '.saved') -Destination $path }
    $stream = [IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite)
    try { $original = $stream.ReadByte(); $stream.Position = 0; $stream.WriteByte($original -bxor 1) }
    finally { $stream.Dispose() }
    try { Assert-Rejected 'Native catalog checksum mismatch:*' }
    finally { $stream = [IO.File]::OpenWrite($path); try { $stream.WriteByte($original) } finally { $stream.Dispose() } }
    if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Rejected input created catalog.' }
}
foreach ($kit in $inventory.kits) {
    $path = Join-Path $arguments.MaterialsDirectory ($kit.name + '/README.txt')
    Move-Item -LiteralPath $path -Destination ($path + '.saved')
    try { Assert-Rejected 'Native catalog kit mismatch:*' }
    finally { Move-Item -LiteralPath ($path + '.saved') -Destination $path }
    $stream = [IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite)
    try { $original = $stream.ReadByte(); $stream.Position = 0; $stream.WriteByte($original -bxor 1) }
    finally { $stream.Dispose() }
    try { Assert-Rejected 'Native catalog kit mismatch:*' }
    finally { $stream = [IO.File]::OpenWrite($path); try { $stream.WriteByte($original) } finally { $stream.Dispose() } }
}
$manifestPath = Join-Path $fixture 'docs/native-material-catalog.json'
$manifestBytes = [IO.File]::ReadAllBytes($manifestPath)
$utf8 = [Text.UTF8Encoding]::new($false)
foreach ($kind in @('excluded','coverage','duplicate','path','digest')) {
    $changed = $utf8.GetString($manifestBytes) | ConvertFrom-Json
    switch ($kind) {
        'excluded' { $changed.excluded_package = 'unrelated'; $message = 'Incomplete native material catalog.' }
        'coverage' { $changed.package_count = 70; $message = 'Native catalog package coverage mismatch.' }
        'duplicate' { $changed.kits[1] = $changed.kits[0]; $message = 'Duplicate native catalog kit.' }
        'path' { $changed.kits[0].name = '../outside'; $message = 'Invalid native catalog path.' }
        'digest' { $changed.kits[0].tree_sha256 = '0' * 64; $message = 'Native catalog kit mismatch:*' }
    }
    [IO.File]::WriteAllText($manifestPath,($changed | ConvertTo-Json -Depth 10),$utf8)
    try { Assert-Rejected $message }
    finally { [IO.File]::WriteAllBytes($manifestPath,$manifestBytes) }
    if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Invalid catalog manifest created output.' }
}
$auditPath = Join-Path $fixture 'docs/native-runtime-package-audit.json'
$auditBytes = [IO.File]::ReadAllBytes($auditPath)
$changed = $utf8.GetString($auditBytes) | ConvertFrom-Json
$changed.packages[0].package_notices[0].sha256 = '0' * 64
[IO.File]::WriteAllText($auditPath,($changed | ConvertTo-Json -Depth 15),$utf8)
$arguments.OutputDirectory = Join-Path $testRoot 'partial-copy'
try { Assert-Rejected 'Native catalog checksum mismatch:*' }
finally { [IO.File]::WriteAllBytes($auditPath,$auditBytes) }
if (-not (Test-Path -LiteralPath $arguments.OutputDirectory) -or (Test-Path -LiteralPath (Join-Path $arguments.OutputDirectory 'CATALOG.json'))) { throw 'Failed catalog copy has incorrect completion state.' }
foreach ($path in $fixtureInputs.Keys) {
    foreach ($check in @($path,$fixtureInputs[$path].original)) {
        if ((Get-FileHash -LiteralPath $check).Hash -ne $fixtureInputs[$path].hash) { throw 'Original catalog input changed.' }
    }
}
Assert-Output $first
Write-Output 'Native catalog checks passed: exact files/links, arbitrary cwd/repeat, 142 missing/corrupt package/recipe pairs, ten kit missing/corrupt pairs, five manifest failures, failed-copy marker protection and input/output preservation.'
