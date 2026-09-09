[CmdletBinding()]
param(
    [string]$OutputPath,
    [string]$UpstreamNoticeDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
if (-not $OutputPath) {
    $OutputPath = Join-Path $repositoryRoot 'target\distribution\RUST-THIRD-PARTY-NOTICES.txt'
}
if (-not $UpstreamNoticeDirectory) {
    $UpstreamNoticeDirectory = Join-Path $repositoryRoot 'target\tmp\rust-license-sources'
}
$OutputPath = [IO.Path]::GetFullPath($OutputPath)
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs\rust-license-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$utf8 = [Text.UTF8Encoding]::new($false, $true)

function Assert-Hash([string]$Path, [string]$Expected) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Notice input is missing: $Path"
    }
    if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Expected) {
        throw "Notice input checksum mismatch: $Path"
    }
}

function Read-ArchiveText([string]$Archive, [string]$Entry) {
    if ($Entry -match '(^/|(^|/)\.\.(/|$)|["\\])') {
        throw "Invalid archive notice entry: $Entry"
    }
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = 'tar.exe'
    $startInfo.Arguments = '-xOf "' + $Archive + '" "' + $Entry + '"'
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
        if ($process.ExitCode -ne 0) {
            throw "Cannot read archive notice $Entry : $($errorRead.Result)"
        }
        return $utf8.GetString($buffer.ToArray())
    }
    finally {
        $buffer.Dispose()
        $process.Dispose()
    }
}

