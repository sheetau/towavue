[CmdletBinding()]
param([string]$PackagePath)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'vc-redist-state.ps1')
$manifest = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/vc-redist-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($manifest.schema_version -ne 1 -or $manifest.architecture -ne 'x64' -or
    $manifest.minimum_version -ne $manifest.package.version -or $manifest.registry.hive -ne 'LocalMachine' -or
    $manifest.registry.view -ne 'Registry64') { throw 'Invalid VC prerequisite manifest.' }
if (-not [Environment]::Is64BitOperatingSystem) { throw 'The x64 prerequisite requires a 64-bit Windows host.' }
if ($PackagePath) {
    if (-not (Test-Path -LiteralPath $PackagePath -PathType Leaf)) { throw 'Missing VC redistributable package.' }
    $item = Get-Item -LiteralPath $PackagePath
    if ($item.Length -ne $manifest.package.bytes -or (Get-FileHash -LiteralPath $PackagePath).Hash -ne $manifest.package.sha256 -or
        $item.VersionInfo.FileVersion -ne $manifest.package.version) { throw 'VC redistributable package identity mismatch.' }
    $signature = Get-AuthenticodeSignature -LiteralPath $PackagePath
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -ne $manifest.package.signer_subject) { throw 'VC redistributable signature is not verified.' }
}
# Explicit 64-bit view even when run by 32-bit PowerShell. This key is read-only.
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine,[Microsoft.Win32.RegistryView]::Registry64)
$snapshot = $null
try {
    $key = $base.OpenSubKey($manifest.registry.key,$false)
    try {
        if ($key) {
            $snapshot = @{}
            foreach ($name in $manifest.registry.fields) { $snapshot[$name] = $key.GetValue($name,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) }
        }
    }
    finally { if ($key) { $key.Dispose() } }
}
finally { $base.Dispose() }
$state = Get-VcRedistState -Snapshot $snapshot -MinimumVersion $manifest.minimum_version
[pscustomobject]@{
    architecture=$manifest.architecture
    minimum_version=$manifest.minimum_version
    registry_view=$manifest.registry.view
    state=$state.state
    reason=$state.reason
    installed_version=$state.installed_version
    package_verified=[bool]$PackagePath
    action=if ($state.state -eq 'satisfied') { 'skip_prerequisite' } elseif ($state.state -eq 'required') { 'installation_required' } else { 'stop_and_diagnose' }
    scope='Registry/package inspection only; no installation, runtime health or distribution approval.'
} | ConvertTo-Json
