[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ArtifactDirectory,
    [Parameter(Mandatory)][string]$VerifierExe
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-materials.ps1')
$repository = Split-Path -Parent $PSScriptRoot
$root = Join-Path $repository ('target/tmp/local-update-trial-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($root) | Out-Null
$cache = Join-Path $root 'cache'
$prepare = Join-Path $PSScriptRoot 'prepare-local-update-trial.ps1'
$VerifierExe = Resolve-ReleasePath $VerifierExe
$arguments = @{ArtifactDirectory=$ArtifactDirectory;VerifierExe=$VerifierExe;CacheDirectory=$cache}
$refusals = 0
function Refused([scriptblock]$Action,[string]$Expected) {
    $failed = $false
    try { & $Action | Out-Null } catch { if (-not $_.Exception.Message.Contains($Expected)) { throw }; $failed = $true }
    if (-not $failed) { throw 'Expected refusal.' }
    $script:refusals++
}
Refused { & $prepare @arguments -InstalledTrial } 'explicit InstalledTrial'
if (Test-Path -LiteralPath $cache) { throw 'Rejected scope created a cache.' }
$trial = & $prepare @arguments | ConvertFrom-Json
if ($trial.phase -cne 'ready' -or $trial.installed_trial) { throw 'Wrong isolated trial state.' }
$before = (Get-ReleaseTree $cache).tree_sha256
Refused { & $prepare @arguments } 'cache is not empty'
if ((Get-ReleaseTree $cache).tree_sha256 -cne $before) { throw 'Refused duplicate changed the cached update.' }
$tampered = Join-Path $root 'tampered'
[IO.Directory]::CreateDirectory($tampered) | Out-Null
foreach ($record in (Get-ReleaseTree $cache).files) {
    $destination = Join-Path $tampered $record.name
    [IO.Directory]::CreateDirectory((Split-Path -Parent $destination)) | Out-Null
    Copy-Item -LiteralPath (Join-Path $cache $record.name) -Destination $destination
}
$payload = Join-Path (Join-Path $tampered $trial.stage) 'setup.exe'
$stream = [IO.File]::Open($payload,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
try { $stream.Position = $stream.Length - 1; $byte = $stream.ReadByte(); $stream.Position--; $stream.WriteByte($byte -bxor 1); $stream.Flush($true) } finally { $stream.Dispose() }
$output = Join-Path $root 'tamper.stdout.log'
$errorOutput = Join-Path $root 'tamper.stderr.log'
$process = Start-Process -FilePath $VerifierExe -ArgumentList ('"' + $tampered + '" "' + $trial.version + '"') -WindowStyle Hidden -PassThru -RedirectStandardOutput $output -RedirectStandardError $errorOutput
[void]$process.Handle
$process.WaitForExit()
if ($process.ExitCode -eq 0 -or -not ([IO.File]::ReadAllText($errorOutput).Contains('hash does not match'))) { throw 'Native verifier did not reject the actual altered Setup.' }
$script:refusals++
if ((Get-ReleaseTree $cache).tree_sha256 -cne $before) { throw 'Original authenticated cache changed during tamper trial.' }
$result = [ordered]@{scope='Isolated local cache preparation only; no production cache, registration, private key or application process changed.';native_verified=$true;refusals=$refusals;original_preserved=$true;trial=$trial}
Write-ReleaseJson (Join-Path $root 'RESULT.json') $result
Write-Output "PASS: native authenticated ready cache, $refusals scope/duplicate/tamper refusals and original preservation. Evidence: $root"