Assert-Hash (Join-Path $repositoryRoot 'Cargo.lock') $inventory.cargo_lock_sha256
Push-Location -LiteralPath $repositoryRoot
try {
    $metadataText = cargo metadata --locked --offline --format-version 1 --filter-platform x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve the locked Windows dependency graph.' }
}
finally { Pop-Location }
$metadata = $metadataText | ConvertFrom-Json
$packagesById = @{}
$nodesById = @{}
foreach ($package in $metadata.packages) { $packagesById[$package.id] = $package }
foreach ($node in $metadata.resolve.nodes) { $nodesById[$node.id] = $node }
$pending = [Collections.Generic.Queue[string]]::new()
$visited = [Collections.Generic.HashSet[string]]::new()
$pending.Enqueue(($metadata.packages | Where-Object name -eq 'towavue-app').id)
$resolved = @{}
while ($pending.Count -gt 0) {
    $id = $pending.Dequeue()
    if (-not $visited.Add($id)) { continue }
    $package = $packagesById[$id]
    if ($package.source -or @($inventory.vendored_packages | Where-Object { $_.name -eq $package.name -and $_.version -eq $package.version }).Count) { $resolved[$package.name + '@' + $package.version] = $package }
    foreach ($edge in $nodesById[$id].deps) {
        if (@($edge.dep_kinds | Where-Object kind -ne 'dev').Count -gt 0) { $pending.Enqueue($edge.pkg) }
    }
}
if ($resolved.Count -ne $inventory.packages.Count) { throw 'Rust notice inventory is stale.' }
$bundle = [Text.StringBuilder]::new()
[void]$bundle.Append("towavue - Rust third-party notices`n`n")
[void]$bundle.Append("Includes normal and build dependencies for Windows x86-64, not only linked runtime code.`n")
[void]$bundle.Append("Upstream alternative license texts are retained; this does not change OR into AND.`n")
[void]$bundle.Append("FFmpeg native libraries and Microsoft runtime notices are provided separately.`n`n")
foreach ($item in $inventory.packages) {
    $package = $resolved[$item.name + '@' + $item.version]
    if (-not $package -or $package.license -ne $item.declared_license) { throw "Stale notice entry: $($item.name)" }
    $registryDirectory = Split-Path -Parent (Split-Path -Parent $package.manifest_path)
    $vendored = $inventory.vendored_packages | Where-Object { $_.name -eq $item.name -and $_.version -eq $item.version }
    if ($vendored) {
        $vendoredRoot = Join-Path $repositoryRoot $vendored.path
        if ($package.source -or [IO.Path]::GetFullPath($package.manifest_path) -ne [IO.Path]::GetFullPath((Join-Path $vendoredRoot 'Cargo.toml'))) { throw 'Unexpected vendored renderer resolution.' }
        foreach ($file in $vendored.files) { Assert-Hash (Join-Path $vendoredRoot $file.path) $file.sha256 }
    }
    $registryRoot = Split-Path -Parent (Split-Path -Parent $registryDirectory)
    $stem = $item.name + '-' + $item.version
    $archive = Join-Path $registryRoot ('cache/' + (Split-Path -Leaf $registryDirectory) + '/' + $stem + '.crate')
    if (-not $vendored) { Assert-Hash $archive $item.crate_sha256 }
    [void]$bundle.Append("===== $($item.name) $($item.version) =====`nDeclared license: $($item.declared_license)`nSource: $($package.repository)`n")
    if ($vendored) { [void]$bundle.Append("`n--- Local source modification notice ---`n" + [IO.File]::ReadAllText((Join-Path $vendoredRoot 'TOWAVUE-PATCH.md'), $utf8).Replace("`r`n", "`n") + "`n") }
    $noticeCount = 0
    $entries = @($item.local_root_notice_files)
    $entries += @($inventory.additional_crate_notices | Where-Object { $_.name -eq $item.name -and $_.version -eq $item.version } | ForEach-Object path)
    if ($item.name -eq $inventory.embedded_font_notices.package) { $entries += @($inventory.embedded_font_notices.files.path) }
    foreach ($entry in $entries) {
        $body = if ($vendored) { [IO.File]::ReadAllText((Join-Path $vendoredRoot $entry), $utf8).Replace("`r`n", "`n") } else { Read-ArchiveText $archive ($stem + '/' + $entry) }
        [void]$bundle.Append("`n--- Crate file: $entry ---`n$body`n")
        $noticeCount++
    }
    $provenance = $inventory.missing_root_notice_provenance | Where-Object { $_.name -eq $item.name -and $_.version -eq $item.version }
    if ($provenance) {
        foreach ($notice in @($inventory.retrieved_upstream_notices | Where-Object commit -eq $provenance.commit)) {
            $relative = $notice.repository.Replace('https://github.com/', '') + '/' + $notice.commit + '/' + $notice.path
            $noticePath = Join-Path $UpstreamNoticeDirectory $relative
            Assert-Hash $noticePath $notice.sha256
            $body = $utf8.GetString([IO.File]::ReadAllBytes($noticePath))
            [void]$bundle.Append("`n--- Upstream file: $($notice.repository)/blob/$($notice.commit)/$($notice.path) ---`n$body`n")
            $noticeCount++
        }
    }
    foreach ($fallback in @($inventory.license_text_fallbacks | Where-Object { $_.name -eq $item.name -and $_.version -eq $item.version })) {
        $noticePath = Join-Path $repositoryRoot $fallback.path
        Assert-Hash $noticePath $fallback.sha256
        $body = $utf8.GetString([IO.File]::ReadAllBytes($noticePath))
        [void]$bundle.Append("`n--- Standard license text: $($fallback.source_url) ---`n$($fallback.basis)`n`n$body`n")
        $noticeCount++
    }
    if ($noticeCount -eq 0) { throw "No license text for $($item.name) $($item.version)." }
    [void]$bundle.Append("`n")
}

# Validate every input before touching an existing generated bundle.
$outputDirectory = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
$temporaryPath = Join-Path $outputDirectory ([IO.Path]::GetRandomFileName())
try {
    [IO.File]::WriteAllText($temporaryPath, $bundle.ToString().Replace("`r`n", "`n"), $utf8)
    Move-Item -LiteralPath $temporaryPath -Destination $OutputPath -Force
}
finally {
    if (Test-Path -LiteralPath $temporaryPath) { Remove-Item -LiteralPath $temporaryPath }
}
Write-Output $OutputPath
