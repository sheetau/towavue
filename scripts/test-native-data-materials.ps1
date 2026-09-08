[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$CacheDirectory,
    [Parameter(Mandatory = $true)][string]$SourceMaterialsDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$CacheDirectory = (Resolve-Path -LiteralPath $CacheDirectory).Path
$SourceMaterialsDirectory = (Resolve-Path -LiteralPath $SourceMaterialsDirectory).Path
$testRoot = Join-Path $repositoryRoot ('target/tmp/native-data-test-' + [guid]::NewGuid().ToString('N'))
$fixtureRoot = Join-Path $testRoot 'repository'
New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null
foreach ($name in @('scripts/prepare-native-data-materials.ps1','docs/native-unicode-table-audit.json',
    'docs/native-font-data-audit.json','docs/native-source-supplements.json',
    'third-party/unicode-data-20260908/LICENSE.txt','third-party/NATIVE-DATA-MATERIALS-README.txt')) {
    $path = Join-Path $fixtureRoot $name
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repositoryRoot $name) -Destination $path
}
$unicode = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-unicode-table-audit.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$font = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-font-data-audit.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$supplement = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-source-supplements.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$expected = @{}
$external = @{}
foreach ($group in @(
    @{prefix='pcre2/Unicode.tables/';records=$unicode.pcre2.data},
    @{prefix='pcre2/';records=$unicode.pcre2.generators},
    @{prefix='libxml2/';records=$unicode.libxml2.data},
    @{prefix='libunibreak/';records=$font.libunibreak.inputs}
)) {
    foreach ($record in $group.records) { $expected[$group.prefix + $record.name] = $record.sha256; $external[$group.prefix + $record.name] = $record.sha256 }
}
$pcre = $supplement.packages | Where-Object package -eq $unicode.pcre2.package
$license = $pcre.selected_documents | Where-Object package_notice -eq 'mingw64/share/licenses/pcre2/LICENCE.md'
$licenseRelative = $pcre.package + '/' + $license.name
$expected['pcre2/LICENCE.md'] = $license.sha256
$expected['UNICODE-LICENSE.txt'] = $unicode.notice.sha256
foreach ($pair in @(@('native-unicode-table-audit.json','docs/native-unicode-table-audit.json'),
    @('native-font-data-audit.json','docs/native-font-data-audit.json'),@('README.txt','third-party/NATIVE-DATA-MATERIALS-README.txt'))) {
    $expected[$pair[0]] = (Get-FileHash -LiteralPath (Join-Path $repositoryRoot $pair[1])).Hash
}
$generator = Join-Path $fixtureRoot 'scripts/prepare-native-data-materials.ps1'
$arguments = @{CacheDirectory=$CacheDirectory;SourceMaterialsDirectory=$SourceMaterialsDirectory}
function Assert-Output([string]$Directory) {
    if (@(Get-ChildItem -LiteralPath $Directory -Recurse -File).Count -ne 31) { throw 'Unexpected native data output count.' }
    foreach ($name in $expected.Keys) {
        if ((Get-FileHash -LiteralPath (Join-Path $Directory $name)).Hash -ne $expected[$name]) { throw "Native data output changed: $name" }
    }
    $marker = Get-Content -LiteralPath (Join-Path $Directory 'INPUTS.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($marker.files.Count -ne 30 -or $marker.source_supplement_sha256 -ne (Get-FileHash -LiteralPath (Join-Path $repositoryRoot 'docs/native-source-supplements.json')).Hash) { throw 'Incorrect native data completion inventory.' }
    foreach ($record in $marker.files) {
        $path = Join-Path $Directory $record.name
        if ((Get-FileHash -LiteralPath $path).Hash -ne $record.sha256 -or (Get-Item -LiteralPath $path).Length -ne $record.bytes) { throw 'Native data marker differs from output.' }
    }
}
function Assert-Rejected([string]$Message) {
    $rejected = $false
    try { & $generator @arguments | Out-Null }
    catch { if ($_.Exception.Message -notlike $Message) { throw }; $rejected = $true }
    if (-not $rejected) { throw "Invalid native data accepted: $Message" }
}
Push-Location $testRoot
try {
    foreach ($name in @('first','repeat')) {
        & $generator @arguments -OutputDirectory (Join-Path $testRoot $name) | Out-Null
        Assert-Output (Join-Path $testRoot $name)
    }
} finally { Pop-Location }
if ((Get-FileHash -LiteralPath (Join-Path $testRoot 'first/INPUTS.json')).Hash -ne (Get-FileHash -LiteralPath (Join-Path $testRoot 'repeat/INPUTS.json')).Hash) { throw 'Native data output is not repeatable.' }
$arguments.OutputDirectory = Join-Path $testRoot 'first'
Assert-Rejected 'Use a fresh native data output directory.'
$arguments.OutputDirectory = Join-Path $CacheDirectory 'nested-output'
Assert-Rejected 'Native data output overlaps an input.'
$arguments.OutputDirectory = Join-Path $SourceMaterialsDirectory 'nested-output'
Assert-Rejected 'Native data output overlaps an input.'
$paths = @()
foreach ($name in $external.Keys) {
    $path = Join-Path $testRoot ('cache/' + $name)
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $CacheDirectory $name) -Destination $path
    $paths += $path
}
foreach ($name in @('INPUTS.json',$licenseRelative)) {
    $path = Join-Path $testRoot ('source/' + $name)
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $SourceMaterialsDirectory $name) -Destination $path
}
$paths += Join-Path $testRoot ('source/' + $licenseRelative)
$paths += Join-Path $fixtureRoot $unicode.notice.name
$paths += Join-Path $fixtureRoot 'third-party/NATIVE-DATA-MATERIALS-README.txt'
$arguments.CacheDirectory = Join-Path $testRoot 'cache'
$arguments.SourceMaterialsDirectory = Join-Path $testRoot 'source'
$arguments.OutputDirectory = Join-Path $testRoot 'rejected'
foreach ($path in $paths) {
    $bytes = [IO.File]::ReadAllBytes($path)
    Move-Item -LiteralPath $path -Destination ($path + '.saved')
    try { Assert-Rejected 'Missing native data input:*' }
    finally { Move-Item -LiteralPath ($path + '.saved') -Destination $path }
    if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Missing native data input created output.' }
    if ($path.EndsWith('NATIVE-DATA-MATERIALS-README.txt')) { continue }
    $changed = [byte[]]$bytes.Clone()
    $changed[0] = $changed[0] -bxor 1
    [IO.File]::WriteAllBytes($path,$changed)
    $changedHash = (Get-FileHash -LiteralPath $path).Hash
    try {
        Assert-Rejected 'Native data checksum mismatch:*'
        if ((Get-FileHash -LiteralPath $path).Hash -ne $changedHash) { throw 'Corrupt native data input was overwritten.' }
    } finally { [IO.File]::WriteAllBytes($path,$bytes) }
    if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Rejected native data preflight created output.' }
}
$unicodePath = Join-Path $fixtureRoot 'docs/native-unicode-table-audit.json'
$original = [IO.File]::ReadAllBytes($unicodePath)
$utf8 = [Text.UTF8Encoding]::new($false)
foreach ($kind in @('count','path','duplicate','pcre_source','xml_source')) {
    $changed = $utf8.GetString($original) | ConvertFrom-Json
    switch ($kind) {
        'count' { $changed.pcre2.data = @(); $message = 'Incomplete native data inventory.' }
        'path' { $changed.pcre2.data[0].name = '../outside'; $message = 'Invalid native data path.' }
        'duplicate' { $changed.pcre2.data[1] = $changed.pcre2.data[0]; $message = 'Duplicate native data output path.' }
        'pcre_source' { $changed.pcre2.source_sha256 = '0' * 64; $message = 'Stale native data source mapping.' }
        'xml_source' { $changed.libxml2.source_sha256 = '0' * 64; $message = 'Stale native data source mapping.' }
    }
    [IO.File]::WriteAllText($unicodePath,($changed | ConvertTo-Json -Depth 15),$utf8)
    try { Assert-Rejected $message }
    finally { [IO.File]::WriteAllBytes($unicodePath,$original) }
    if (Test-Path -LiteralPath $arguments.OutputDirectory) { throw 'Invalid native data mapping created output.' }
}
$marker = Join-Path $arguments.SourceMaterialsDirectory 'INPUTS.json'
$original = [IO.File]::ReadAllBytes($marker)
try {
    [IO.File]::WriteAllText($marker,'{}',$utf8)
    Assert-Rejected 'Stale native data source supplement.'
} finally { [IO.File]::WriteAllBytes($marker,$original) }
$cachePath = $arguments.CacheDirectory
$link = Join-Path $testRoot 'linked-cache'
New-Item -ItemType Junction -Path $link -Target $cachePath | Out-Null
try {
    $arguments.CacheDirectory = $link
    Assert-Rejected 'Native data input links are not allowed.'
} finally {
    $arguments.CacheDirectory = $cachePath
    [IO.Directory]::Delete($link)
}
$generatorBytes = [IO.File]::ReadAllBytes($generator)
$copyLine = '    Copy-Item -LiteralPath $inputFile.path -Destination $path'
$generatorText = $utf8.GetString($generatorBytes)
if (-not $generatorText.Contains($copyLine)) { throw 'Missing copy-failure fixture target.' }
$failureLine = '    if ($inputFile.name -eq "UNICODE-LICENSE.txt") { throw "Injected data copy failure." }'
[IO.File]::WriteAllText($generator, $generatorText.Replace($copyLine,$failureLine + "`n" + $copyLine),$utf8)
$arguments.OutputDirectory = Join-Path $testRoot 'partial'
try { Assert-Rejected 'Injected data copy failure.' }
finally { [IO.File]::WriteAllBytes($generator,$generatorBytes) }
if (-not (Test-Path -LiteralPath $arguments.OutputDirectory) -or
    (Test-Path -LiteralPath (Join-Path $arguments.OutputDirectory 'INPUTS.json'))) { throw 'Failed native data copy has an invalid completion marker.' }
foreach ($name in $external.Keys) {
    if ((Get-FileHash -LiteralPath (Join-Path $CacheDirectory $name)).Hash -ne $external[$name]) { throw 'Original native data input changed.' }
}
Assert-Output (Join-Path $testRoot 'first')
Write-Output "Native data checks passed: 31 exact files, arbitrary cwd/repeat, 27 missing/corrupt input pairs, missing README, six mapping failures, links/overlap, failed-copy marker and original-output preservation. Evidence: $testRoot"
