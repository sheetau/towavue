[CmdletBinding(DefaultParameterSetName='Build')]
param(
    [Parameter(Mandatory,ParameterSetName='Build')][string]$FfmpegPrefix,
    [Parameter(Mandatory,ParameterSetName='Build')][string]$NativeMaterialsDirectory,
    [Parameter(Mandatory,ParameterSetName='Build')][string]$VcRedist,
    [Parameter(Mandatory,ParameterSetName='Build')][string]$NsisArchive,
    [Parameter(Mandatory,ParameterSetName='Build')][string]$OutputDirectory,
    [Parameter(Mandatory,ParameterSetName='Prepared')][string]$PreparedDirectory,
    [Parameter(ParameterSetName='Build')][string]$CargoTargetDirectory,
    [Parameter(ParameterSetName='Build')][string]$PackageDirectory,
    [Parameter(ParameterSetName='Build')][string]$RecipeDirectory,
    [Parameter(ParameterSetName='Build')][string]$RustNoticeDirectory,
    [Parameter(ParameterSetName='Build')][string]$RuntimeNoticeCache,
    [switch]$CheckOnly
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-artifacts.ps1')
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$identity = Get-ReleaseSourceIdentity $repositoryRoot
$repository = 'sheetau/towavue'
$githubCli = (Get-Command gh.exe -ErrorAction Stop).Source
$origin = & git -C $repositoryRoot remote get-url origin
if ($LASTEXITCODE -ne 0 -or $origin -cnotmatch '^(https://github\.com/sheetau/towavue(?:\.git)?|git@github\.com:sheetau/towavue(?:\.git)?)\z') { throw 'Release publishing requires the sheetau/towavue origin.' }
$branch = & git -C $repositoryRoot symbolic-ref --short HEAD
if ($LASTEXITCODE -ne 0 -or $branch -cne 'main') { throw 'Release publishing requires the clean main branch.' }
$notesPath = Join-Path $repositoryRoot ('docs/releases/' + $identity.version + '.md')
$notesText = Get-Content -LiteralPath $notesPath -Raw -Encoding UTF8
if (-not $notesText.Trim()) { throw 'Committed release notes are required.' }

function Invoke-ReleaseGitHub([string[]]$Arguments, [switch]$AllowNotFound, [switch]$Raw) {
    $errorFile = Join-Path ([IO.Path]::GetTempPath()) ('towavue-gh-' + [guid]::NewGuid().ToString('N') + '.txt')
    $savedPreference = $ErrorActionPreference
    try {
        if ($Arguments[0] -ceq 'api') {
            $Arguments = @('api','-H','Accept: application/vnd.github+json','-H','X-GitHub-Api-Version: 2022-11-28') + $Arguments[1..($Arguments.Count - 1)]
        }
        $ErrorActionPreference = 'Continue'
        $output = @(& $githubCli @Arguments 2> $errorFile)
        $exitCode = $LASTEXITCODE
        $ErrorActionPreference = $savedPreference
        $text = $output -join "`n"
        if ($exitCode -ne 0) {
            $errorResponse = $null
            try { $errorResponse = $text | ConvertFrom-Json } catch { }
            if ($AllowNotFound -and $errorResponse.status -eq 404) { return $null }
            $diagnostic = [IO.File]::ReadAllText($errorFile).Trim()
            if ($diagnostic.Length -gt 2048) { $diagnostic = $diagnostic.Substring(0,2048) }
            throw "GitHub release operation failed (exit $exitCode): $text $diagnostic"
        }
        if ($Raw) { return $text }
        if ($text) { return ($text | ConvertFrom-Json) }
    }
    finally {
        $ErrorActionPreference = $savedPreference
        if (Test-Path -LiteralPath $errorFile) { Remove-Item -LiteralPath $errorFile }
    }
}
function Read-ReleaseTagCommit([string]$Tag) {
    $reference = Invoke-ReleaseGitHub @('api',"repos/$repository/git/ref/tags/$Tag") -AllowNotFound
    if ($null -eq $reference) { return $null }
    $object = $reference.object
    for ($depth = 0; $depth -lt 8; $depth++) {
        if ($object.sha -cnotmatch '^[0-9a-f]{40}\z') { throw 'Invalid remote tag object.' }
        if ($object.type -ceq 'commit') { return $object.sha }
        if ($object.type -cne 'tag') { throw 'Release tag does not reference a commit.' }
        $tagObject = Invoke-ReleaseGitHub @('api',"repos/$repository/git/tags/$($object.sha)")
        $object = $tagObject.object
    }
    throw 'Release tag nesting exceeds the verification limit.'
}
function Read-ReleaseByTag([string]$Tag) {
    # The tag endpoint returns published releases only. Authenticated listing
    # also includes drafts; resolve the unique matching release by its ID.
    $matchingReleases = [Collections.Generic.List[object]]::new()
    for ($page = 1; $page -le 100; $page++) {
        $batch = @(Invoke-ReleaseGitHub @('api',"repos/$repository/releases?per_page=100&page=$page"))
        foreach ($candidate in $batch) {
            if ($candidate.tag_name -ceq $Tag) { $matchingReleases.Add($candidate) }
        }
        if ($matchingReleases.Count -gt 1) { throw 'Multiple releases reference the requested tag.' }
        if ($batch.Count -lt 100) {
            if (-not $matchingReleases.Count) { return $null }
            $releaseId = [string]$matchingReleases[0].id
            if ($releaseId -cnotmatch '^[1-9][0-9]*\z') { throw 'Invalid release ID.' }
            $release = Invoke-ReleaseGitHub @('api',"repos/$repository/releases/$releaseId")
            if ($null -eq $release -or $release.tag_name -cne $Tag -or [string]$release.id -cne $releaseId) { throw 'Release identity changed during lookup.' }
            return $release
        }
    }
    throw 'Release listing exceeds the lookup limit.'
}
function Read-ReleaseState($Artifacts, $Notes) {
    $tagCommit = Read-ReleaseTagCommit $Artifacts.receipt.tag
    $release = Read-ReleaseByTag $Artifacts.receipt.tag
    $plan = Get-TowavueDraftPlan $Artifacts $release $tagCommit $Notes
    return [pscustomobject]@{release=$release;plan=$plan;tag_commit=$tagCommit}
}
# Read-only authentication/repository access before a potentially long build.
$remote = Invoke-ReleaseGitHub @('api',"repos/$repository")
if ($remote.full_name -cne $repository -or -not $remote.permissions.push) { throw 'GitHub credentials cannot publish this repository.' }
$initialTag = Read-ReleaseTagCommit ('v' + $identity.version)
if ($initialTag -and $initialTag -cne $identity.commit) { throw 'Release tag belongs to a different source commit.' }
$initialRelease = Read-ReleaseByTag ('v' + $identity.version)
if ($null -ne $initialRelease -and ($initialRelease.draft -ne $true -or $initialRelease.prerelease -ne $false)) { throw 'Only an unpublished stable draft can be uploaded.' }
if ($PSCmdlet.ParameterSetName -eq 'Build') {
    $buildArguments = @{}
    foreach ($name in @('FfmpegPrefix','NativeMaterialsDirectory','VcRedist','NsisArchive','OutputDirectory','CargoTargetDirectory','PackageDirectory','RecipeDirectory','RustNoticeDirectory','RuntimeNoticeCache')) {
        $value = Get-Variable -Name $name -ValueOnly
        if ($value) { $buildArguments[$name] = $value }
    }
    & (Join-Path $PSScriptRoot 'build-release-artifacts.ps1') @buildArguments | Write-Host
    $PreparedDirectory = $OutputDirectory
}
$PreparedDirectory = Resolve-ReleasePath $PreparedDirectory
$artifacts = Read-TowavueReleaseArtifacts $PreparedDirectory (Join-Path $repositoryRoot 'packaging/windows/update-public-key.hex')
if ($artifacts.receipt.source_commit -cne $identity.commit -or $artifacts.receipt.product_version -cne $identity.version) { throw 'Prepared assets do not belong to the clean current source commit.' }
$notes = Get-TowavueDraftNotes $artifacts $notesText
$state = Read-ReleaseState $artifacts $notes
if ($CheckOnly) {
    [pscustomobject]@{source_commit=$identity.commit;tag=$artifacts.receipt.tag;create_draft=$state.plan.create;missing_assets=@($state.plan.missing | ForEach-Object { $_.name });incomplete_placeholders=@($state.plan.incomplete | ForEach-Object { $_.name });scope='Read-only GitHub preflight; no push, tag, upload or release change.'} | ConvertTo-Json -Depth 5
    return
}
$current = Get-ReleaseSourceIdentity $repositoryRoot
if ($current.commit -cne $identity.commit) { throw 'Source changed before draft publication.' }
# Ordinary fast-forward push only. No force, retagging or history rewrite.
& git -C $repositoryRoot push origin HEAD:refs/heads/main
if ($LASTEXITCODE -ne 0) { throw 'Release source push failed.' }
$state = Read-ReleaseState $artifacts $notes
if (-not $state.tag_commit) {
    & git -C $repositoryRoot push origin ($identity.commit + ':refs/tags/' + $artifacts.receipt.tag)
    if ($LASTEXITCODE -ne 0) { throw 'Release tag push failed.' }
}
$pendingNotes = Join-Path $PreparedDirectory 'draft-pending.md'
$completeNotes = Join-Path $PreparedDirectory 'draft-complete.md'
[IO.File]::WriteAllText($pendingNotes,$notes.pending,[Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText($completeNotes,$notes.complete,[Text.UTF8Encoding]::new($false))
$state = Read-ReleaseState $artifacts $notes
if ($state.plan.create) {
    [void](Invoke-ReleaseGitHub @('release','create',$artifacts.receipt.tag,'--repo',$repository,'--draft','--verify-tag','--target',$identity.commit,'--title',('towavue ' + $identity.version),'--notes-file',$pendingNotes) -Raw)
}
$state = Read-ReleaseState $artifacts $notes
foreach ($placeholder in $state.plan.incomplete) {
    $current = Read-ReleaseState $artifacts $notes
    $same = @($current.plan.incomplete | Where-Object { $_.id -eq $placeholder.id -and $_.name -ceq $placeholder.name })
    if ($same.Count -ne 1) { throw 'Incomplete upload changed before retry.' }
    [void](Invoke-ReleaseGitHub @('api','--method','DELETE',"repos/$repository/releases/assets/$($placeholder.id)"))
}
$state = Read-ReleaseState $artifacts $notes
foreach ($record in $state.plan.missing) {
    $current = Read-ReleaseState $artifacts $notes
    if ($record.name -cnotin @($current.plan.missing.name)) { continue }
    $path = Join-Path $artifacts.assets_directory $record.name
    Assert-ReleaseFile $path $record
    # Never use --clobber. A competing upload cannot replace an existing asset.
    [void](Invoke-ReleaseGitHub @('release','upload',$artifacts.receipt.tag,$path,'--repo',$repository) -Raw)
    $verified = Read-ReleaseState $artifacts $notes
    if ($record.name -cin @($verified.plan.missing.name)) { throw 'Uploaded release asset is incomplete.' }
}
# Recheck local files, signature and source, and every remote asset digest before
# removing the incomplete-upload notice. This command never publishes a release.
[void](Read-TowavueReleaseArtifacts $PreparedDirectory (Join-Path $repositoryRoot 'packaging/windows/update-public-key.hex'))
$state = Read-ReleaseState $artifacts $notes
if ($state.plan.create -or $state.plan.missing.Count -or $state.plan.incomplete.Count) { throw 'Draft release is incomplete.' }
if ($state.release.body -cne $notes.complete) {
    [void](Invoke-ReleaseGitHub @('release','edit',$artifacts.receipt.tag,'--repo',$repository,'--notes-file',$completeNotes,'--title',('towavue ' + $identity.version)) -Raw)
}
$state = Read-ReleaseState $artifacts $notes
if ($state.release.body -cne $notes.complete -or $state.plan.missing.Count -or $state.plan.incomplete.Count) { throw 'Draft final verification failed.' }
$result = [ordered]@{schema_version=1;draft=$true;repository=$repository;tag=$artifacts.receipt.tag;source_commit=$identity.commit;release_id=$state.release.id;url=$state.release.html_url;receipt_sha256=$artifacts.receipt_record.sha256;assets=@($state.release.assets | Select-Object id,name,size,digest);scope='Verified GitHub draft; publication remains an owner action.'}
Write-ReleaseJson (Join-Path $PreparedDirectory 'DRAFT.json') $result
$worktree = @(& git -C $repositoryRoot status --porcelain=v1 --untracked-files=normal)
if ($LASTEXITCODE -ne 0 -or $worktree.Count) { throw 'Worktree changed during upload; draft is retained for inspection.' }
$result | ConvertTo-Json -Depth 6
