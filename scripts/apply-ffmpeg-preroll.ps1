[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$SourceDirectory,
    [Parameter(Mandatory = $true)][string]$MsysRoot,
    [switch]$ValidateOnly
)

$ErrorActionPreference = 'Stop'
$sourceRoot = (Resolve-Path -LiteralPath $SourceDirectory).Path
$patchPath = Join-Path $PSScriptRoot 'ffmpeg-preroll/metadata-preroll.patch'
$manifestPath = Join-Path $PSScriptRoot 'ffmpeg-preroll/metadata-preroll.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$patchTool = Join-Path (Resolve-Path -LiteralPath $MsysRoot).Path 'usr/bin/patch.exe'
if (-not (Test-Path -LiteralPath $patchTool -PathType Leaf)) { throw 'Missing MSYS patch tool.' }
if ((Get-FileHash -LiteralPath $patchPath -Algorithm SHA256).Hash -ne $manifest.patch_sha256) {
    throw 'FFmpeg preroll patch checksum mismatch.'
}

# Refuse an altered, partially patched or already patched input before any write.
foreach ($file in $manifest.files) {
    if ($file.path -notmatch '^libav(codec|util)/[A-Za-z0-9_./-]+$' -or $file.path.Contains('..')) {
        throw 'Invalid source path in the pinned patch manifest.'
    }
    $path = Join-Path $sourceRoot $file.path
    if ($null -eq $file.before_sha256) {
        if (Test-Path -LiteralPath $path) { throw "New patch file already exists: $($file.path)" }
    } elseif (-not (Test-Path -LiteralPath $path -PathType Leaf) -or
        (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $file.before_sha256) {
        throw "Source differs from pinned FFmpeg: $($file.path)"
    }
}

function Invoke-PinnedPatch([bool]$DryRun) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $patchTool
    $start.Arguments = '--batch --forward --fuzz=0 -p1 --directory="' + $sourceRoot + '" --input="' + $patchPath + '"'
    if ($DryRun) { $start.Arguments += ' --dry-run' }
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($start)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        $output = $stdout.Result + $stderr.Result
        if ($process.ExitCode -ne 0) { throw "Pinned patch failed: $output" }
        Write-Verbose $output
    } finally {
        $process.Dispose()
    }
}

Invoke-PinnedPatch $true
if (-not $ValidateOnly) {
    Invoke-PinnedPatch $false
    foreach ($file in $manifest.files) {
        if ((Get-FileHash -LiteralPath (Join-Path $sourceRoot $file.path) -Algorithm SHA256).Hash -ne $file.after_sha256) {
            throw "Patched source checksum mismatch: $($file.path)"
        }
    }
}
[pscustomobject]@{
    validated = $true
    applied = -not [bool]$ValidateOnly
    ffmpeg_commit = $manifest.ffmpeg_commit
    patch_sha256 = $manifest.patch_sha256
    source_files = $manifest.files.Count
    codec_mask = $manifest.codec_mask
}
