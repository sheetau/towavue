[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ArtifactDirectory,
    [Parameter(Mandatory)][string]$CacheDirectory,
    [Parameter(Mandatory)][string]$VerifierExe,
    [switch]$InstalledTrial
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-artifacts.ps1')
$repository = Split-Path -Parent $PSScriptRoot
$CacheDirectory = Resolve-ReleasePath $CacheDirectory
$VerifierExe = Resolve-ReleasePath $VerifierExe
$productionCache = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'towavue\updates'
if ([bool]$InstalledTrial -ne ($CacheDirectory -ieq $productionCache)) { throw 'The real app cache requires explicit InstalledTrial; isolated trials must use a different path.' }
if (@(Get-Process towavue -ErrorAction SilentlyContinue).Count -and $InstalledTrial) { throw 'Close every towavue process before preparing the installed trial.' }
if (-not (Test-Path -LiteralPath $VerifierExe -PathType Leaf)) { throw 'Build the native update-cache-verify example first.' }
# Authenticate all immutable release inputs before touching any cache. No private
# key is read and no release, install registration, settings or media are changed.
$artifacts = Read-TowavueReleaseArtifacts $ArtifactDirectory (Join-Path $repository 'packaging/windows/update-public-key.hex') $repository
$version = $artifacts.receipt.product_version
if ($InstalledTrial) {
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
    $key = $null
    try {
        $key = $base.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue')
        if (-not $key -or $key.GetValue('TowavuePendingUpdate') -or $key.GetValue('TowavueOwnershipId') -cnotmatch '^towavue-release-[0-9a-f]{64}$') { throw 'A complete production installation is required.' }
        $installed = $key.GetValue('DisplayVersion')
        Assert-ReleaseVersion $installed
        $exe = Join-Path (Resolve-ReleasePath $key.GetValue('InstallLocation')) 'towavue.exe'
        if ([Diagnostics.FileVersionInfo]::GetVersionInfo($exe).ProductVersion -cne $installed -or [version]$version -le [version]$installed) { throw 'The trial must be newer than the matching installed/registered version.' }
    } finally { if ($key) { $key.Dispose() }; $base.Dispose() }
}
[IO.Directory]::CreateDirectory($CacheDirectory) | Out-Null
$locks = @()
$stage = $null
$published = $false
try {
    # Serialize with both the runtime worker and external helper. Refuse any
    # existing state/generation instead of replacing an owner's pending update.
    foreach ($name in @('cache.lock','handoff.lock')) {
        $path = Resolve-ReleasePath (Join-Path $CacheDirectory $name)
        $locks += [IO.File]::Open($path,[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
    }
    $extra = @(Get-ChildItem -LiteralPath $CacheDirectory -Force | Where-Object Name -NotIn @('cache.lock','handoff.lock'))
    if ($extra.Count) { throw 'The cache is not empty; preserve its state and generations.' }
    if ($InstalledTrial -and @(Get-Process towavue -ErrorAction SilentlyContinue).Count) { throw 'The app started during preparation; close it and retry.' }
    $stageName = 'stage-' + [guid]::NewGuid().ToString('N') + [guid]::NewGuid().ToString('N').Substring(0,24)
    $stage = Join-Path $CacheDirectory $stageName
    [IO.Directory]::CreateDirectory($stage) | Out-Null
    foreach ($pair in @(@('towavue-update-v1.txt','manifest.txt'),@('towavue-update-v1.sig','manifest.sig'),@("towavue-$version-windows-x64-setup.exe",'setup.exe'))) {
        $inputFile = Join-Path $artifacts.assets_directory $pair[0]
        $record = @($artifacts.receipt.assets | Where-Object name -CEQ $pair[0])[0]
        $output = Join-Path $stage $pair[1]
        $source = [IO.File]::Open($inputFile,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
        try {
            $destination = [IO.File]::Open($output,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
            try { $source.CopyTo($destination); $destination.Flush($true) } finally { $destination.Dispose() }
            Assert-ReleaseFile $output $record
        } finally { $source.Dispose() }
    }
    $bytes = [Text.Encoding]::UTF8.GetBytes("towavue-update-state-v1`n$stageName`nready`n")
    $temporary = Join-Path $CacheDirectory ($stageName + '.state')
    $stream = [IO.File]::Open($temporary,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try { $stream.Write($bytes,0,$bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
    [IO.File]::Move($temporary,(Join-Path $CacheDirectory 'state.txt'))
    $published = $true
} finally {
    foreach ($held in $locks) { $held.Dispose() }
    # Never recursively delete or conceal failed/partially prepared evidence.
    if ($stage -and -not $published) { Write-Warning "Trial preparation stopped; inspect the owned generation: $stage" }
}
$verification = & $VerifierExe $CacheDirectory $version
if ($LASTEXITCODE -ne 0) { throw 'Native trial verification failed; do not start the app. Preserve the cache for inspection.' }
$verification | Write-Host
[pscustomobject]@{version=$version;cache_directory=$CacheDirectory;stage=$stageName;phase='ready';installed_trial=[bool]$InstalledTrial;source_commit=$artifacts.receipt.source_commit;scope='Local authenticated test input only; not downloaded, installed or published.'} | ConvertTo-Json
