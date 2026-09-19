[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$FfmpegPrefix,
    [Parameter(Mandatory)][string]$NativeMaterialsDirectory,
    [Parameter(Mandatory)][string]$VcRedist,
    [Parameter(Mandatory)][string]$NsisArchive,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [string]$CargoTargetDirectory,
    [string]$PackageDirectory,
    [string]$RecipeDirectory,
    [string]$RustNoticeDirectory,
    [string]$RuntimeNoticeCache
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-materials.ps1')
. (Join-Path $PSScriptRoot 'release-signing.ps1')
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$identity = Get-ReleaseSourceIdentity $repositoryRoot
if (-not $CargoTargetDirectory) { $CargoTargetDirectory = Join-Path $repositoryRoot 'target/release-windows-x64' }
foreach ($name in @('FfmpegPrefix','NativeMaterialsDirectory','VcRedist','NsisArchive','OutputDirectory','CargoTargetDirectory')) {
    Set-Variable -Name $name -Value (Resolve-ReleasePath (Get-Variable -Name $name -ValueOnly))
}
Assert-ReleaseOutput $OutputDirectory @($FfmpegPrefix,$NativeMaterialsDirectory,$VcRedist,$NsisArchive,$CargoTargetDirectory)
$keyFile = Get-TowavueSigningKeyPath
$publicKey = Join-Path $repositoryRoot 'packaging/windows/update-public-key.hex'
# Fail before expensive work; never initialize or replace a production identity here.
$key = Read-TowavueSigningKey $keyFile $publicKey
$key.Dispose()
$nsis = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/nsis-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
Assert-ReleaseFile $NsisArchive ([pscustomobject]@{name='nsis-3.12.zip';bytes=$nsis.archive.bytes;sha256=$nsis.archive.sha256})
$vc = & (Join-Path $PSScriptRoot 'get-vc-redist-status.ps1') -PackagePath $VcRedist | ConvertFrom-Json
if (-not $vc.package_verified) { throw 'Release prerequisite identity is not verified.' }
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$savedEnvironment = @{}
foreach ($name in @('FFMPEG_DIR','CARGO_TARGET_DIR','PATH')) { $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name,'Process') }
Push-Location $repositoryRoot
try {
    $env:FFMPEG_DIR = $FfmpegPrefix
    $env:CARGO_TARGET_DIR = $CargoTargetDirectory
    $env:PATH = (Join-Path $FfmpegPrefix 'bin') + ';' + $savedEnvironment.PATH
    # Capture native stderr without PowerShell's NativeCommandError conversion.
    $cargo = (Get-Command cargo.exe -ErrorAction Stop).Source
    $buildLog = Join-Path $OutputDirectory 'cargo-build.log'
    $errorLog = Join-Path $OutputDirectory 'cargo-build.stderr.log'
    $process = Start-Process -FilePath $cargo -ArgumentList 'build -p towavue-app --release --target x86_64-pc-windows-msvc --locked --offline' -WorkingDirectory $repositoryRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput $buildLog -RedirectStandardError $errorLog
    [void]$process.Handle
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) { throw "Release application build failed. See $errorLog" }
}
finally {
    Pop-Location
    foreach ($name in $savedEnvironment.Keys) { [Environment]::SetEnvironmentVariable($name,$savedEnvironment[$name],'Process') }
}
$builtExecutable = Join-Path $CargoTargetDirectory 'x86_64-pc-windows-msvc/release/towavue.exe'
$builtRecord = Get-ReleaseRecord $builtExecutable 'towavue.exe'
$applicationOutput = Join-Path $OutputDirectory 'application'
New-Item -ItemType Directory -Path $applicationOutput | Out-Null
$executable = Join-Path $applicationOutput 'towavue.exe'
Copy-Item -LiteralPath $builtExecutable -Destination $executable
Assert-ReleaseFile $executable $builtRecord
$materialsOutput = Join-Path $OutputDirectory 'materials-build'
$arguments = @{Executable=$executable;FfmpegPrefix=$FfmpegPrefix;NativeMaterialsDirectory=$NativeMaterialsDirectory;OutputDirectory=$materialsOutput}
foreach ($name in @('PackageDirectory','RecipeDirectory','RustNoticeDirectory','RuntimeNoticeCache')) {
    $value = Get-Variable -Name $name -ValueOnly
    if ($value) { $arguments[$name] = $value }
}
$materials = & (Join-Path $PSScriptRoot 'prepare-release-materials.ps1') @arguments | ConvertFrom-Json
if ($materials.source_commit -cne $identity.commit -or $materials.product_version -cne $identity.version) { throw 'Release source changed during build.' }
$setupInputs = Join-Path $materialsOutput 'release-setup-inputs.json'
$companion = Join-Path $materialsOutput $materials.source_companion.name
$setupOutput = Join-Path $OutputDirectory 'setup-build'
$setup = & (Join-Path $PSScriptRoot 'build-local-setup.ps1') -Executable $executable -RuntimeDirectory (Join-Path $materialsOutput 'runtime') -SourceCompanion $companion -VcRedist $VcRedist -NsisArchive $NsisArchive -OutputDirectory $setupOutput -InputManifest $setupInputs | ConvertFrom-Json
# Qualify exact payload, installer sources, guide and non-installing entry points.
& (Join-Path $PSScriptRoot 'test-local-setup.ps1') -BuildDirectory $setupOutput -SourceCompanion $companion -NsisArchive $NsisArchive -InputManifest $setupInputs | Write-Host
if ($setup.setup.bytes -lt 1 -or $setup.setup.bytes -gt 512MB) { throw 'Setup exceeds the authenticated updater size limit.' }
$assets = Join-Path $OutputDirectory 'assets'
New-Item -ItemType Directory -Path $assets | Out-Null
Copy-Item -LiteralPath (Join-Path $setupOutput $setup.setup.name) -Destination $assets
Copy-Item -LiteralPath $companion -Destination $assets
Assert-ReleaseFile (Join-Path $assets $setup.setup.name) $setup.setup
Assert-ReleaseFile (Join-Path $assets $materials.source_companion.name) $materials.source_companion
$manifest = Join-Path $assets 'towavue-update-v1.txt'
[IO.File]::WriteAllText($manifest, "towavue-update-v1`n$($identity.version)`nwindows-x64`n$($setup.setup.bytes)`n$($setup.setup.sha256)`n", [Text.UTF8Encoding]::new($false))
Write-TowavueUpdateSignature $manifest (Join-Path $assets 'towavue-update-v1.sig') $keyFile $publicKey
$assetTree = Get-ReleaseTree $assets
$checksums = @($assetTree.files | ForEach-Object { $_.sha256 + '  ' + $_.name })
[IO.File]::WriteAllText((Join-Path $assets 'SHA256SUMS.txt'), ($checksums -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
$finalIdentity = Get-ReleaseSourceIdentity $repositoryRoot
if ($finalIdentity.commit -cne $identity.commit) { throw 'Release source changed before completion.' }
$result = [ordered]@{schema_version=1;scope='Built and verified local release artifacts; not installed or uploaded. Final qualification remains separate.';repository='sheetau/towavue';product_version=$identity.version;tag=('v' + $identity.version);source_commit=$identity.commit;executable=$materials.executable;assets=(Get-ReleaseTree $assets).files}
Write-ReleaseJson (Join-Path $OutputDirectory 'RELEASE.json') $result
$result | ConvertTo-Json -Depth 6
