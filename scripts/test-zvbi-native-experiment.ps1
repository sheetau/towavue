[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$MsysRoot,
    [Parameter(Mandatory = $true)][string]$FfmpegPrefix,
    [Parameter(Mandatory = $true)][string]$ZvbiPrefix
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$MsysRoot = (Resolve-Path -LiteralPath $MsysRoot).Path.Replace('\', '/')
$FfmpegPrefix = (Resolve-Path -LiteralPath $FfmpegPrefix).Path.Replace('\', '/')
$ZvbiPrefix = (Resolve-Path -LiteralPath $ZvbiPrefix).Path.Replace('\', '/')
$baselineDll = "$FfmpegPrefix/bin/libzvbi-0.dll"
$candidateDll = "$ZvbiPrefix/bin/libzvbi-0.dll"
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-zvbi-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ((Get-FileHash -LiteralPath $baselineDll).Hash -ne $inventory.runtime.sha256) { throw 'Baseline ZVBI is not the audited package DLL.' }
$candidateHash = (Get-FileHash -LiteralPath $candidateDll).Hash
if ($candidateHash -eq $inventory.runtime.sha256) { throw 'A different experimental ZVBI DLL is required.' }
function Get-Exports([string]$Dll) {
    $lines = & "$MsysRoot/mingw64/bin/objdump.exe" -p $Dll
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect ZVBI exports.' }
    $names = @($lines | ForEach-Object {
        if ($_ -match '^\s+\[\s*\d+\]\s+\+base\[\s*\d+\]\s+[0-9a-f]+\s+([A-Za-z_][A-Za-z_0-9]+)$') { $Matches[1] }
    })
    if ($names.Count -eq 0) { throw 'ZVBI export parser returned no names.' }
    return $names
}
$removed = @(
    '_vbi_mktime', '_vbi_pil_dump', '_vbi_pil_from_string', '_vbi_program_id_dump', '_vbi_timegm',
    'vbi_decode_dvb_pdc_descriptor', 'vbi_decode_teletext_8301_cni', 'vbi_decode_teletext_8301_local_time',
    'vbi_decode_teletext_8302_cni', 'vbi_decode_teletext_8302_pdc', 'vbi_decode_vps_pdc',
    'vbi_encode_dvb_pdc_descriptor', 'vbi_encode_vps_pdc', 'vbi_pil_is_valid_date', 'vbi_pil_lto_to_time',
    'vbi_pil_lto_validity_window', 'vbi_pil_to_time', 'vbi_pil_validity_window', 'vbi_pty_validity_window'
)
$before = @(Get-Exports $baselineDll)
$after = @(Get-Exports $candidateDll)
if ($before.Count -ne 367 -or $after.Count -ne 348 -or
    @(Compare-Object @($before | Where-Object { $_ -notin $removed }) $after).Count) {
    throw 'ZVBI export changes exceed the nineteen excluded program-ID/time APIs.'
}
$testDirectory = Join-Path $repositoryRoot ('target/tmp/zvbi-teletext-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path "$testDirectory/baseline", "$testDirectory/limited" | Out-Null
Copy-Item -LiteralPath $candidateDll -Destination "$testDirectory/limited/libzvbi-0.dll"
$savedPath = $env:PATH
Push-Location -LiteralPath $testDirectory
try {
    $fixture = Join-Path $PSScriptRoot 'fixtures/zvbi-teletext-smoke.c'
    $compiler = "$MsysRoot/mingw64/bin/gcc.exe"
    foreach ($variant in @('baseline', 'limited')) {
        $env:PATH = "$MsysRoot/mingw64/bin;" + (Join-Path $env:SystemRoot 'System32')
        $include = if ($variant -eq 'baseline') { "$MsysRoot/mingw64/include" } else { "$ZvbiPrefix/include" }
        $lib = if ($variant -eq 'baseline') { "$MsysRoot/mingw64/lib" } else { "$ZvbiPrefix/lib" }
        & $compiler -std=c11 -Wall -Wextra -Werror $fixture "-I$include" "-I$FfmpegPrefix/include" `
            "-L$FfmpegPrefix/lib" "-L$lib" -lavcodec -lavutil -lzvbi -o "$testDirectory/$variant/smoke.exe"
        if ($LASTEXITCODE -ne 0) { throw "Teletext caller compilation failed: $variant" }
        $env:PATH = "$FfmpegPrefix/bin;" + (Join-Path $env:SystemRoot 'System32')
        $dll = if ($variant -eq 'baseline') { $baselineDll } else { "$testDirectory/limited/libzvbi-0.dll".Replace('\', '/') }
        $scope = if ($variant -eq 'baseline') { '1' } else { '0' }
        & "$testDirectory/$variant/smoke.exe" "$testDirectory/$variant/output.bin" $dll $scope
        if ($LASTEXITCODE -ne 0) { throw "Teletext decode or actual DLL/scope check failed: $variant" }
    }
    Copy-Item -LiteralPath "$testDirectory/baseline/smoke.exe" -Destination "$testDirectory/limited/old-header.exe"
    & "$testDirectory/limited/old-header.exe" "$testDirectory/limited/old-header.bin" `
        "$testDirectory/limited/libzvbi-0.dll".Replace('\', '/') '0'
    if ($LASTEXITCODE -ne 0) { throw 'The old-header caller failed against experimental ZVBI.' }
    $expected = (Get-FileHash -LiteralPath "$testDirectory/baseline/output.bin").Hash
    foreach ($path in @("$testDirectory/limited/output.bin", "$testDirectory/limited/old-header.bin")) {
        if ((Get-FileHash -LiteralPath $path).Hash -ne $expected) { throw 'Subtitle bytes or tested ABI differ from the baseline.' }
    }
    if ((Get-FileHash -LiteralPath $baselineDll).Hash -ne $inventory.runtime.sha256 -or
        (Get-FileHash -LiteralPath $candidateDll).Hash -ne $candidateHash -or
        (Get-FileHash -LiteralPath "$testDirectory/limited/libzvbi-0.dll").Hash -ne $candidateHash) {
        throw 'A compared ZVBI DLL changed during the test.'
    }
    Write-Output "All twelve bitmap/text/ASS cases match byte-for-byte: $expected"
    Write-Output "Evidence: $testDirectory"
    Write-Output 'Synthetic API coverage only; no recorded-broadcast, full FFmpeg rebuild, app/installer or distribution approval.'
}
finally {
    Pop-Location
    $env:PATH = $savedPath
}
