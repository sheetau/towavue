[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$features = Get-Content (Join-Path $repositoryRoot 'docs/ffmpeg-native-features.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$base = Get-Content (Join-Path $repositoryRoot 'docs/msys2-toolchain-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$media = Get-Content (Join-Path $repositoryRoot 'docs/msys2-media-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$sources = Get-Content (Join-Path $repositoryRoot 'docs/ffmpeg-build-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$enabled = @($features.configure_flags | Where-Object { $_ -like '--enable-*' } | ForEach-Object { $_.Substring(9) })
if ($features.schema_version -ne 1 -or $features.configure_flags.Count -ne 81 -or
    @($features.configure_flags | Sort-Object -Unique).Count -ne 81 -or $enabled.Count -ne 61) {
    throw 'The retained native FFmpeg feature set is incomplete or duplicated.'
}
$mapped = @($features.package_features.PSObject.Properties.Name) + @($features.source_features.PSObject.Properties.Name) + @($features.builtin_features)
if ($mapped.Count -ne 61 -or @(Compare-Object ($enabled | Sort-Object) ($mapped | Sort-Object)).Count -ne 0) {
    throw 'Every enabled FFmpeg feature must have exactly one preparation route.'
}
$packages = @($base.packages.name) + @($media.packages.name)
foreach ($name in $features.package_features.PSObject.Properties.Value | ForEach-Object { $_ }) {
    if (('mingw-w64-x86_64-' + $name) -notin $packages) { throw "Unpinned native feature package: $name" }
}
foreach ($revision in $features.source_features.PSObject.Properties.Value) {
    if ($revision -notin $sources.inputs.revision) { throw "Unpinned source feature: $revision" }
}
$baseline = Join-Path $repositoryRoot 'target/tmp/ffmpeg-registry-provenance-20260903/image-config.json'
if (Test-Path -LiteralPath $baseline) {
    if ((Get-FileHash $baseline).Hash.ToLowerInvariant() -ne $features.baseline_config_sha256) { throw 'Original image configuration hash mismatch.' }
    $config = Get-Content $baseline -Raw | ConvertFrom-Json
    $original = ($config.config.Env | Where-Object { $_ -like 'FF_CONFIGURE=*' }).Substring(13)
    if (($features.configure_flags -join ' ') -cne $original) { throw 'Native flags differ from the original configuration.' }
    Write-Output 'All 81 flags exactly match the hash-verified original image configuration.'
} else {
    Write-Output 'SKIP original image configuration comparison: ignored provenance input is absent.'
}
Write-Output 'Native preparation map passed: 52 package features, five source features and four built-in features. This does not prove successful FFmpeg configuration or equivalent binaries.'
