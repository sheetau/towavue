[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$FfmpegPrefix,
    [Parameter(Mandatory)][string]$NativeMaterialsDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [string]$PackageDirectory,
    [string]$RecipeDirectory,
    [string]$RustNoticeDirectory,
    [string]$RuntimeNoticeCache
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'application-license.ps1')
. (Join-Path $PSScriptRoot 'release-materials.ps1')
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$identity = Get-ReleaseSourceIdentity $repositoryRoot
Assert-TowavueReleaseLicense $repositoryRoot $identity.version
foreach ($name in @('Executable','FfmpegPrefix','NativeMaterialsDirectory','OutputDirectory')) {
    Set-Variable -Name $name -Value (Resolve-ReleasePath (Get-Variable -Name $name -ValueOnly))
}
Assert-ReleaseOutput $OutputDirectory @($Executable,$FfmpegPrefix,$NativeMaterialsDirectory)
if ([Diagnostics.FileVersionInfo]::GetVersionInfo($Executable).ProductVersion -cne $identity.version) { throw 'Executable and committed release versions differ.' }
$executableRecord = Get-ReleaseRecord $Executable 'towavue.exe'
$catalogInput = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/native-material-catalog.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($catalogInput.kits.Count -ne 13) { throw 'Unexpected native material kit coverage.' }
# Verify every unchanged native kit against its committed digest before staging.
$nativeTrees = @{}
foreach ($kit in $catalogInput.kits) {
    if ($kit.name -eq 'app-materials-v2') { continue }
    Assert-ReleaseName $kit.name
    $tree = Get-ReleaseTree (Join-Path $NativeMaterialsDirectory $kit.name)
    if ($tree.files.Count -ne $kit.files -or $tree.bytes -ne $kit.bytes -or $tree.tree_sha256 -cne $kit.tree_sha256) { throw "Pinned native material kit differs: $($kit.name)" }
    $nativeTrees.Add($kit.name,$tree)
}
$runtime = Get-Content -LiteralPath (Join-Path $NativeMaterialsDirectory 'native-ffmpeg-materials-v1/RUNTIME.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($runtime.Count -ne 94) { throw 'Unexpected selected runtime coverage.' }
foreach ($record in $runtime) { Assert-ReleaseName $record.name; if ($record.name.Contains('/')) { throw 'Runtime files must be adjacent.' }; Assert-ReleaseFile (Join-Path $FfmpegPrefix ('bin/' + $record.name)) $record }

New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$materials = Join-Path $OutputDirectory 'materials'
$runtimeOutput = Join-Path $OutputDirectory 'runtime'
New-Item -ItemType Directory -Path $materials,$runtimeOutput | Out-Null
foreach ($name in $nativeTrees.Keys) {
    foreach ($record in $nativeTrees[$name].files) {
        $destination = Join-Path $materials ($name + '/' + $record.name)
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $NativeMaterialsDirectory ($name + '/' + $record.name)) -Destination $destination
        Assert-ReleaseFile $destination $record
    }
}
foreach ($record in $runtime) {
    Copy-Item -LiteralPath (Join-Path $FfmpegPrefix ('bin/' + $record.name)) -Destination (Join-Path $runtimeOutput $record.name)
    Assert-ReleaseFile (Join-Path $runtimeOutput $record.name) $record
}
$rustNotices = Join-Path $OutputDirectory 'RUST-THIRD-PARTY-NOTICES.txt'
$rustArguments = @{OutputPath=$rustNotices}
if ($RustNoticeDirectory) { $rustArguments.UpstreamNoticeDirectory = $RustNoticeDirectory }
& (Join-Path $PSScriptRoot 'prepare-rust-notices.ps1') @rustArguments | Write-Host
$runtimeNotices = Join-Path $OutputDirectory 'TOWAVUE-RUST-RUNTIME-NOTICES.zip'
$runtimeArguments = @{OutputPath=$runtimeNotices;Scope='towavue'}
if ($RuntimeNoticeCache) { $runtimeArguments.CacheDirectory = $RuntimeNoticeCache }
& (Join-Path $PSScriptRoot 'prepare-rust-runtime-notices.ps1') @runtimeArguments | Write-Host
$app = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/app-material-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$licenses = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/rust-license-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$app | Add-Member -NotePropertyName release_version -NotePropertyValue $identity.version
$app.candidate = $executableRecord
$app.dependency_count = $licenses.packages.Count
$app.notice_files = @((Get-ReleaseRecord $rustNotices 'RUST-THIRD-PARTY-NOTICES.txt'),(Get-ReleaseRecord $runtimeNotices 'TOWAVUE-RUST-RUNTIME-NOTICES.zip'))
$app.runtime_inventory = Get-ReleaseRecord (Join-Path $repositoryRoot $app.runtime_inventory.name) $app.runtime_inventory.name
$profile = Get-TowavueLicenseProfile $identity.version
$app.repository_materials = @($app.repository_materials | Where-Object { $_.name -notin @('LICENSE-MIT','LICENSE-APACHE','NOTICE') } | ForEach-Object {
    $name = if ($_.name -eq 'third-party/APP-MATERIALS-README.txt') { 'third-party/RELEASE-MATERIALS-README.txt' } else { $_.name }
    Get-ReleaseRecord (Join-Path $repositoryRoot $name) $name
}) + @($profile.materials | ForEach-Object { Get-ReleaseRecord (Resolve-TowavueLicenseMaterial $repositoryRoot $_) $_ })
$appInput = Join-Path $OutputDirectory 'app-inputs.json'
Write-ReleaseJson $appInput $app
& (Join-Path $PSScriptRoot 'prepare-app-materials.ps1') -RustNotices $rustNotices -RuntimeNotices $runtimeNotices -Executable $Executable -OutputDirectory (Join-Path $materials 'app-materials-v2') -InputManifest $appInput | Write-Host
$appTree = Get-ReleaseTree (Join-Path $materials 'app-materials-v2')
$appKit = @($catalogInput.kits | Where-Object name -eq 'app-materials-v2')[0]
$appKit.files = $appTree.files.Count; $appKit.bytes = $appTree.bytes; $appKit.tree_sha256 = $appTree.tree_sha256
$appKit.description = 'Application licenses, ' + $app.dependency_count + ' locked dependency entries and the selected MSVC Rust runtime originals.'
$catalogInput | Add-Member -NotePropertyName release_version -NotePropertyValue $identity.version
$catalogInput.scope = 'Release source/notice catalog. Unchanged native kits preserve historical collection records; the companion BINDING.json identifies the selected payload.'
$catalogManifest = Join-Path $OutputDirectory 'catalog-inputs.json'
Write-ReleaseJson $catalogManifest $catalogInput
$catalog = Join-Path $OutputDirectory 'catalog'
$catalogArguments = @{MaterialsDirectory=$materials;OutputDirectory=$catalog;InputManifest=$catalogManifest}
if ($PackageDirectory) { $catalogArguments.PackageDirectory = $PackageDirectory }
if ($RecipeDirectory) { $catalogArguments.RecipeDirectory = $RecipeDirectory }
& (Join-Path $PSScriptRoot 'prepare-native-material-catalog.ps1') @catalogArguments | Write-Host

