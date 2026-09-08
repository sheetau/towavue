[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$testRoot = Join-Path $repositoryRoot ('target/tmp/material-archive-test-' + [guid]::NewGuid().ToString('N'))
$inputRoot = Join-Path $testRoot 'fixture'
New-Item -ItemType Directory -Path (Join-Path $inputRoot 'catalog') | Out-Null
$utf8 = [Text.UTF8Encoding]::new($false)
$japanese = ([string][char]0x65e5) + [char]0x672c + [char]0x8a9e
$unicodeName = 'catalog/' + $japanese + ' space & dollar $ #.txt'
$contents = @{
    'INPUTS.json'='{"fixture":true}'
    'BINDING.json'='{"fixture":true}'
    'START-HERE.html'='<p>Archive test fixture, not candidate validation.</p>'
    'catalog/.BUILDINFO'="Hidden fixture record.`r`n"
    $unicodeName="Original bytes.`r`n"
}
$hashes = @{}
foreach ($name in $contents.Keys) {
    $file = Join-Path $inputRoot $name
    [IO.File]::WriteAllText($file,$contents[$name],$utf8)
    $hashes[$name] = (Get-FileHash -LiteralPath $file).Hash
}
[IO.File]::SetAttributes((Join-Path $inputRoot 'catalog/.BUILDINFO'),[IO.FileAttributes]::Hidden)
$generator = Join-Path $PSScriptRoot 'pack-candidate-materials.ps1'
$arguments = @{MaterialsDirectory=$inputRoot;ArchivePath=(Join-Path $testRoot 'first.zip')}
$first = & $generator @arguments | ConvertFrom-Json
$arguments.ArchivePath = Join-Path $testRoot 'repeat.zip'
Push-Location (Join-Path $inputRoot 'catalog')
try { $repeat = & $generator @arguments | ConvertFrom-Json }
finally { Pop-Location }
if ($first.files -ne 5 -or $first.sha256 -ne $repeat.sha256) { throw 'Repeated/arbitrary-cwd archive differs.' }
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::OpenRead($first.archive)
try {
    if ($zip.Entries.Count -ne $contents.Count) { throw 'Archive entry count differs.' }
    foreach ($entry in $zip.Entries) {
        if ($entry.FullName.Contains('\') -or -not $contents.ContainsKey($entry.FullName)) { throw 'Nonportable or unexpected ZIP path.' }
        $reader = [IO.StreamReader]::new($entry.Open(),$utf8)
        try { if ($reader.ReadToEnd() -cne $contents[$entry.FullName]) { throw 'ZIP content differs.' } }
        finally { $reader.Dispose() }
    }
}
finally { $zip.Dispose() }
function Assert-Rejected([string]$Message) {
    $rejected = $false
    try { & $generator @arguments | Out-Null }
    catch { if ($_.Exception.Message -ne $Message) { throw }; $rejected = $true }
    if (-not $rejected) { throw "Archive accepted invalid input: $Message" }
}
Assert-Rejected 'Use a fresh source companion archive path.'
$arguments.ArchivePath = Join-Path $inputRoot 'overlap.zip'
Assert-Rejected 'Source companion output overlaps its input.'
$arguments.ArchivePath = Join-Path $testRoot 'archive.txt'
Assert-Rejected 'Use a ZIP filename for source companions.'
$arguments.ArchivePath = Join-Path $testRoot 'missing-parent/archive.zip'
Assert-Rejected 'Source companion output parent is missing.'
$arguments.ArchivePath = Join-Path $testRoot 'missing-marker.zip'
$arguments.MaterialsDirectory = Join-Path $inputRoot 'catalog'
Assert-Rejected 'Candidate material completion files are missing.'
$arguments.MaterialsDirectory = $inputRoot
$junction = Join-Path $inputRoot 'linked'
New-Item -ItemType Junction -Path $junction -Target (Join-Path $inputRoot 'catalog') | Out-Null
Assert-Rejected 'Source companion links are not allowed.'
if (-not [IO.Path]::GetFullPath($junction).StartsWith($testRoot + '\',[StringComparison]::OrdinalIgnoreCase) -or
    -not ((Get-Item -LiteralPath $junction).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unexpected junction removal target.' }
[IO.Directory]::Delete($junction)
$outputLink = Join-Path $testRoot 'output-link'
New-Item -ItemType Junction -Path $outputLink -Target $inputRoot | Out-Null
$arguments.ArchivePath = Join-Path $outputLink 'hidden-overlap.zip'
Assert-Rejected 'Source companion links are not allowed.'
$source = [IO.File]::ReadAllText($generator,$utf8)
$injected = $source.Replace('$inputStream.CopyTo($outputStream)','throw "Injected copy failure."')
if ($injected -eq $source) { throw 'Failure injection anchor missing.' }
$generator = Join-Path $testRoot 'injected.ps1'
[IO.File]::WriteAllText($generator,$injected,$utf8)
$arguments.ArchivePath = Join-Path $testRoot 'incomplete.zip'
Assert-Rejected 'Injected copy failure.'
if ((Test-Path -LiteralPath $arguments.ArchivePath) -or
    @(Get-ChildItem -LiteralPath $testRoot -Filter 'incomplete.zip.partial-*' -File).Count -ne 1) { throw 'Interrupted archive appeared complete.' }
foreach ($name in $hashes.Keys) {
    if ((Get-FileHash -LiteralPath (Join-Path $inputRoot $name)).Hash -ne $hashes[$name]) { throw 'Original fixture changed.' }
}
if ((Get-FileHash -LiteralPath $first.archive).Hash -ne $first.sha256) { throw 'Existing archive changed.' }
Write-Output 'PASS: lossless portable ZIP paths including hidden/Unicode files, repeated/cwd output, invalid paths/markers/links, partial-copy publication guard and original preservation.'
Write-Output "Evidence: $testRoot"
