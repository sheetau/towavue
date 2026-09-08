[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$trialRoot = Join-Path $repositoryRoot ('target/tmp/setup-prerequisite-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $trialRoot | Out-Null
$utf8 = [Text.UTF8Encoding]::new($false)
$wrapper = Join-Path $repositoryRoot 'packaging/windows/prerequisite.ps1'
$wrapperHash = (Get-FileHash -LiteralPath $wrapper).Hash
$powershell = Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'
$cases = @(
    @{name='inspect satisfied';mode='Inspect';before='satisfied';code=0;starts=0},
    @{name='inspect required';mode='Inspect';before='required';code=10;starts=0},
    @{name='inspect unknown';mode='Inspect';before='unknown';code=20;starts=0},
    @{name='install skip';mode='Install';before='satisfied';code=0;starts=0},
    @{name='install unknown';mode='Install';before='unknown';code=20;starts=0},
    @{name='install success';mode='Install';before='required';after='satisfied';native=0;code=0;starts=1},
    @{name='reboot required';mode='Install';before='required';after='satisfied';native=3010;code=3010;starts=1},
    @{name='postcheck required';mode='Install';before='required';after='required';native=0;code=20;starts=1},
    @{name='postcheck unknown';mode='Install';before='required';after='unknown';native=0;code=20;starts=1},
    @{name='reboot but missing';mode='Install';before='required';after='required';native=3010;code=20;starts=1},
    @{name='cancelled';mode='Install';before='required';after='satisfied';native=1602;code=20;starts=1},
    @{name='busy';mode='Install';before='required';after='satisfied';native=1618;code=20;starts=1},
    @{name='different version';mode='Install';before='required';after='satisfied';native=1638;code=20;starts=1},
    @{name='wrapped failure';mode='Install';before='required';after='satisfied';native=-2147023293;code=20;starts=1},
    @{name='uac rejected';mode='Install';before='required';launch_error=$true;code=20;starts=1},
    @{name='reader fails';mode='Inspect';reader_error=$true;code=20;starts=0}
)
# The production wrapper is unmodified. Only its reader is replaced in a fresh child
# directory; the fake package does not exist. A child-scoped command mock cannot
# launch Microsoft code, write a registry key or change the parent's command table.
$readerFixture = @'
param([string]$PackagePath)
$global:FixtureRoot = $PSScriptRoot
$global:FixtureCase = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'case.json') -Raw | ConvertFrom-Json
function global:Start-Process {
    param($FilePath,$ArgumentList,$Verb,$WindowStyle,$WorkingDirectory,[switch]$PassThru)
    if ($FilePath -ne (Join-Path $global:FixtureRoot 'does-not-exist.exe') -or
        ($ArgumentList -join ' ') -ne '/install /norestart' -or $Verb -ne 'RunAs' -or
        $WindowStyle -ne 'Normal' -or $WorkingDirectory -ne $global:FixtureRoot -or -not $PassThru) { throw 'Unexpected native launch contract.' }
    [IO.File]::WriteAllText((Join-Path $global:FixtureRoot 'started.txt'),'mock only')
    if ($global:FixtureCase.launch_error) { throw 'Simulated UAC cancellation.' }
    $process = [pscustomobject]@{Id=123;StartTime=[datetime]'2000-01-01T00:00:00Z';ExitCode=[int]$global:FixtureCase.native}
    $process | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value {
        [IO.File]::WriteAllText((Join-Path $global:FixtureRoot 'waited.txt'),'mock only')
    }
    return $process
}
if ($global:FixtureCase.reader_error) { throw 'Simulated package/registry failure.' }
$state = if (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'waited.txt')) { $global:FixtureCase.after } else { $global:FixtureCase.before }
[pscustomobject]@{state=$state;scope='Synthetic child-process fixture only.'} | ConvertTo-Json
'@
foreach ($case in $cases) {
    $directory = Join-Path $trialRoot $case.name.Replace(' ','-')
    New-Item -ItemType Directory -Path $directory | Out-Null
    $copy = Join-Path $directory 'prerequisite.ps1'
    Copy-Item -LiteralPath $wrapper -Destination $copy
    [IO.File]::WriteAllText((Join-Path $directory 'case.json'),($case | ConvertTo-Json),$utf8)
    [IO.File]::WriteAllText((Join-Path $directory 'get-vc-redist-status.ps1'),$readerFixture,$utf8)
    $arguments = '-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "{0}" -Mode {1} -PackagePath "{2}"' -f $copy,$case.mode,(Join-Path $directory 'does-not-exist.exe')
    $process = Start-Process -FilePath $powershell -ArgumentList $arguments -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $directory 'stdout.txt') -RedirectStandardError (Join-Path $directory 'stderr.txt')
    [void]$process.Handle
    if (-not $process.WaitForExit(15000)) { throw "Checker still running; do not restart: PID $($process.Id), start UTC $($process.StartTime.ToUniversalTime().ToString('o'))." }
    if ($process.ExitCode -ne $case.code) { throw "Wrong checker exit for $($case.name): $($process.ExitCode), expected $($case.code). Evidence: $directory" }
    $started = Test-Path -LiteralPath (Join-Path $directory 'started.txt')
    if ([int]$started -ne $case.starts) { throw "Wrong package launch count: $($case.name)" }
    $waited = Test-Path -LiteralPath (Join-Path $directory 'waited.txt')
    if ($waited -ne ($case.starts -eq 1 -and -not $case.launch_error)) { throw "Package was not waited on: $($case.name)" }
    if ((Get-FileHash -LiteralPath $copy).Hash -ne $wrapperHash) { throw 'Test mutated the production wrapper copy.' }
}
if ((Get-FileHash -LiteralPath $wrapper).Hash -ne $wrapperHash) { throw 'Original prerequisite wrapper changed.' }
Write-Output "PASS: $($cases.Count) synthetic prerequisite UI/return/postcheck cases. No actual redistributable was executed. Evidence: $trialRoot"