$sourceName = 'towavue-' + $identity.version + '-source.zip'
$source = Join-Path $OutputDirectory $sourceName
& git -C $repositoryRoot -c core.autocrlf=false -c core.eol=lf archive --format=zip --prefix=towavue/ ('--output=' + $source) $identity.commit
if ($LASTEXITCODE -ne 0) { throw 'Release source archive failed.' }
$sourceFiles = Assert-ReleaseSourceArchive $source $repositoryRoot $identity.commit
$candidateInput = [ordered]@{schema_version=1;release_version=$identity.version;application_source_commit=$identity.commit;application_source=(Get-ReleaseRecord $source $sourceName);catalog_inventory=(Get-ReleaseRecord (Join-Path $catalog 'FILES.json') 'FILES.json');catalog_marker=(Get-ReleaseRecord (Join-Path $catalog 'CATALOG.json') 'CATALOG.json');catalog_files=(Get-ReleaseTree $catalog).files.Count}
$candidateManifest = Join-Path $OutputDirectory 'candidate-inputs.json'
Write-ReleaseJson $candidateManifest $candidateInput
$candidate = Join-Path $OutputDirectory 'candidate'
& (Join-Path $PSScriptRoot 'prepare-candidate-materials.ps1') -Executable $Executable -RuntimeDirectory $runtimeOutput -CatalogDirectory $catalog -ApplicationSource $source -OutputDirectory $candidate -InputManifest $candidateManifest | Write-Host
$companionName = 'towavue-' + $identity.version + '-sources.zip'
$companionPath = Join-Path $OutputDirectory $companionName
$companion = & (Join-Path $PSScriptRoot 'pack-candidate-materials.ps1') -MaterialsDirectory $candidate -ArchivePath $companionPath | ConvertFrom-Json
Assert-ReleaseFile $Executable $executableRecord
$finalIdentity = Get-ReleaseSourceIdentity $repositoryRoot
if ($finalIdentity.commit -cne $identity.commit) { throw 'Committed source changed during release assembly.' }
$setupManifest = [ordered]@{schema_version=2;distribution_approved=$false;release_version=$identity.version;source_commit=$identity.commit;binary_files=95;companion_files=$companion.files;companion=(Get-ReleaseRecord $companionPath $companionName);binding=(Get-ReleaseRecord (Join-Path $candidate 'BINDING.json') 'BINDING.json')}
Write-ReleaseJson (Join-Path $OutputDirectory 'release-setup-inputs.json') $setupManifest
$result = [ordered]@{schema_version=1;scope='Exact source/material assembly; no installation, signature or publication.';product_version=$identity.version;source_commit=$identity.commit;source_files=$sourceFiles;executable=$executableRecord;source_companion=$setupManifest.companion;native_kits=$nativeTrees.Count;rust_packages=$app.dependency_count;runtime_files=$runtime.Count}
Write-ReleaseJson (Join-Path $OutputDirectory 'MATERIALS.json') $result
$result | ConvertTo-Json -Depth 6
