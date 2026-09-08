[CmdletBinding()]
param(
    [switch]$Download,
    [string]$CacheDirectory,
    [string]$OutputPath
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
if (-not $CacheDirectory) { $CacheDirectory = Join-Path $repositoryRoot 'target/tmp/rust-runtime-materials' }
if (-not $OutputPath) { $OutputPath = Join-Path $repositoryRoot 'target/distribution/RUST-RUNTIME-NOTICES.zip' }
$CacheDirectory = [IO.Path]::GetFullPath($CacheDirectory)
$OutputPath = [IO.Path]::GetFullPath($OutputPath)
$inventoryPath = Join-Path $repositoryRoot 'docs/rust-runtime-inputs.json'
$readmePath = Join-Path $repositoryRoot 'third-party/RUST-RUNTIME-README.txt'
$utf8 = [Text.UTF8Encoding]::new($false, $true)
$inventoryText = [IO.File]::ReadAllText($inventoryPath, $utf8).Replace("`r`n", "`n")
$inventory = $inventoryText | ConvertFrom-Json
if ($inventory.schema_version -ne 1) { throw 'Unsupported Rust runtime notice inventory.' }
$toolchain = [IO.File]::ReadAllText((Join-Path $repositoryRoot 'rust-toolchain.toml'), $utf8)
$appRuntime = $inventory.archives | Where-Object { $_.used_by -eq 'towavue' -and $_.component -eq 'rustc' }
$nativeRuntime = $inventory.archives | Where-Object { $_.used_by -eq 'rav1e in the fixed FFmpeg binary' -and $_.component -eq 'rustc' }
$nativeInputs = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/ffmpeg-runtime-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($toolchain -notmatch ('(?m)^channel\s*=\s*"' + [regex]::Escape($appRuntime.version) + '"\s*$') -or
    $nativeRuntime.version -ne $nativeInputs.rav1e.rustc_version -or
    $nativeRuntime.compiler_commit -ne $nativeInputs.rav1e.rustc_commit) {
    throw 'Rust runtime notice inventory is stale.'
}
$inputs = @($inventoryPath, $readmePath) + @($inventory.archives | ForEach-Object {
    Join-Path $CacheDirectory ([uri]$_.url).Segments[-1]
})
if ($OutputPath -in $inputs) { throw 'The notice output must not replace an input.' }

function Assert-Archive([string]$Path, $Record) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Runtime archive is missing: $Path" }
    if ((Get-Item -LiteralPath $Path).Length -ne $Record.bytes -or
        (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Record.sha256) {
        throw "Runtime archive checksum mismatch: $Path"
    }
}

function Get-BytesHash([byte[]]$Bytes) {
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($hasher.ComputeHash($Bytes)).Replace('-', '').ToLowerInvariant() }
    finally { $hasher.Dispose() }
}

function Read-Notices([string]$Path, $Files) {
    foreach ($file in $Files) {
        foreach ($name in @($file.entry, $file.output)) {
            if ($name -notmatch '^[A-Za-z0-9_][A-Za-z0-9_./-]*$' -or $name -match '(^|/)\.\.(/|$)') {
                throw "Invalid runtime notice path: $name"
            }
        }
    }
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = 'tar.exe'
    $startInfo.Arguments = '-xOf "' + $Path + '" ' + (($Files | ForEach-Object { '"' + $_.entry + '"' }) -join ' ')
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($startInfo)
    $buffer = [IO.MemoryStream]::new()
    try {
        $errorRead = $process.StandardError.ReadToEndAsync()
        $process.StandardOutput.BaseStream.CopyTo($buffer)
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) { throw "Cannot read runtime notices: $($errorRead.Result)" }
        # The pinned archive contains regular files in manifest order. Hash each
        # segment of tar's concatenated stdout; never extract an upstream path.
        $expectedLength = ($Files | Measure-Object -Property bytes -Sum).Sum
        if ($buffer.Length -ne $expectedLength) { throw 'Runtime notice stream length mismatch.' }
        $buffer.Position = 0
        foreach ($file in $Files) {
            $bytes = [byte[]]::new($file.bytes)
            if ($buffer.Read($bytes, 0, $bytes.Length) -ne $bytes.Length -or (Get-BytesHash $bytes) -ne $file.sha256) {
                throw "Runtime notice checksum mismatch: $($file.entry)"
            }
            if ($materials.Contains($file.output)) { throw "Duplicate runtime notice output: $($file.output)" }
            $materials.Add($file.output, $bytes)
        }
    }
    finally {
        $buffer.Dispose()
        $process.Dispose()
    }
}

foreach ($archive in $inventory.archives) {
    $path = Join-Path $CacheDirectory ([uri]$archive.url).Segments[-1]
    if ($Download -and -not (Test-Path -LiteralPath $path)) {
        New-Item -ItemType Directory -Path $CacheDirectory -Force | Out-Null
        $temporary = Join-Path $CacheDirectory ([IO.Path]::GetRandomFileName())
        try {
            Invoke-WebRequest -UseBasicParsing -Uri $archive.url -OutFile $temporary
            Assert-Archive $temporary $archive
            Move-Item -LiteralPath $temporary -Destination $path
        }
        finally {
            if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
        }
    }
    Assert-Archive $path $archive
}

$materials = [ordered]@{
    'README.txt' = $utf8.GetBytes([IO.File]::ReadAllText($readmePath, $utf8).Replace("`r`n", "`n"))
    'INPUTS.json' = $utf8.GetBytes($inventoryText)
}
foreach ($archive in $inventory.archives) {
    Read-Notices (Join-Path $CacheDirectory ([uri]$archive.url).Segments[-1]) $archive.files
}

Add-Type -AssemblyName System.IO.Compression
$outputDirectory = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
$temporary = Join-Path $outputDirectory ([IO.Path]::GetRandomFileName())
try {
    $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew)
    try {
        $zip = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create, $true)
        try {
            foreach ($name in $materials.Keys) {
                $entry = $zip.CreateEntry($name, [IO.Compression.CompressionLevel]::NoCompression)
                $entry.LastWriteTime = [DateTimeOffset]::new(1980, 1, 1, 0, 0, 0, [TimeSpan]::Zero)
                $entry.ExternalAttributes = 0
                $destination = $entry.Open()
                try { $destination.Write($materials[$name], 0, $materials[$name].Length) }
                finally { $destination.Dispose() }
            }
        }
        finally { $zip.Dispose() }
    }
    finally { $stream.Dispose() }
    Move-Item -LiteralPath $temporary -Destination $OutputPath -Force
}
finally {
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
}
Write-Output "Rust runtime notices: $($materials.Count - 2) original files; $OutputPath"
Write-Output "SHA256: $((Get-FileHash -LiteralPath $OutputPath -Algorithm SHA256).Hash)"
