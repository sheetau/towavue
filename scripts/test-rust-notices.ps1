[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$generator = Join-Path $PSScriptRoot 'prepare-rust-notices.ps1'
$testDirectory = Join-Path $repositoryRoot ('target\tmp\rust-notice-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory | Out-Null
$firstPath = Join-Path $testDirectory 'first.txt'
$secondPath = Join-Path $testDirectory 'second.txt'
& $generator -OutputPath $firstPath | Out-Null
Push-Location -LiteralPath $testDirectory
try { & $generator -OutputPath $secondPath | Out-Null }
finally { Pop-Location }
$expectedHash = (Get-FileHash -LiteralPath $firstPath -Algorithm SHA256).Hash
if ($expectedHash -ne (Get-FileHash -LiteralPath $secondPath -Algorithm SHA256).Hash) {
    throw 'Repeated generation produced different bytes.'
}
$text = [IO.File]::ReadAllText($firstPath, [Text.UTF8Encoding]::new($false, $true))
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs\rust-license-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
foreach ($package in $inventory.packages) {
    $heading = '===== ' + $package.name + ' ' + $package.version + ' ====='
    if ([regex]::Matches($text, [regex]::Escape($heading)).Count -ne 1) {
        throw "Missing or duplicate package section: $heading"
    }
}
foreach ($required in @('LICENSE.chromium', 'fonts/Hack-Regular.txt', 'fonts/OFL.txt', 'fonts/UFL.txt',
        'fonts/emoji-icon-font-mit-license.txt', 'src/unicode_tables/LICENSE-UNICODE', 'src/spin/LICENSE',
        'not a notice file recovered from the crate', 'not the authors of ffmpeg-sys-next')) {
    if (-not $text.Contains($required)) { throw "Missing notice material: $required" }
}
$metadata = cargo metadata --locked --offline --format-version 1 --filter-platform x86_64-pc-windows-msvc --manifest-path (Join-Path $repositoryRoot 'Cargo.toml') | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve notice test inputs.' }
$bodyChecks = @($inventory.license_text_fallbacks | ForEach-Object {
    @{ path = (Join-Path $repositoryRoot $_.path); sha256 = $_.sha256 }
})
foreach ($notice in $inventory.retrieved_upstream_notices) {
    $relative = $notice.repository.Replace('https://github.com/', '') + '/' + $notice.commit + '/' + $notice.path
    $bodyChecks += @{ path = (Join-Path $repositoryRoot ('target/tmp/rust-license-sources/' + $relative)); sha256 = $notice.sha256 }
}
foreach ($notice in $inventory.additional_crate_notices) {
    $package = $metadata.packages | Where-Object { $_.name -eq $notice.name -and $_.version -eq $notice.version }
    $bodyChecks += @{ path = (Join-Path (Split-Path -Parent $package.manifest_path) $notice.path); sha256 = $notice.sha256 }
}
$fontPackage = $metadata.packages | Where-Object { $_.name -eq $inventory.embedded_font_notices.package -and $_.version -eq $inventory.embedded_font_notices.version }
foreach ($notice in $inventory.embedded_font_notices.files) {
    $bodyChecks += @{ path = (Join-Path (Split-Path -Parent $fontPackage.manifest_path) $notice.path); sha256 = $notice.sha256 }
}
foreach ($check in $bodyChecks) {
    if ((Get-FileHash -LiteralPath $check.path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $check.sha256) {
        throw "Unexpected reference notice bytes: $($check.path)"
    }
    $body = [IO.File]::ReadAllText($check.path, [Text.UTF8Encoding]::new($false, $true)).Replace("`r`n", "`n")
    if (-not $text.Contains($body)) { throw "Incomplete notice body: $($check.path)" }
}
$bytes = [IO.File]::ReadAllBytes($firstPath)
if (($bytes[0] -eq 239 -and $bytes[1] -eq 187 -and $bytes[2] -eq 191) -or $text.Contains("`r")) {
    throw 'Expected UTF-8 without BOM and LF line endings.'
}

$missingDirectory = Join-Path $testDirectory 'missing'
$rejected = $false
try { & $generator -OutputPath $firstPath -UpstreamNoticeDirectory $missingDirectory | Out-Null }
catch {
    if ($_.Exception.Message -notlike 'Notice input is missing:*') { throw }
    $rejected = $true
}
if (-not $rejected) { throw 'Missing upstream notices were accepted.' }
if ((Get-FileHash -LiteralPath $firstPath -Algorithm SHA256).Hash -ne $expectedHash) {
    throw 'Missing input changed the existing output.'
}

$notice = $inventory.retrieved_upstream_notices[0]
$modifiedDirectory = Join-Path $testDirectory 'modified'
$relative = $notice.repository.Replace('https://github.com/', '') + '/' + $notice.commit + '/' + $notice.path
$modifiedPath = Join-Path $modifiedDirectory $relative
New-Item -ItemType Directory -Path (Split-Path -Parent $modifiedPath) -Force | Out-Null
[IO.File]::WriteAllText($modifiedPath, 'Deliberately corrupted test input.', [Text.UTF8Encoding]::new($false))
$rejected = $false
try { & $generator -OutputPath $firstPath -UpstreamNoticeDirectory $modifiedDirectory | Out-Null }
catch {
    if ($_.Exception.Message -notlike 'Notice input checksum mismatch:*') { throw }
    $rejected = $true
}
if (-not $rejected) { throw 'Modified upstream notices were accepted.' }
if ((Get-FileHash -LiteralPath $firstPath -Algorithm SHA256).Hash -ne $expectedHash) {
    throw 'Modified input changed the existing output.'
}
Write-Output "Rust notices passed: $($inventory.packages.Count) packages, deterministic bytes, missing/modified input rejection, existing output preserved."
Write-Output "Artifacts: $testDirectory"
