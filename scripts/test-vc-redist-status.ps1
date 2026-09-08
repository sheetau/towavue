[CmdletBinding()]
param([string]$PackagePath)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'vc-redist-state.ps1')
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $repositoryRoot 'docs/vc-redist-inputs.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$minimum = [version]$manifest.minimum_version
function New-Snapshot([int]$Major=14,[int]$Minor=51,[int]$Build=36247,[int]$Revision=0) {
    return @{Installed=1;Version="v$Major.$Minor.$Build.$Revision";Major=$Major;Minor=$Minor;Bld=$Build;Rbld=$Revision}
}
$cases = @(
    @{name='missing';record=$null;state='required';reason='not_registered'},
    @{name='not installed';record=@{Installed=0};state='required';reason='not_installed'},
    @{name='equal';record=(New-Snapshot);state='satisfied';reason='compatible_version_registered'},
    @{name='older minor';record=(New-Snapshot -Minor 44 -Build 40000);state='required';reason='older_version'},
    @{name='older build';record=(New-Snapshot -Build 36246 -Revision 65535);state='required';reason='older_version'},
    @{name='newer revision';record=(New-Snapshot -Revision 1);state='satisfied';reason='compatible_version_registered'},
    @{name='newer build';record=(New-Snapshot -Build 36248);state='satisfied';reason='compatible_version_registered'},
    @{name='newer minor';record=(New-Snapshot -Minor 52 -Build 1);state='satisfied';reason='compatible_version_registered'},
    @{name='different ABI';record=(New-Snapshot -Major 15);state='unknown';reason='unverified_abi_major'}
)
foreach ($kind in @('flag_string','flag_missing','flag_invalid','field_missing','field_string','field_qword','field_negative','field_overflow','version_missing','version_malformed','version_mismatch','version_overflow','version_three_parts','version_suffix')) {
    $record = New-Snapshot
    $reason = switch ($kind) {
        'flag_string' { $record.Installed = '1'; 'invalid_installed_flag' }
        'flag_missing' { $record.Remove('Installed'); 'invalid_installed_flag' }
        'flag_invalid' { $record.Installed = 2; 'invalid_installed_flag' }
        'field_missing' { $record.Remove('Rbld'); 'invalid_version_fields' }
        'field_string' { $record.Major = '14'; 'invalid_version_fields' }
        'field_qword' { $record.Major = [long]14; 'invalid_version_fields' }
        'field_negative' { $record.Rbld = -1; 'invalid_version_fields' }
        'field_overflow' { $record.Rbld = 65536; 'invalid_version_fields' }
        'version_missing' { $record.Remove('Version'); 'inconsistent_version' }
        'version_malformed' { $record.Version = 'not-a-version'; 'inconsistent_version' }
        'version_mismatch' { $record.Version = 'v14.51.36248.0'; 'inconsistent_version' }
        'version_overflow' { $record.Version = 'v14.51.99999999999999999999.0'; 'inconsistent_version' }
        'version_three_parts' { $record.Version = '14.51.36247'; 'inconsistent_version' }
        'version_suffix' { $record.Version = 'v14.51.36247.0-preview'; 'inconsistent_version' }
    }
    $cases += @{name=$kind;record=$record;state='unknown';reason=$reason}
}
foreach ($text in @('14.51.36247.0','V14.51.36247.00','v14.051.36247.00')) {
    $record = New-Snapshot
    $record.Version = $text
    $cases += @{name=$text;record=$record;state='satisfied';reason='compatible_version_registered'}
}
foreach ($case in $cases) {
    $before = ConvertTo-Json -InputObject $case.record -Compress
    $state = Get-VcRedistState -Snapshot $case.record -MinimumVersion $minimum
    if ($state.state -ne $case.state -or $state.reason -ne $case.reason) { throw "Wrong prerequisite decision: $($case.name)" }
    if ($before -ne (ConvertTo-Json -InputObject $case.record -Compress)) { throw 'Prerequisite evaluator mutated its input.' }
}
$rejected = $false
try { Get-VcRedistState -Snapshot (New-Snapshot) -MinimumVersion '15.0.0.0' | Out-Null }
catch { if ($_.Exception.Message -ne 'Use a complete v14 prerequisite version.') { throw }; $rejected = $true }
if (-not $rejected) { throw 'Unsupported minimum ABI accepted.' }
$reader = Join-Path $PSScriptRoot 'get-vc-redist-status.ps1'
$beforeLive = & $reader | ConvertFrom-Json
if ($PackagePath) {
    $PackagePath = (Resolve-Path -LiteralPath $PackagePath).Path
    $packageHash = (Get-FileHash -LiteralPath $PackagePath).Hash
    $status = & $reader -PackagePath $PackagePath | ConvertFrom-Json
    if (-not $status.package_verified) { throw 'Package identity was not verified.' }
    $scratch = Join-Path $repositoryRoot ('target/tmp/vc-redist-test-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $scratch | Out-Null
    $missing = Join-Path $scratch 'missing.exe'
    $rejected = $false
    try { & $reader -PackagePath $missing | Out-Null }
    catch { if ($_.Exception.Message -ne 'Missing VC redistributable package.') { throw }; $rejected = $true }
    if (-not $rejected) { throw 'Missing prerequisite package accepted.' }
    $corrupt = Join-Path $scratch 'corrupt.exe'
    Copy-Item -LiteralPath $PackagePath -Destination $corrupt
    $stream = [IO.File]::Open($corrupt,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite)
    try { $first = $stream.ReadByte(); $stream.Position = 0; $stream.WriteByte($first -bxor 1) }
    finally { $stream.Dispose() }
    $corruptHash = (Get-FileHash -LiteralPath $corrupt).Hash
    $rejected = $false
    try { & $reader -PackagePath $corrupt | Out-Null }
    catch { if ($_.Exception.Message -ne 'VC redistributable package identity mismatch.') { throw }; $rejected = $true }
    if (-not $rejected -or (Get-FileHash -LiteralPath $corrupt).Hash -ne $corruptHash -or (Get-FileHash -LiteralPath $PackagePath).Hash -ne $packageHash) { throw 'Invalid package accepted or input modified.' }
    Write-Output "Package evidence: $scratch"
}
else { Write-Output 'SKIP: package/signature checks require an explicit local PackagePath; state tests do not validate a redistributable binary.' }
$afterLive = & $reader | ConvertFrom-Json
if (($beforeLive | ConvertTo-Json -Compress) -ne ($afterLive | ConvertTo-Json -Compress)) { throw 'Live prerequisite registration changed during read-only checks.' }
Write-Output "PASS: $($cases.Count) registry snapshots, unsupported ABI rejection and read-only live inspection. No installation executed."
