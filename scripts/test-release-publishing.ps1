[CmdletBinding()]
param([string]$ArtifactDirectory)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-artifacts.ps1')
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$testRoot = Join-Path $repositoryRoot ('target/tmp/release-publishing-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot | Out-Null
$script:refusals = 0
function Assert-Refused([scriptblock]$Action, [string]$Expected) {
    try { & $Action | Out-Null }
    catch { if (-not $_.Exception.Message.Contains($Expected)) { throw }; $script:refusals++; return }
    throw "Expected refusal: $Expected"
}
function Clone-Value($Value) { return ($Value | ConvertTo-Json -Depth 20 | ConvertFrom-Json) }
# Import only the lookup function; executing the publisher would contact GitHub.
$publisherAst = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'publish-release.ps1'),[ref]$null,[ref]$null)
$lookup = $publisherAst.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Read-ReleaseByTag' },$true)
if (-not $lookup) { throw 'Draft lookup function is missing.' }
. ([scriptblock]::Create($lookup.Extent.Text))
$repository = 'fixture/repository'
$script:lookupPages = @{}
$script:lookupDetail = [pscustomobject]@{id=42;tag_name='v1.0.0';draft=$true}
function Invoke-ReleaseGitHub($Arguments) {
    $path = $Arguments[1]
    if ($path -match '/releases\?per_page=100&page=([0-9]+)$') { return $script:lookupPages[[int]$Matches[1]] }
    if ($path -ceq 'repos/fixture/repository/releases/42') { return $script:lookupDetail }
    throw "Unexpected fixture request: $path"
}
$script:lookupPages[1] = @($script:lookupDetail)
if ((Read-ReleaseByTag 'v1.0.0').id -ne 42) { throw 'Draft lookup failed.' }
if ($null -ne (Read-ReleaseByTag 'missing')) { throw 'Absent release lookup failed.' }
$script:lookupPages[1] = @(1..100 | ForEach-Object { [pscustomobject]@{id=($_+100);tag_name='older'} })
$script:lookupPages[2] = @($script:lookupDetail)
if ((Read-ReleaseByTag 'v1.0.0').id -ne 42) { throw 'Paginated draft lookup failed.' }
$script:lookupPages[1][0] = $script:lookupDetail
Assert-Refused { Read-ReleaseByTag 'v1.0.0' } 'Multiple releases'
$script:lookupPages = @{1=@([pscustomobject]@{id='../other';tag_name='v1.0.0'})}
Assert-Refused { Read-ReleaseByTag 'v1.0.0' } 'Invalid release ID'
$script:lookupPages = @{1=@([pscustomobject]@{id=42;tag_name='v1.0.0'})}
$script:lookupDetail = [pscustomobject]@{id=42;tag_name='changed'}
Assert-Refused { Read-ReleaseByTag 'v1.0.0' } 'identity changed'
$script:lookupDetail = [pscustomobject]@{id=43;tag_name='v1.0.0'}
Assert-Refused { Read-ReleaseByTag 'v1.0.0' } 'identity changed'
$script:lookupDetail = $null
Assert-Refused { Read-ReleaseByTag 'v1.0.0' } 'identity changed'
$fullPage = @(1..100 | ForEach-Object { [pscustomobject]@{id=$_;tag_name='older'} })
$script:lookupPages = @{}
foreach ($page in 1..100) { $script:lookupPages[$page] = $fullPage }
Assert-Refused { Read-ReleaseByTag 'missing' } 'lookup limit'
Write-Output 'PASS: authenticated draft lookup, pagination, absent tags, duplicate/invalid/racing identity and bounded-list refusal.'
$receipt = [pscustomobject]@{source_commit=('a' * 40);tag='v1.0.0';assets=@(
    [pscustomobject]@{name='first.zip';bytes=100;sha256=('b' * 64)},
    [pscustomobject]@{name='second.exe';bytes=200;sha256=('c' * 64)}
)}
$artifacts = [pscustomobject]@{receipt=$receipt;receipt_record=[pscustomobject]@{sha256=('d' * 64)}}
$notes = Get-TowavueDraftNotes $artifacts "First release.`n"
$plan = Get-TowavueDraftPlan $artifacts $null '' $notes
if (-not $plan.create -or $plan.missing.Count -ne 2 -or $plan.incomplete.Count) { throw 'New draft plan differs.' }
$remote = [pscustomobject]@{draft=$true;prerelease=$false;tag_name='v1.0.0';target_commitish=$receipt.source_commit;body=$notes.pending;assets=@()}
$plan = Get-TowavueDraftPlan $artifacts $remote $receipt.source_commit $notes
if ($plan.create -or $plan.missing.Count -ne 2) { throw 'Empty draft retry differs.' }
$remote.assets = @($receipt.assets | ForEach-Object { [pscustomobject]@{id=123;name=$_.name;size=$_.bytes;digest=('sha256:' + $_.sha256);state='uploaded'} })
$plan = Get-TowavueDraftPlan $artifacts $remote $receipt.source_commit $notes
if ($plan.missing.Count -or $plan.incomplete.Count -or $plan.create) { throw 'Identical draft is not a no-op.' }
$complete = Clone-Value $remote
$complete.body = $notes.complete
$complete.target_commitish = 'main'
if ((Get-TowavueDraftPlan $artifacts $complete $receipt.source_commit $notes).missing.Count) { throw 'Verified tag/branch target was not accepted.' }
$partial = Clone-Value $remote
$partial.assets = @($partial.assets[0])
if ((Get-TowavueDraftPlan $artifacts $partial $receipt.source_commit $notes).missing[0].name -cne 'second.exe') { throw 'Partial retry selects the wrong asset.' }
$starter = Clone-Value $remote
$starter.assets[1].state = 'starter'; $starter.assets[1].size = 0; $starter.assets[1].digest = $null
$plan = Get-TowavueDraftPlan $artifacts $starter $receipt.source_commit $notes
if ($plan.missing.Count -ne 1 -or $plan.incomplete.Count -ne 1 -or $plan.incomplete[0].name -cne 'second.exe') { throw 'Empty failed-upload retry differs.' }
Assert-Refused { Get-TowavueDraftPlan $artifacts $remote ('e' * 40) $notes } 'different source commit'
Assert-Refused { Get-TowavueDraftPlan $artifacts $remote '' $notes } 'does not belong'
foreach ($mutation in @('published','prerelease','tag','target','body','edited-notes','extra','duplicate','hash','size','state','missing-digest','nonempty-starter','starter-id')) {
    $bad = Clone-Value $remote
    $expected = 'Existing draft asset differs'
    switch ($mutation) {
        'published' { $bad.draft=$false; $expected='unpublished stable draft' }
        'prerelease' { $bad.prerelease=$true; $expected='unpublished stable draft' }
        'tag' { $bad.tag_name='v2.0.0'; $expected='does not belong' }
        'target' { $bad.target_commitish='foreign-branch'; $expected='does not belong' }
        'body' { $bad.body='Unrelated owner draft.'; $expected='does not belong' }
        'edited-notes' { $bad.body=$notes.complete + 'Owner edit.'; $expected='does not belong' }
        'extra' { $bad.assets += [pscustomobject]@{name='unexpected';size=1;digest='';state='uploaded'}; $expected='unexpected or duplicate' }
        'duplicate' { $bad.assets += $bad.assets[0]; $expected='unexpected or duplicate' }
        'hash' { $bad.assets[0].digest='sha256:' + ('0' * 64) }
        'size' { $bad.assets[0].size++ }
        'state' { $bad.assets[0].state='starter' }
        'missing-digest' { $bad.assets[0].digest=$null }
        'nonempty-starter' { $bad.assets[0].state='starter'; $bad.assets[0].digest=$null }
        'starter-id' { $bad.assets[0].state='starter'; $bad.assets[0].size=0; $bad.assets[0].digest=$null; $bad.assets[0].id='../other' }
    }
    Assert-Refused { Get-TowavueDraftPlan $artifacts $bad $receipt.source_commit $notes } $expected
}
if ($ArtifactDirectory) {
    $public = Join-Path $repositoryRoot 'packaging/windows/update-public-key.hex'
    $actual = Read-TowavueReleaseArtifacts $ArtifactDirectory $public
    $copy = Join-Path $testRoot 'artifact-copy'
    New-Item -ItemType Directory -Path (Join-Path $copy 'assets') | Out-Null
    Copy-Item -LiteralPath (Join-Path $ArtifactDirectory 'RELEASE.json') -Destination $copy
    foreach ($file in $actual.receipt.assets) { Copy-Item -LiteralPath (Join-Path $actual.assets_directory $file.name) -Destination (Join-Path $copy 'assets') }
    foreach ($mutation in @('hash','metadata','signature','checksums','extra','missing','duplicate-record')) {
        $changed = Clone-Value $actual.receipt
        $name = 'towavue-update-v1.txt'
        $expected = 'Release material identity differs'
        if ($mutation -eq 'signature') { $name='towavue-update-v1.sig'; $expected='Release signature verification failed' }
        if ($mutation -eq 'checksums') { $name='SHA256SUMS.txt'; $expected='Release checksum list differs' }
        $path = Join-Path $copy ('assets/' + $name)
        switch ($mutation) {
            'hash' { [IO.File]::AppendAllText($path,'Changed.',[Text.UTF8Encoding]::new($false)) }
            'metadata' { [IO.File]::WriteAllText($path,'Unsupported metadata.',[Text.UTF8Encoding]::new($false)); $expected='Release update metadata differs' }
            'signature' { $bytes=[IO.File]::ReadAllBytes($path); $bytes[0]=$bytes[0] -bxor 1; [IO.File]::WriteAllBytes($path,$bytes) }
            'checksums' { [IO.File]::WriteAllText($path,"Changed.`n",[Text.UTF8Encoding]::new($false)) }
            'extra' { $path=Join-Path $copy 'assets/extra.txt'; [IO.File]::WriteAllText($path,'Extra.'); $expected='Release asset coverage differs' }
            'missing' { Remove-Item -LiteralPath $path; $expected='Release asset coverage differs' }
            'duplicate-record' { $changed.assets[1]=$changed.assets[0]; $expected='Invalid release asset record' }
        }
        if ($mutation -in @('metadata','signature','checksums')) {
            $changed.assets = @($changed.assets | ForEach-Object { if ($_.name -ceq $name) { Get-ReleaseRecord $path $name } else { $_ } })
        }
        Write-ReleaseJson (Join-Path $copy 'RELEASE.json') $changed
        Assert-Refused { Read-TowavueReleaseArtifacts $copy $public } $expected
        if ($mutation -eq 'extra') { Remove-Item -LiteralPath $path }
        Copy-Item -LiteralPath (Join-Path $actual.assets_directory $name) -Destination (Join-Path $copy ('assets/' + $name)) -Force
        Copy-Item -LiteralPath (Join-Path $ArtifactDirectory 'RELEASE.json') -Destination $copy -Force
    }
    [void](Read-TowavueReleaseArtifacts $ArtifactDirectory $public)
    Write-Output 'PASS: real asset hashes, public-key signature, canonical metadata/checksums and all application-source Git blobs; seven altered/incomplete local artifact cases refused; original assets preserved.'
}
Write-Output "PASS: draft ownership/tag/channel checks, missing-only retry, identical no-op, empty-starter recovery and published/foreign/changed asset refusal ($script:refusals cases). No GitHub mutation or production private key use. Evidence: $testRoot"
