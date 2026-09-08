[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$CacheDirectory,
    [Parameter(Mandatory = $true)][string]$SourceMaterialsDirectory,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$unicodePath = Join-Path $repositoryRoot 'docs/native-unicode-table-audit.json'
$fontPath = Join-Path $repositoryRoot 'docs/native-font-data-audit.json'
$supplementPath = Join-Path $repositoryRoot 'docs/native-source-supplements.json'
$unicode = Get-Content -LiteralPath $unicodePath -Raw -Encoding UTF8 | ConvertFrom-Json
$font = Get-Content -LiteralPath $fontPath -Raw -Encoding UTF8 | ConvertFrom-Json
$supplement = Get-Content -LiteralPath $supplementPath -Raw -Encoding UTF8 | ConvertFrom-Json
if ($unicode.schema_version -ne 1 -or $font.schema_version -ne 1 -or
    $unicode.pcre2.data.Count -ne 13 -or $unicode.pcre2.generators.Count -ne 3 -or
    $unicode.libxml2.data.Count -ne 2 -or $font.libunibreak.inputs.Count -ne 7) {
    throw 'Incomplete native data inventory.'
}
foreach ($name in @('CacheDirectory','SourceMaterialsDirectory','OutputDirectory')) {
    Set-Variable -Name $name -Value ($ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath((Get-Variable -Name $name -ValueOnly)))
}
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh native data output directory.' }
foreach ($root in @($CacheDirectory,$SourceMaterialsDirectory)) {
    if ($root -eq $OutputDirectory -or
        $root.StartsWith($OutputDirectory.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
        $OutputDirectory.StartsWith($root.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Native data output overlaps an input.'
    }
}
function Get-SafeFile([string]$Root, [string]$Relative) {
    if (-not $Relative -or $Relative -match '(^/|[\\:"]|(^|/)\.\.(/|$))') { throw 'Invalid native data path.' }
    $path = $Root
    foreach ($part in @('') + @($Relative.Split('/'))) {
        if ($part) { $path = Join-Path $path $part }
        if (-not (Test-Path -LiteralPath $path)) { throw "Missing native data input: $path" }
        if ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Native data input links are not allowed.' }
    }
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing native data input: $path" }
    return $path
}
function Assert-Input([string]$Path, $Record) {
    if ((Get-Item -LiteralPath $Path).Length -ne $Record.bytes -or (Get-FileHash -LiteralPath $Path).Hash -ne $Record.sha256) {
        throw "Native data checksum mismatch: $Path"
    }
}
$inputs = [Collections.Generic.List[object]]::new()
$names = @{}
function Add-Input([string]$Root, [string]$Relative, [string]$Name, $Record) {
    if ($names.ContainsKey($Name)) { throw 'Duplicate native data output path.' }
    $names[$Name] = $true
    $path = Get-SafeFile $Root $Relative
    if (-not $Record) { $Record = @{bytes=(Get-Item -LiteralPath $path).Length;sha256=(Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant()} }
    Assert-Input $path $Record
    $inputs.Add([pscustomobject]@{name=$Name;path=$path;bytes=$Record.bytes;sha256=$Record.sha256})
}
foreach ($group in @(
    @{prefix='pcre2/Unicode.tables/';records=$unicode.pcre2.data},
    @{prefix='pcre2/';records=$unicode.pcre2.generators},
    @{prefix='libxml2/';records=$unicode.libxml2.data},
    @{prefix='libunibreak/';records=$font.libunibreak.inputs}
)) {
    foreach ($record in $group.records) {
        if ($record.name -notmatch '^[A-Za-z0-9_.-]+$' -or $record.name -in @('.','..')) { throw 'Invalid native data path.' }
        $relative = $group.prefix + $record.name
        Add-Input $CacheDirectory $relative $relative $record
    }
}
$sourceMarker = Get-SafeFile $SourceMaterialsDirectory 'INPUTS.json'
if ((Get-FileHash -LiteralPath $sourceMarker).Hash -ne (Get-FileHash -LiteralPath $supplementPath).Hash) { throw 'Stale native data source supplement.' }
foreach ($record in @($unicode.pcre2,$unicode.libxml2)) {
    $owner = @($supplement.packages | Where-Object package -eq $record.package)
    $archive = @($owner.files | Where-Object { $_.name -eq $record.source_archive -and $_.sha256 -eq $record.source_sha256 })
    if ($owner.Count -ne 1 -or $archive.Count -ne 1) { throw 'Stale native data source mapping.' }
}
$pcre = $supplement.packages | Where-Object package -eq $unicode.pcre2.package
$license = @($pcre.selected_documents | Where-Object package_notice -eq 'mingw64/share/licenses/pcre2/LICENCE.md')
if ($license.Count -ne 1) { throw 'Missing PCRE2 generator license mapping.' }
Add-Input $SourceMaterialsDirectory ($pcre.package + '/' + $license[0].name) 'pcre2/LICENCE.md' $license[0]
Add-Input $repositoryRoot $unicode.notice.name 'UNICODE-LICENSE.txt' $unicode.notice
Add-Input $repositoryRoot 'docs/native-unicode-table-audit.json' 'native-unicode-table-audit.json' $null
Add-Input $repositoryRoot 'docs/native-font-data-audit.json' 'native-font-data-audit.json' $null
Add-Input $repositoryRoot 'third-party/NATIVE-DATA-MATERIALS-README.txt' 'README.txt' $null

# Validate every input before creating output; INPUTS is written only after all copies verify.
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
foreach ($inputFile in $inputs) {
    $path = Join-Path $OutputDirectory $inputFile.name
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    Copy-Item -LiteralPath $inputFile.path -Destination $path
    Assert-Input $path $inputFile
}
$manifest = [ordered]@{
    schema_version=1
    scope='Fixed external Unicode data and PCRE2 generators; companion to the source supplement, not a DLL rebuild or release approval.'
    source_supplement_sha256=(Get-FileHash -LiteralPath $supplementPath).Hash.ToLowerInvariant()
    files=@($inputs | Select-Object name,bytes,sha256)
}
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'INPUTS.json'), ($manifest | ConvertTo-Json -Depth 6) + "`n", [Text.UTF8Encoding]::new($false))
Write-Output "Native data materials: $OutputDirectory"
Write-Output '22 fixed data files, three original generators and their notices; no download or generator execution.'
