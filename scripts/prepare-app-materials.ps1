[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$RustNotices,
    [Parameter(Mandatory = $true)][string]$RuntimeNotices,
    [Parameter(Mandatory = $true)][string]$Executable,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $repositoryRoot 'docs/app-material-inputs.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$OutputDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh application materials output directory.' }
if ($manifest.schema_version -ne 1 -or $manifest.notice_files.Count -ne 2) { throw 'Incomplete application materials manifest.' }
function Assert-File([string]$Path, $Record) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing application material: $Path" }
    $item = Get-Item -LiteralPath $Path
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Application material links are not allowed.' }
    if ($item.FullName.StartsWith($OutputDirectory.TrimEnd('\','/') + '\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Application material output overlaps an input.' }
    if ($item.Length -ne $Record.bytes -or (Get-FileHash -LiteralPath $Path).Hash -ne $Record.sha256) { throw "Application material checksum mismatch: $Path" }
}
Assert-File $RustNotices $manifest.notice_files[0]
Assert-File $RuntimeNotices $manifest.notice_files[1]
Assert-File $Executable $manifest.candidate
Assert-File (Join-Path $repositoryRoot $manifest.runtime_inventory.name) $manifest.runtime_inventory
$copies = @{}
foreach ($record in $manifest.repository_materials) {
    if ($record.name -notmatch '^[A-Za-z0-9_][A-Za-z0-9_./-]*$' -or $record.name -match '(^|/)\.\.(/|$)') { throw 'Invalid application material path.' }
    if ($copies.ContainsKey($record.name)) { throw 'Duplicate application material.' }
    $path = Join-Path $repositoryRoot $record.name
    Assert-File $path $record
    $copies[$record.name] = @{path=$path;record=$record}
}
$copies['RUST-THIRD-PARTY-NOTICES.txt'] = @{path=$RustNotices;record=$manifest.notice_files[0]}
$copies['TOWAVUE-RUST-RUNTIME-NOTICES.zip'] = @{path=$RuntimeNotices;record=$manifest.notice_files[1]}
$licenses = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/rust-license-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$toolchain = Get-Content -LiteralPath (Join-Path $repositoryRoot 'rust-toolchain.toml') -Raw -Encoding UTF8
if ($licenses.cargo_lock_sha256 -ne (Get-FileHash -LiteralPath (Join-Path $repositoryRoot 'Cargo.lock')).Hash -or
    $licenses.packages.Count -ne $manifest.dependency_count -or
    $toolchain -notmatch ('(?m)^channel\s*=\s*"' + [regex]::Escape($manifest.version) + '"\s*$')) { throw 'Application dependency/toolchain mapping is stale.' }
$runtimeInventory = Get-Content -LiteralPath (Join-Path $repositoryRoot $manifest.runtime_inventory.name) -Raw -Encoding UTF8 | ConvertFrom-Json
$archives = @($runtimeInventory.archives | Where-Object used_by -eq 'towavue')
$files = @($archives.files)
if ($archives.Count -ne 2 -or $files.Count -ne $manifest.runtime_notice_count -or
    @($archives | Where-Object { $_.version -ne $manifest.version -or $_.target -ne $manifest.target }).Count) { throw 'Application runtime selection is stale.' }
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::OpenRead($ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($RuntimeNotices))
$hasher = [Security.Cryptography.SHA256]::Create()
try {
    if ($zip.Entries.Count -ne $files.Count + 2) { throw 'Unexpected application runtime ZIP coverage.' }
    foreach ($file in $files) {
        $entries = @($zip.Entries | Where-Object FullName -eq $file.output)
        if ($entries.Count -ne 1 -or $entries[0].Length -ne $file.bytes) { throw 'Missing or duplicate application runtime notice.' }
        $stream = $entries[0].Open()
        try { $hash = [BitConverter]::ToString($hasher.ComputeHash($stream)).Replace('-','').ToLowerInvariant() }
        finally { $stream.Dispose() }
        if ($hash -ne $file.sha256) { throw 'Application runtime notice differs from original.' }
    }
    if (-not $zip.GetEntry('README.txt') -or -not $zip.GetEntry('INPUTS.json')) { throw 'Missing application runtime provenance.' }
    $reader = [IO.StreamReader]::new($zip.GetEntry('INPUTS.json').Open(),[Text.UTF8Encoding]::new($false,$true))
    try { $embedded = $reader.ReadToEnd() | ConvertFrom-Json }
    finally { $reader.Dispose() }
    if ((ConvertTo-Json -InputObject @($embedded.archives) -Depth 10 -Compress) -ne
        (ConvertTo-Json -InputObject $archives -Depth 10 -Compress)) { throw 'Application runtime provenance selection differs.' }
}
finally { $hasher.Dispose(); $zip.Dispose() }

New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
foreach ($name in $copies.Keys) {
    $target = Join-Path $OutputDirectory $name
    New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
    Copy-Item -LiteralPath $copies[$name].path -Destination $target
    # Output copies are expected inside OutputDirectory; compare bytes directly.
    if ((Get-Item -LiteralPath $target).Length -ne $copies[$name].record.bytes -or (Get-FileHash -LiteralPath $target).Hash -ne $copies[$name].record.sha256) { throw 'Application material copy mismatch.' }
}
Copy-Item -LiteralPath (Join-Path $OutputDirectory 'third-party/APP-MATERIALS-README.txt') -Destination "$OutputDirectory/README.txt"
Copy-Item -LiteralPath $manifestPath -Destination "$OutputDirectory/INPUTS.json"
$evidence = [ordered]@{schema_version=1;candidate=$manifest.candidate;cargo_lock_sha256=$licenses.cargo_lock_sha256;dependency_count=$licenses.packages.Count;runtime_version=$manifest.version;runtime_target=$manifest.target;runtime_notice_count=$files.Count;distribution_approved=$false}
[IO.File]::WriteAllText("$OutputDirectory/EVIDENCE.json", ($evidence | ConvertTo-Json -Depth 5) + "`n", [Text.UTF8Encoding]::new($false))
Write-Output "Verified application materials: $OutputDirectory"
Write-Output '146 dependency entries with embedded font notices; 18 matching MSVC Rust originals. No executable copied or distribution approved.'
