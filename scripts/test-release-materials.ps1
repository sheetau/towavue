[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-materials.ps1')
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$testRoot = Join-Path $repositoryRoot ('target/tmp/release-material-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot | Out-Null
$utf8 = [Text.UTF8Encoding]::new($false)
$script:refusals = 0
function Assert-Refused([scriptblock]$Action, [string]$Expected) {
    try { & $Action | Out-Null }
    catch {
        if (-not $_.Exception.Message.Contains($Expected)) { throw }
        $script:refusals++
        return
    }
    throw "Expected refusal: $Expected"
}
foreach ($version in @('0.0.0','1.0.0','65535.65535.65535')) { Assert-ReleaseVersion $version }
foreach ($version in @('01.0.0','1.2','1.2.3.4','1.0.0-beta','65536.0.0','1/2/3',"1.0.0`n")) {
    Assert-Refused { Assert-ReleaseVersion $version } 'Release version must'
}
foreach ($name in @('../escape','root/../escape','C:/escape','root\escape','/escape','root/./escape','root/')) {
    Assert-Refused { Assert-ReleaseName $name } 'Unsafe release material path'
}
$sourceRoot = Join-Path $testRoot 'source'
New-Item -ItemType Directory -Path $sourceRoot | Out-Null
[IO.File]::WriteAllText((Join-Path $sourceRoot 'Cargo.toml'), "[workspace.package]`nversion = `"1.0.0`"`n",$utf8)
[IO.File]::WriteAllText((Join-Path $sourceRoot 'original.txt'), "Preserve this source.`n",$utf8)
& git -C $sourceRoot init --quiet
if ($LASTEXITCODE -ne 0) { throw 'Fixture Git init failed.' }
& git -C $sourceRoot -c core.autocrlf=false add -- Cargo.toml original.txt
& git -C $sourceRoot -c user.name='Release fixture' -c user.email='fixture@example.invalid' -c commit.gpgsign=false commit --quiet -m 'Fixture source'
if ($LASTEXITCODE -ne 0) { throw 'Fixture Git commit failed.' }
$identity = Get-ReleaseSourceIdentity $sourceRoot
if ($identity.version -cne '1.0.0') { throw 'Wrong source version.' }
$archive = Join-Path $testRoot 'source.zip'
& git -C $sourceRoot -c core.autocrlf=false -c core.eol=lf archive --format=zip --prefix=towavue/ ('--output=' + $archive) $identity.commit
if ($LASTEXITCODE -ne 0 -or (Assert-ReleaseSourceArchive $archive $sourceRoot $identity.commit) -ne 2) { throw 'Source snapshot differs.' }
$before = Get-ReleaseTree $sourceRoot
foreach ($mutation in @('changed','missing','duplicate','extra')) {
    $copy = Join-Path $testRoot ($mutation + '.zip')
    Copy-Item -LiteralPath $archive -Destination $copy
    $zip = [IO.Compression.ZipFile]::Open($copy,[IO.Compression.ZipArchiveMode]::Update)
    try {
        $name = 'towavue/original.txt'
        if ($mutation -in @('changed','missing')) { $zip.GetEntry($name).Delete() }
        if ($mutation -eq 'extra') { $name = 'towavue/extra.txt' }
        if ($mutation -ne 'missing') {
            $writer = [IO.StreamWriter]::new($zip.CreateEntry($name).Open(),$utf8)
            try { $writer.Write('Changed fixture.') } finally { $writer.Dispose() }
        }
    }
    finally { $zip.Dispose() }
    Assert-Refused { Assert-ReleaseSourceArchive $copy $sourceRoot $identity.commit } 'Source archive'
}
$untracked = Join-Path $sourceRoot 'untracked.txt'
[IO.File]::WriteAllText($untracked,'Extra input.',$utf8)
Assert-Refused { Get-ReleaseSourceIdentity $sourceRoot } 'clean committed worktree'
Remove-Item -LiteralPath $untracked
[IO.File]::AppendAllText((Join-Path $sourceRoot 'original.txt'),'Changed.',$utf8)
Assert-Refused { Get-ReleaseSourceIdentity $sourceRoot } 'clean committed worktree'
[IO.File]::WriteAllText((Join-Path $sourceRoot 'original.txt'),"Preserve this source.`n",$utf8)
# Git refreshes its own index metadata; compare the committed files themselves.
if ((Assert-ReleaseSourceArchive $archive $sourceRoot $identity.commit) -ne 2) { throw 'Source changed after refusal controls.' }
$record = Get-ReleaseRecord (Join-Path $sourceRoot 'original.txt') 'original.txt'
$wrong = [pscustomobject]@{name=$record.name;bytes=$record.bytes;sha256=('0' * 64)}
Assert-Refused { Assert-ReleaseFile (Join-Path $sourceRoot 'original.txt') $wrong } 'Release material identity differs'
Assert-Refused { Assert-ReleaseOutput $sourceRoot @($archive) } 'fresh release output'
Assert-Refused { Assert-ReleaseOutput (Join-Path $sourceRoot 'nested') @($sourceRoot) } 'overlaps an input'
Assert-Refused { Assert-ReleaseOutput (Join-Path $testRoot 'absent/child') @($sourceRoot) } 'parent must exist'
$treeRoot = Join-Path $testRoot 'tree'
New-Item -ItemType Directory -Path $treeRoot | Out-Null
Copy-Item -LiteralPath (Join-Path $sourceRoot 'original.txt') -Destination $treeRoot
$tree = Get-ReleaseTree $treeRoot
if ($tree.tree_sha256 -cne (Get-ReleaseTree $treeRoot).tree_sha256) { throw 'Repeated tree hash differs.' }
[IO.File]::AppendAllText((Join-Path $treeRoot 'original.txt'),'Changed.',$utf8)
if ($tree.tree_sha256 -ceq (Get-ReleaseTree $treeRoot).tree_sha256) { throw 'Changed tree is not detected.' }
$junction = Join-Path $testRoot 'linked-tree'
New-Item -ItemType Junction -Path $junction -Target $treeRoot | Out-Null
Assert-Refused { Resolve-ReleasePath (Join-Path $junction 'original.txt') } 'reparse points'
# No removal is needed: all fixtures are retained under the owned test root.
Write-Output "PASS: committed-source ZIP/blob equality, four ZIP mutations, dirty/untracked source, version/path/hash/output/junction refusal ($script:refusals cases), original preservation and deterministic tree hashing. Evidence: $testRoot"
