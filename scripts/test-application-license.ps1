[CmdletBinding()]
param([Parameter(Mandatory)][string]$PreparedDirectory)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'application-license.ps1')
. (Join-Path $PSScriptRoot 'release-materials.ps1')
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$prepared = (Resolve-Path -LiteralPath $PreparedDirectory).Path
$materials = Join-Path $prepared 'materials-build'
$executable = Join-Path $prepared 'application/towavue.exe'
$testRoot = Join-Path $repositoryRoot ('target/tmp/application-license-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot | Out-Null
$generator = Join-Path $PSScriptRoot 'prepare-app-materials.ps1'
$arguments = @{
    Executable=$executable
    RustNotices=(Join-Path $materials 'RUST-THIRD-PARTY-NOTICES.txt')
    RuntimeNotices=(Join-Path $materials 'TOWAVUE-RUST-RUNTIME-NOTICES.zip')
}
function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function Assert-Refused([scriptblock]$Action, [string]$Message) {
    try { & $Action | Out-Null } catch {
        if (-not $_.Exception.Message.Contains($Message)) { throw }
        return
    }
    throw "Expected refusal: $Message"
}
$historical = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/app-material-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$mit = @($historical.repository_materials | Where-Object name -eq 'LICENSE-MIT')[0]
Assert-ReleaseFile (Resolve-TowavueLicenseMaterial $repositoryRoot 'LICENSE-MIT') $mit
Assert-True ((Get-TowavueLicenseProfile '').id -ceq 'MIT OR Apache-2.0') 'Evaluation license changed.'
Assert-Refused { Get-TowavueLicenseProfile '1.0.3-beta' } 'Invalid application license release version.'
Assert-TowavueReleaseLicense $repositoryRoot '1.0.3'
Assert-Refused { Assert-TowavueReleaseLicense $repositoryRoot '1.0.2' } 'Application license and release version differ.'

# Reuse verified notice/runtime bytes and a historical EXE as a material-only
# fixture. These version labels do not produce or qualify a release executable.
foreach ($version in @('1.0.2','1.0.3','1.1.0')) {
    $manifest = Get-Content -LiteralPath (Join-Path $materials 'app-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $manifest.release_version = $version
    $manifest.candidate = Get-ReleaseRecord $executable 'towavue.exe'
    $manifest.repository_materials = @($manifest.repository_materials | Where-Object { $_.name -notin @('LICENSE-MIT','LICENSE-APACHE','NOTICE') } | ForEach-Object {
        Get-ReleaseRecord (Join-Path $repositoryRoot $_.name) $_.name
    })
    $expected = if ($version -eq '1.0.2') { 'MIT OR Apache-2.0' } else { 'Apache-2.0' }
    $profile = Get-TowavueLicenseProfile $version
    Assert-True ($profile.id -ceq $expected) 'Wrong license at the release boundary.'
    $manifest.repository_materials += @($profile.materials | ForEach-Object { Get-ReleaseRecord (Resolve-TowavueLicenseMaterial $repositoryRoot $_) $_ })
    $arguments.InputManifest = Join-Path $testRoot ($version + '.json')
    $arguments.OutputDirectory = Join-Path $testRoot $version
    Write-ReleaseJson $arguments.InputManifest $manifest
    & $generator @arguments | Out-Null
    $links = @{}
    foreach ($name in $profile.materials) {
        $record = @($manifest.repository_materials | Where-Object name -ceq $name)[0]
        Assert-ReleaseFile (Join-Path $arguments.OutputDirectory $name) $record
        $links[$name] = $name
    }
    $html = Get-TowavueLicenseLinks $version $links
    Assert-True ($html.Contains('href="LICENSE-APACHE"')) 'Apache link missing.'
    if ($version -ne '1.0.2') {
        Assert-True ($html.Contains('href="NOTICE"') -and -not $html.Contains('href="LICENSE-MIT"')) 'Current links offer a stale license or omit NOTICE.'
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $arguments.OutputDirectory 'LICENSE-MIT'))) 'Current kit includes historical MIT.'
    }
    if ($version -eq '1.0.3') {
        $links.Remove('NOTICE')
        Assert-Refused { Get-TowavueLicenseLinks $version $links } 'Missing application license link: NOTICE'
        $original = $manifest | ConvertTo-Json -Depth 12
        foreach ($mutation in @('missing-notice','corrupt-notice','stale-mit')) {
            $changed = $original | ConvertFrom-Json
            $message = switch ($mutation) {
                'missing-notice' {
                    $changed.repository_materials = @($changed.repository_materials | Where-Object name -ne 'NOTICE')
                    'Missing application license material: NOTICE'
                }
                'corrupt-notice' {
                    @($changed.repository_materials | Where-Object name -eq 'NOTICE')[0].sha256 = '0' * 64
                    'Application material checksum mismatch:'
                }
                'stale-mit' {
                    $changed.repository_materials += Get-ReleaseRecord (Resolve-TowavueLicenseMaterial $repositoryRoot 'LICENSE-MIT') 'LICENSE-MIT'
                    'Current application materials must not offer the historical MIT license.'
                }
            }
            $arguments.OutputDirectory = Join-Path $testRoot $mutation
            Write-ReleaseJson $arguments.InputManifest $changed
            Assert-Refused { & $generator @arguments } $message
            Assert-True (-not (Test-Path -LiteralPath $arguments.OutputDirectory)) 'Rejected manifest created output.'
        }
    }
}
Write-Output "PASS: legacy/evaluation and 1.0.3+ license selection, exact material copies, guide links, missing/corrupt NOTICE and stale MIT refusals. Material-only fixtures, not release qualification. Evidence: $testRoot"
