[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$MaterialsDirectory,
    [Parameter(Mandatory)][string]$ArchivePath
)

$ErrorActionPreference = 'Stop'
$MaterialsDirectory = (Resolve-Path -LiteralPath $MaterialsDirectory).Path
$ArchivePath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($ArchivePath)
if ([IO.Path]::GetExtension($ArchivePath) -ne '.zip') { throw 'Use a ZIP filename for source companions.' }
if (Test-Path -LiteralPath $ArchivePath) { throw 'Use a fresh source companion archive path.' }
if ($ArchivePath.StartsWith($MaterialsDirectory.TrimEnd('\','/') + '\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Source companion output overlaps its input.' }
$archiveParent = Split-Path -Parent $ArchivePath
if (-not (Test-Path -LiteralPath $archiveParent -PathType Container)) { throw 'Source companion output parent is missing.' }
foreach ($root in @($MaterialsDirectory,$archiveParent)) {
    $ancestor = $root
    while ($ancestor) {
        if ((Get-Item -LiteralPath $ancestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Source companion links are not allowed.' }
        $ancestor = Split-Path -Parent $ancestor
    }
}
function Get-Snapshot {
    $snapshot = @{}
    $pending = [Collections.Generic.Queue[string]]::new()
    $pending.Enqueue($MaterialsDirectory)
    while ($pending.Count) {
        foreach ($item in Get-ChildItem -LiteralPath $pending.Dequeue() -Force) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Source companion links are not allowed.' }
            if ($item.PSIsContainer) { $pending.Enqueue($item.FullName); continue }
            $name = $item.FullName.Substring($MaterialsDirectory.Length + 1).Replace('\','/')
            $snapshot.Add($name,@{path=$item.FullName;bytes=$item.Length;sha256=(Get-FileHash -LiteralPath $item.FullName).Hash})
        }
    }
    return $snapshot
}
$before = Get-Snapshot
foreach ($required in @('INPUTS.json','BINDING.json','START-HERE.html')) {
    if (-not $before.ContainsKey($required)) { throw 'Candidate material completion files are missing.' }
}
# This is lossless packaging, not a replacement for test-candidate-materials.ps1.
$names = [string[]]@($before.Keys)
[Array]::Sort($names,[StringComparer]::Ordinal)
$partialPath = $ArchivePath + '.partial-' + [guid]::NewGuid().ToString('N')
Add-Type -AssemblyName System.IO.Compression,System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::Open($partialPath,[IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($name in $names) {
        # Legacy .NET CreateFromDirectory can emit backslashes; name entries explicitly.
        $entry = $archive.CreateEntry($name,[IO.Compression.CompressionLevel]::Optimal)
        $entry.LastWriteTime = [DateTimeOffset]::new(2000,1,1,0,0,0,[TimeSpan]::Zero)
        $inputStream = [IO.File]::OpenRead($before[$name].path)
        try {
            $outputStream = $entry.Open()
            try { $inputStream.CopyTo($outputStream) }
            finally { $outputStream.Dispose() }
        }
        finally { $inputStream.Dispose() }
    }
}
finally { $archive.Dispose() }
$archive = [IO.Compression.ZipFile]::OpenRead($partialPath)
$hasher = [Security.Cryptography.SHA256]::Create()
try {
    if ($archive.Entries.Count -ne $before.Count) { throw 'Source companion coverage mismatch.' }
    $seen = @{}
    foreach ($entry in $archive.Entries) {
        if (-not $before.ContainsKey($entry.FullName) -or $seen.ContainsKey($entry.FullName) -or
            $entry.FullName.Contains('\') -or $entry.Length -ne $before[$entry.FullName].bytes) { throw 'Source companion entry mismatch.' }
        $stream = $entry.Open()
        try { $hash = [BitConverter]::ToString($hasher.ComputeHash($stream)).Replace('-','') }
        finally { $stream.Dispose() }
        if ($hash -ne $before[$entry.FullName].sha256) { throw 'Source companion content mismatch.' }
        $seen[$entry.FullName] = $true
    }
}
finally { $hasher.Dispose(); $archive.Dispose() }
$after = Get-Snapshot
if ($after.Count -ne $before.Count) { throw 'Source material coverage changed during archiving.' }
foreach ($name in $names) {
    if ($before[$name].sha256 -ne $after[$name].sha256) { throw 'Source materials changed during archiving.' }
}
# Publish the local filename only after verifying every decompressed entry and input.
if ([IO.Path]::GetDirectoryName($partialPath) -ne $archiveParent -or [IO.Path]::GetDirectoryName($ArchivePath) -ne $archiveParent) { throw 'Source companion move escapes its output directory.' }
[IO.File]::Move($partialPath,$ArchivePath)
[pscustomobject]@{archive=$ArchivePath;bytes=(Get-Item -LiteralPath $ArchivePath).Length;sha256=(Get-FileHash -LiteralPath $ArchivePath).Hash.ToLowerInvariant();files=$names.Count} | ConvertTo-Json
